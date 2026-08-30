/**
 * Reading a Modbus RTU bus you are not driving.
 *
 * This is the part `debug-listener` does not attempt — it prints bytes. Turning those bytes back
 * into frames is awkward, because RTU has no start byte, no length field and no terminator. Frames
 * are separated only by 3.5 characters of silence, which at 19 200 baud is about 2 ms, and neither
 * the Web Serial API nor the USB adapter behind it preserves gaps that short: bytes arrive in
 * chunks whose boundaries have more to do with USB polling than with the wire.
 *
 * So timing is a hint here, not the mechanism. What actually recovers the framing is that a frame
 * is *self-checking*: guess where it ends, and the CRC tells you whether the guess was right. The
 * function code narrows the guessing to one or two candidate lengths (see `pdu.ts`), and a bus has
 * exactly one master, so requests and responses alternate. Try the expected one first, fall back
 * to the other, and if neither checks out, drop a byte and try again from there.
 *
 * The result is best-effort by nature. It resynchronises within a frame or two of joining a bus
 * mid-conversation, and it says plainly which bytes it could not account for rather than hiding
 * them.
 */

import { isValid } from "./crc";
import {
  decodeRequest,
  decodeResponse,
  requestLength,
  responseLength,
  type FrameLength,
  type Request,
  type Response,
} from "./pdu";

export type Role = "request" | "response";

/** The longest an RTU frame can be, so any inferred length beyond it is a misread */
const MAX_FRAME = 256;

/**
 * Silence after which the next byte is assumed to start a fresh request. Far longer than the 3.5
 * character times the specification asks for, because the adapter's own buffering routinely
 * stretches the real gap; this is only used to abandon a partial frame that will never complete
 */
const DEFAULT_GAP_MS = 50;

export type Observed = {
  /** When the chunk that completed the frame arrived, not when the frame went out on the wire */
  at: number;
  role: Role;
  address: number;
  functionCode: number;
  /** The whole frame including its CRC, copied out of the working buffer */
  bytes: Uint8Array;
  /** `null` when the function code is outside what `pdu.ts` understands */
  decoded: Request | Response | null;
  /**
   * For a response, the request it answers, when one was seen. A read response carries values but
   * not the addresses they came from — only the request says that — so without this a register
   * dump cannot be labelled
   */
  inReplyTo?: Observed;
};

/** Bytes the monitor could not fit into any frame, kept rather than silently dropped */
export type Noise = {
  at: number;
  bytes: Uint8Array;
};

export type MonitorEvent = ({ type: "frame" } & Observed) | ({ type: "noise" } & Noise);

export class BusMonitor {
  // Annotated rather than inferred: chunks handed in by the Web Serial reader are
  // `Uint8Array<ArrayBufferLike>`, and inferring `Uint8Array<ArrayBuffer>` from the initialiser
  // would make them unassignable here
  #buffer: Uint8Array = new Uint8Array(0);
  #expecting: Role = "request";
  #lastRequest: Observed | undefined;
  #lastArrival: number | undefined;
  #pendingNoise: number[] = [];
  readonly #gapMs: number;

  constructor(options: { gapMs?: number } = {}) {
    this.#gapMs = options.gapMs ?? DEFAULT_GAP_MS;
  }

