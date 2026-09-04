/**
 * One open serial port, and the two things this tool does with it.
 *
 * There is exactly one reader on a port, so both jobs are served from a single read loop:
 * every chunk goes to the subscribers (which is how the monitor sees traffic) and, while a
 * request is outstanding, to whatever is waiting for a reply.
 *
 * Watching and talking are still mutually exclusive at the level above this — a half-duplex
 * RS-485 line has room for one master, and if the fan controller is already polling, this tool
 * must not also be issuing requests. `connection.ts` does not enforce that; it just makes both
 * possible on the same port.
 */

import { isValid } from "../modbus/crc";
import { responseLength } from "../modbus/pdu";

export type Parity = "none" | "even" | "odd";

export type PortSettings = {
  baudRate: number;
  parity: Parity;
  dataBits: 7 | 8;
  stopBits: 1 | 2;
};

/** What fan-controller and the RadiCal manual both use */
export const defaultSettings: PortSettings = {
  baudRate: 19_200,
  parity: "even",
  dataBits: 8,
  stopBits: 1,
};

/** Bit rates offered in the UI. The RadiCal supports all of these; other devices support fewer */
export const baudRates = [1200, 2400, 4800, 9600, 14_400, 19_200, 38_400, 57_600, 115_200] as const;

export type Chunk = { bytes: Uint8Array; at: number };
export type Subscriber = (chunk: Chunk) => void;

export class RequestFailed extends Error {
  constructor(
    message: string,
    readonly reason: "timeout" | "closed" | "noise",
    /**
     * What arrived while waiting that was neither the echo of the request nor an answer. Empty for
     * a line that stayed silent, which is a different fault: silence is a wrong address, a wrong
     * bit rate or a wire, whereas noise is something that talked and was not understood
     */
    readonly stray: Uint8Array = new Uint8Array(0),
  ) {
    super(message);
    this.name = "RequestFailed";
  }
}

export class Connection {
  #reader: ReadableStreamDefaultReader<Uint8Array> | undefined;
  #writer: WritableStreamDefaultWriter<Uint8Array> | undefined;
  #subscribers = new Set<Subscriber>();
  #reading: Promise<void> | undefined;
  #closing = false;

  constructor(readonly port: SerialPort) {}

  get isOpen(): boolean {
    return this.#reader !== undefined;
  }

  async open(settings: PortSettings): Promise<void> {
    if (this.isOpen) return;

    await this.port.open({
      baudRate: settings.baudRate,
      parity: settings.parity,
      dataBits: settings.dataBits,
      stopBits: settings.stopBits,
    });

    if (!this.port.readable || !this.port.writable) {
      throw new Error("The port opened without a readable or writable stream");
    }

    this.#closing = false;
    this.#reader = this.port.readable.getReader();
    this.#writer = this.port.writable.getWriter();
    this.#reading = this.#readLoop();
  }

  async close(): Promise<void> {
    if (!this.isOpen) return;

    this.#closing = true;

    // Cancelling makes the pending read() resolve, which is what lets the loop finish. Without it
    // the loop stays parked on a read that never completes and the port cannot be released
    await this.#reader?.cancel().catch(() => undefined);
    await this.#reading?.catch(() => undefined);

    this.#reader?.releaseLock();
    this.#writer?.releaseLock();
    this.#reader = undefined;
    this.#writer = undefined;

    await this.port.close().catch(() => undefined);
  }

  subscribe(subscriber: Subscriber): () => void {
    this.#subscribers.add(subscriber);
    return () => this.#subscribers.delete(subscriber);
  }

  /**
   * Sends a frame and waits for the device's answer.
   *
   * The reply is found by checksum rather than by counting bytes, which also steps over the echo
   * of the request itself: some RS-485 adapters put what they transmit back on the receive line,
   * and `debug-listener` has a flag for exactly that. An echo is a valid frame, so it has to be
   * ruled out by shape — same address, but the request's shape rather than a response's
   *
   * An attempt that heard nothing at all is not retried: silence means the device is not listening
   * on this address or at this bit rate, and asking again only spends another timeout confirming
   * it. An attempt that heard *something* is retried once, because bytes on the line prove a device
   * is there and that this particular exchange was spoiled rather than impossible. The relay module
   * is why: it greets the line in ASCII when it powers up, and that greeting collides with the
   * answer to whatever was asked first, taking the request down with it — see `docs/relay.md`.
   *
   * Retrying is safe for what this tool sends. Every write it issues sets a coil or a register to
   * a stated value, so arriving twice leaves the device where arriving once would have
   */
  async request(
    frame: Uint8Array,
    timeoutMs = 1_000,
    options: { retries?: number } = {},
  ): Promise<Uint8Array> {
    const writer = this.#writer;
    if (!writer) throw new RequestFailed("The port is not open", "closed");

    const address = frame[0]!;
    const functionCode = frame[1]!;
    let remaining = options.retries ?? 1;

    for (;;) {
      const reply = this.#awaitResponse(address, functionCode, frame, timeoutMs);
      await writer.write(frame);

      try {
        return await reply;
      } catch (error) {
        const spoiled = error instanceof RequestFailed && error.reason === "noise";
        if (!spoiled || remaining <= 0) throw error;
        remaining--;
      }
    }
  }

