import { describe, expect, it } from "vitest";
import { bytes, relay, relayPowerUp, temperatureSensor } from "../modbus/frames.fixture";
import { POWER_UP_GREETING } from "../devices/relay";
import { Connection, RequestFailed, findResponse, type PortSettings } from "./connection";

/** The request each fixture response answers, for the echo checks */
const readBaud = relay.readBaudRequest;

describe("findResponse", () => {
  it("finds the answer to the request that was sent", () => {
    const found = findResponse(relay.readBaudResponse, 0xff, 0x03, readBaud);

    expect(found).toEqual(relay.readBaudResponse);
  });

  it("waits while the answer could still be incomplete", () => {
    expect(findResponse(relay.readBaudResponse.subarray(0, 4), 0xff, 0x03, readBaud)).toBeUndefined();
  });

  /**
   * Some RS-485 adapters put what they transmit back on the receive line — `debug-listener` has a
   * flag for it. The echo is a perfectly valid frame with the right address and function code, so
   * nothing but recognising the bytes themselves keeps it from being returned as the answer
   */
  it("steps over the echo of the request", () => {
    const withEcho = concat(readBaud, relay.readBaudResponse);
    const found = findResponse(withEcho, 0xff, 0x03, readBaud);

    expect(found).toEqual(relay.readBaudResponse);
  });

  it("returns nothing while only the echo has come back", () => {
    expect(findResponse(readBaud, 0xff, 0x03, readBaud)).toBeUndefined();
  });

  it("ignores traffic to and from other devices", () => {
    const busy = concat(temperatureSensor.readBothRequest, temperatureSensor.readBothResponse, relay.readBaudResponse);
    const found = findResponse(busy, 0xff, 0x03, readBaud);

    expect(found).toEqual(relay.readBaudResponse);
  });

  it("ignores an answer to a different function code", () => {
    // A read-coils response from the same device is not the answer to a read-holding-registers
    expect(findResponse(relay.readRelayStateResponse, 0xff, 0x03, readBaud)).toBeUndefined();
  });

  /** An exception is the device answering, not the device staying silent */
  it("accepts the exception form of the function code as the answer", () => {
    const exception = withCrc(bytes("FF 83 02 00 00"));
    const found = findResponse(exception, 0xff, 0x03, readBaud);

    expect(found).toEqual(exception);
  });

  it("skips leading rubbish rather than giving up on the whole buffer", () => {
    const found = findResponse(concat(bytes("00 11 22"), relay.readBaudResponse), 0xff, 0x03, readBaud);

    expect(found).toEqual(relay.readBaudResponse);
  });

  it("copies the answer out of the working buffer", () => {
    const buffer = concat(relay.readBaudResponse);
    const found = findResponse(buffer, 0xff, 0x03, readBaud)!;

    buffer.fill(0);

    expect(Array.from(found)).toEqual(Array.from(relay.readBaudResponse));
  });
});

/**
 * The bug this section exists for: a coil write reported "no reply" from a module that had answered
 * perfectly. 0x05 and 0x06 are confirmed by returning the request byte for byte — the fixture holds
 * the manual's own proof, `closeRelayRequest` and `closeRelayResponse` being the same eight bytes —
 * and the rule that steps over an adapter's echo was throwing that answer away every time
 */
describe("findResponse, for a write the device confirms by repeating it", () => {
  const write = relay.closeRelayRequest;

  it("is the manual's own frame in both directions", () => {
    expect(relay.closeRelayResponse).toEqual(relay.closeRelayRequest);
  });

  it("takes the request's bytes as the answer when the adapter does not echo", () => {
    expect(findResponse(relay.closeRelayResponse, 0xff, 0x05, write, false)).toEqual(relay.closeRelayResponse);
  });

  /** Two copies on an echoing adapter: the first is ours coming back, the second is the module */
  it("takes the second copy when the adapter echoes", () => {
    const withEcho = concat(write, relay.closeRelayResponse);

    expect(findResponse(withEcho, 0xff, 0x05, write, true)).toEqual(relay.closeRelayResponse);
  });

  it("does not take the adapter's own copy as the module's answer", () => {
    expect(findResponse(write, 0xff, 0x05, write, true)).toBeUndefined();
  });

  /** A read is unambiguous whatever the adapter does: its answer never looks like the request */
  it("still steps over the echo of a read, echoing adapter or not", () => {
    expect(findResponse(readBaud, 0xff, 0x03, readBaud, false)).toBeUndefined();
    expect(findResponse(concat(readBaud, relay.readBaudResponse), 0xff, 0x03, readBaud, false)).toEqual(
      relay.readBaudResponse,
    );
  });
});

