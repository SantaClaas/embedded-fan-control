import { describe, expect, it } from "vitest";
import { bytes, relay, temperatureSensor } from "../modbus/frames.fixture";
import { findResponse } from "./connection";

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