  /**
   * Feeds one chunk in and takes out whatever became decidable.
   *
   * `at` is the arrival time, defaulting to now. It is passed in so tests can drive the clock and
   * so a replay can use recorded timestamps
   */
  push(chunk: Uint8Array, at: number = performance.now()): MonitorEvent[] {
    const events: MonitorEvent[] = [];

    // A long silence means whatever is still in the buffer never completed. Give up on it before
    // adding to it, or its bogus inferred length will keep swallowing good frames behind it
    const since = this.#lastArrival;
    if (since !== undefined && at - since > this.#gapMs && this.#buffer.length > 0) {
      this.#discardAll();
      this.#expecting = "request";
    }
    this.#lastArrival = at;

    this.#buffer = concat(this.#buffer, chunk);

    while (this.#buffer.length > 0) {
      const frame = this.#takeFrame(at);

      if (frame === "wait") break;

      if (frame === "suspect") {
        // The shape we were expecting was long enough to check and its CRC failed, so the byte at
        // the head is probably not the start of anything — the length we are now waiting on came
        // out of a byte we have no reason to trust. Rather than block on it, look for a frame that
        // does check out further along. Gated this narrowly on purpose: while a frame we *are*
        // expecting is legitimately still arriving, nothing has been disproven and this never runs
        if (!this.#resyncAhead()) break;
        continue;
      }

      if (frame === "resync") {
        // The head byte belongs to no frame we can recognise. Set it aside and try again one byte
        // along — this is how the monitor recovers from joining mid-frame
        this.#pendingNoise.push(this.#buffer[0]!);
        this.#buffer = this.#buffer.subarray(1);
        continue;
      }

      // Any noise accumulated before this frame is reported first, so the order the caller sees
      // matches the order the bytes arrived in
      this.#flushNoise(at, events);
      events.push({ type: "frame", ...frame });
    }

    return events;
  }

  /** Everything still held back, for when the port closes and there will be no more bytes */
  flush(at: number = performance.now()): MonitorEvent[] {
    const events: MonitorEvent[] = [];
    this.#discardAll();
    this.#flushNoise(at, events);
    return events;
  }

  #takeFrame(at: number): Observed | "wait" | "resync" | "suspect" {
    const request = requestLength(this.#buffer);
    const response = responseLength(this.#buffer);

    // One master, so the two alternate. Trying the expected shape first is what keeps an 8-byte
    // request from being mistaken for the equally 8-byte echo of a write
    const candidates: ReadonlyArray<readonly [FrameLength, Role]> =
      this.#expecting === "request"
        ? [
            [request, "request"],
            [response, "response"],
          ]
        : [
            [response, "response"],
            [request, "request"],
          ];

    let mayYetComplete = false;
    let expectedWasDisproven = false;

    for (const [length, role] of candidates) {
      if (length === "incomplete") {
        mayYetComplete = true;
        continue;
      }

      if (length === "unknown" || length > MAX_FRAME) continue;

      if (this.#buffer.length < length) {
        mayYetComplete = true;
        continue;
      }

      const frame = this.#buffer.subarray(0, length);
      if (!isValid(frame)) {
        // Long enough to judge, and it failed. Worth remembering only for the shape we expected:
        // the other shape failing says little, since we did not expect it in the first place
        if (role === this.#expecting) expectedWasDisproven = true;
        continue;
      }

      this.#buffer = this.#buffer.subarray(length);
      return this.#observe(frame, role, at);
    }

    if (!mayYetComplete) return "resync";

    return expectedWasDisproven ? "suspect" : "wait";
  }

  /**
   * Looks for a frame that checks out somewhere after the head, for when the head itself is not
   * trustworthy. Everything skipped over is reported as noise.
   *
   * A CRC-16 agrees by chance about once in 65 536 tries, so scanning is not free of false
   * positives. It only ever runs while the monitor is already lost, though, where the alternative
   * is staying lost until the next silence — and a wrong guess there costs one mislabelled frame,
   * which is what staying lost costs anyway
   */
  #resyncAhead(): boolean {
    for (let offset = 1; offset + 4 <= this.#buffer.length; offset++) {
      const ahead = this.#buffer.subarray(offset);

      for (const length of [requestLength(ahead), responseLength(ahead)]) {
        if (typeof length !== "number" || length > MAX_FRAME || ahead.length < length) continue;
        if (!isValid(ahead.subarray(0, length))) continue;

        for (const byte of this.#buffer.subarray(0, offset)) this.#pendingNoise.push(byte);
        this.#buffer = ahead;
        return true;
      }
    }

    return false;
  }

  #observe(frame: Uint8Array, role: Role, at: number): Observed {
    // Copied, because the working buffer is a view over memory that later chunks will reuse
    const bytes = new Uint8Array(frame);

    const observed: Observed = {
      at,
      role,
      address: bytes[0]!,
      functionCode: bytes[1]!,
      bytes,
      decoded: role === "request" ? decodeRequest(bytes) : decodeResponse(bytes),
    };

    if (role === "request") {
      this.#lastRequest = observed;
      this.#expecting = "response";
    } else {
      // Only pair with a request from the same device; a response that does not match one is
      // reported unpaired rather than mislabelled
      if (this.#lastRequest?.address === observed.address) observed.inReplyTo = this.#lastRequest;
      this.#lastRequest = undefined;
      this.#expecting = "request";
    }

    return observed;
  }

  #discardAll(): void {
    for (const byte of this.#buffer) this.#pendingNoise.push(byte);
    this.#buffer = new Uint8Array(0);
  }

  #flushNoise(at: number, events: MonitorEvent[]): void {
    if (this.#pendingNoise.length === 0) return;

    events.push({ type: "noise", at, bytes: Uint8Array.from(this.#pendingNoise) });
    this.#pendingNoise = [];
  }
}

function concat(left: Uint8Array, right: Uint8Array): Uint8Array {
  if (left.length === 0) return right;
  if (right.length === 0) return left;

  const joined = new Uint8Array(left.length + right.length);
  joined.set(left);
  joined.set(right, left.length);
  return joined;
}