describe("findResponse, with a device that greets the line", () => {
  /**
   * The greeting is ASCII, so none of its bytes can be mistaken for the relay's own address of
   * 0xFF. Reaching the answer behind it needs nothing more than the scan already does
   */
  it("reads past the module's power-up greeting", () => {
    const buffer = concat(relayPowerUp.greeting, relay.readBaudResponse);

    expect(findResponse(buffer, 0xff, 0x03, readBaud)).toEqual(relay.readBaudResponse);
  });

  /**
   * The exchange the greeting spoiled. There is no answer in here to find — the point of the
   * fixture is that no amount of scanning recovers one, which is what makes the retry necessary
   */
  it("finds nothing in the exchange the greeting collided with", () => {
    const found = findResponse(relayPowerUp.spoiledEcho, 0xff, 0x05, relay.closeRelayRequest);

    expect(found).toBeUndefined();
  });
});

describe("request", () => {
  const settings: PortSettings = { baudRate: 9600, parity: "none", dataBits: 8, stopBits: 1 };

  it("asks again when the line carried bytes that were not an answer", async () => {
    // What the relay does when it is powered up: the first exchange is lost to the greeting, and
    // the second is answered normally
    const port = fakePort((written, push) => {
      push(written === 1 ? relayPowerUp.greeting : relay.readBaudResponse);
    });
    const connection = new Connection(port.port);
    await connection.open(settings);

    const answer = await connection.request(relay.readBaudRequest, 50);

    expect(answer).toEqual(relay.readBaudResponse);
    expect(port.written.length).toBe(2);

    await connection.close();
  });

  it("says what the stray bytes said, when they are legible", async () => {
    const port = fakePort((_, push) => push(relayPowerUp.greeting));
    const connection = new Connection(port.port);
    await connection.open(settings);

    const failure = await connection.request(relay.readBaudRequest, 30).catch((error: unknown) => error);

    expect(failure).toBeInstanceOf(RequestFailed);
    expect((failure as RequestFailed).reason).toBe("noise");
    expect((failure as RequestFailed).message).toContain(POWER_UP_GREETING.trim());

    await connection.close();
  });

  /** Silence is a different fault, and asking again only spends a second timeout confirming it */
  it("does not ask again when nothing answered at all", async () => {
    const port = fakePort(() => undefined);
    const connection = new Connection(port.port);
    await connection.open(settings);

    const failure = await connection.request(relay.readBaudRequest, 30).catch((error: unknown) => error);

    expect((failure as RequestFailed).reason).toBe("timeout");
    expect(port.written.length).toBe(1);

    await connection.close();
  });

  /** The write that could not be confirmed: the module answers, and the answer is the request */
  it("confirms a coil write from the bytes the module actually sends back", async () => {
    const port = fakePort((_, push) => push(relay.closeRelayResponse));
    const connection = new Connection(port.port);
    await connection.open(settings);

    expect(await connection.request(relay.closeRelayRequest, 30)).toEqual(relay.closeRelayResponse);
    expect(port.written.length).toBe(1);

    await connection.close();
  });

  /**
   * The adapter's habit is learned from a read, where the request's bytes cannot be an answer, and
   * then applied to the write, where they can be both
   */
  it("learns from a read that the adapter echoes, and counts copies on the write after it", async () => {
    const port = fakePort((_, push, sent) => push(concat(sent, answerTo(sent))));
    const connection = new Connection(port.port);
    await connection.open(settings);

    expect(connection.echoesTransmissions).toBeUndefined();

    await connection.request(relay.readBaudRequest, 30);
    expect(connection.echoesTransmissions).toBe(true);

    // Two copies arrive; taking the first would be reporting the adapter's word for the module's
    expect(await connection.request(relay.closeRelayRequest, 30)).toEqual(relay.closeRelayResponse);

    await connection.close();
  });

  /** And on an adapter that does not echo, one read is enough to know that too */
  it("learns from a read that the adapter is quiet", async () => {
    const port = fakePort((_, push) => push(relay.readBaudResponse));
    const connection = new Connection(port.port);
    await connection.open(settings);

    await connection.request(relay.readBaudRequest, 30);

    expect(connection.echoesTransmissions).toBe(false);

    await connection.close();
  });

  /** Even a silent device says this much: had the adapter echoed, the bytes would be here */
  it("learns it from a request nothing answered", async () => {
    const port = fakePort(() => undefined);
    const connection = new Connection(port.port);
    await connection.open(settings);

    await connection.request(relay.readBaudRequest, 30).catch(() => undefined);

    expect(connection.echoesTransmissions).toBe(false);

    await connection.close();
  });

  /**
   * An adapter that echoes would otherwise make every silent device look like a talking one, and
   * turn each timeout into two
   */
  it("does not count the adapter's echo as something having answered", async () => {
    const port = fakePort((_, push) => push(relay.readBaudRequest));
    const connection = new Connection(port.port);
    await connection.open(settings);

    const failure = await connection.request(relay.readBaudRequest, 30).catch((error: unknown) => error);

    expect((failure as RequestFailed).reason).toBe("timeout");
    expect(port.written.length).toBe(1);

    await connection.close();
  });
});