  #awaitResponse(
    address: number,
    functionCode: number,
    sent: Uint8Array,
    timeoutMs: number,
  ): Promise<Uint8Array> {
    return new Promise((resolve, reject) => {
      let pending: Uint8Array = new Uint8Array(0);

      const timer = setTimeout(() => {
        finish();
        reject(noAnswer(address, timeoutMs, withoutEcho(pending, sent)));
      }, timeoutMs);

      const unsubscribe = this.subscribe(({ bytes }) => {
        pending = concat(pending, bytes);

        const found = findResponse(pending, address, functionCode, sent);
        if (!found) return;

        finish();
        resolve(found);
      });

      function finish() {
        clearTimeout(timer);
        unsubscribe();
      }
    });
  }

  async #readLoop(): Promise<void> {
    const reader = this.#reader;
    if (!reader) return;

    try {
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        if (!value || value.length === 0) continue;

        const chunk: Chunk = { bytes: value, at: performance.now() };
        // Copied before iterating, so a subscriber that unsubscribes itself — which is exactly
        // what a settled request does — cannot disturb the walk
        for (const subscriber of [...this.#subscribers]) subscriber(chunk);
      }
    } catch (error) {
      // A cancel during close surfaces here as a rejection and is not worth reporting
      if (!this.#closing) throw error;
    }
  }
}

/**
 * Finds the device's answer in what has arrived so far.
 *
 * Returns `undefined` while the answer could still be incomplete, so the caller keeps waiting
 * rather than deciding early
 */
export function findResponse(
  buffer: Uint8Array,
  address: number,
  functionCode: number,
  sent: Uint8Array,
): Uint8Array | undefined {
  for (let offset = 0; offset + 4 <= buffer.length; offset++) {
    if (buffer[offset] !== address) continue;

    const ahead = buffer.subarray(offset);
    const code = ahead[1];

    // Either the answer to what was asked, or the exception form of it
    if (code !== functionCode && code !== (functionCode | 0x80)) continue;

    const length = responseLength(ahead);
    if (typeof length !== "number" || ahead.length < length) continue;

    const candidate = ahead.subarray(0, length);
    if (!isValid(candidate)) continue;

    // The echo of our own request is a valid frame with the right address and function code, so
    // it has to be recognised and skipped rather than returned as an answer
    if (equal(candidate, sent)) continue;

    return new Uint8Array(candidate);
  }

  return undefined;
}

/**
 * The failure for a request that ran out of time, which is two different faults wearing one name.
 *
 * Saying which one it was is most of the diagnosis. "Nothing answered" sends someone to check the
 * address and the bit rate; "something answered and it was not Modbus" sends them somewhere else
 * entirely, and when the stray bytes are legible it usually names the culprit outright
 */
function noAnswer(address: number, timeoutMs: number, stray: Uint8Array): RequestFailed {
  if (stray.length === 0) {
    return new RequestFailed(`No reply from address ${address} within ${timeoutMs} ms`, "timeout", stray);
  }

  const text = printableText(stray);
  const what = text === undefined ? "" : `: “${text}”`;

  return new RequestFailed(
    `No reply from address ${address} within ${timeoutMs} ms, but ${stray.length} bytes arrived that were not an answer${what}`,
    "noise",
    stray,
  );
}

/**
 * Everything that arrived except the echo of the request.
 *
 * An adapter that puts its own transmission back on the receive line would otherwise make every
 * silent device look like a talking one, and turn each timeout into two
 */
function withoutEcho(buffer: Uint8Array, sent: Uint8Array): Uint8Array {
  for (let offset = 0; offset + sent.length <= buffer.length; offset++) {
    if (!equal(buffer.subarray(offset, offset + sent.length), sent)) continue;

    const kept = new Uint8Array(buffer.length - sent.length);
    kept.set(buffer.subarray(0, offset));
    kept.set(buffer.subarray(offset + sent.length), offset);
    return kept;
  }

  return buffer;
}

/**
 * Stray bytes as text, when they plainly are text.
 *
 * Devices that announce themselves do it in ASCII, so showing the words is worth more than showing
 * the hex. The threshold keeps a misread frame — which is bytes, not prose — from being dressed up
 * as a message
 */
function printableText(bytes: Uint8Array): string | undefined {
  let printable = 0;

  for (const byte of bytes) {
    const isText = byte === 0x09 || byte === 0x0a || byte === 0x0d || (byte >= 0x20 && byte <= 0x7e);
    if (isText) printable++;
  }

  if (printable / bytes.length < 0.8) return undefined;

  return new TextDecoder("ascii")
    .decode(bytes)
    .replace(/[^\x20-\x7e]+/g, " ")
    .trim();
}

function equal(left: Uint8Array, right: Uint8Array): boolean {
  if (left.length !== right.length) return false;

  for (let index = 0; index < left.length; index++) {
    if (left[index] !== right[index]) return false;
  }

  return true;
}

function concat(left: Uint8Array, right: Uint8Array): Uint8Array {
  if (left.length === 0) return new Uint8Array(right);

  const joined = new Uint8Array(left.length + right.length);
  joined.set(left);
  joined.set(right, left.length);
  return joined;
}