/**
 * A serial port that is only what `Connection` uses of one: a stream each way, and a hook that
 * answers a write the way a device on the other end would
 */
function fakePort(answer: (written: number, push: (bytes: Uint8Array) => void, sent: Uint8Array) => void) {
  const written: Uint8Array[] = [];
  let controller: ReadableStreamDefaultController<Uint8Array> | undefined;

  const readable = new ReadableStream<Uint8Array>({
    start(source) {
      controller = source;
    },
  });

  const push = (chunk: Uint8Array) => controller?.enqueue(chunk);

  const writable = new WritableStream<Uint8Array>({
    write(chunk) {
      const sent = new Uint8Array(chunk);
      written.push(sent);
      answer(written.length, push, sent);
    },
  });

  const port = {
    readable,
    writable,
    open: () => Promise.resolve(),
    close: () => Promise.resolve(),
  } as unknown as SerialPort;

  return { port, written, push };
}

/** What a device on the other end would answer, for the two requests these tests send */
function answerTo(sent: Uint8Array): Uint8Array {
  return sent[1] === 0x03 ? relay.readBaudResponse : relay.closeRelayResponse;
}

function withCrc(frame: Uint8Array): Uint8Array {
  const table = new Uint16Array(256);
  for (let byte = 0; byte < 256; byte++) {
    let crc = byte;
    for (let bit = 0; bit < 8; bit++) crc = crc & 1 ? (crc >> 1) ^ 0xa001 : crc >> 1;
    table[byte] = crc;
  }

  let crc = 0xffff;
  for (let index = 0; index < frame.length - 2; index++) crc = (crc >> 8) ^ table[(crc ^ frame[index]!) & 0xff]!;

  const out = new Uint8Array(frame);
  out[out.length - 2] = crc & 0xff;
  out[out.length - 1] = crc >> 8;
  return out;
}

function concat(...parts: Uint8Array[]): Uint8Array {
  const joined = new Uint8Array(parts.reduce((total, part) => total + part.length, 0));
  let offset = 0;

  for (const part of parts) {
    joined.set(part, offset);
    offset += part.length;
  }

  return joined;
}
