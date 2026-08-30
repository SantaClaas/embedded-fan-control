import { describe, expect, it } from "vitest";
import { append, crc16, isValid, read } from "./crc";
import { bytes, relay, temperatureSensor } from "./frames.fixture";

describe("crc16", () => {
  /**
   * The canonical Modbus check value: the CRC of the ASCII bytes "123456789". If this is wrong,
   * nothing else here is worth checking
   */
  it("matches the standard check value", () => {
    expect(crc16(new TextEncoder().encode("123456789"))).toBe(0x4b37);
  });

  it("seeds with 0xFFFF, so an empty range is the seed itself", () => {
    expect(crc16(new Uint8Array(0))).toBe(0xffff);
  });
});

describe("isValid", () => {
  /**
   * Every frame the two manuals print, checked against the check bytes their authors published.
   * These agreeing is the real evidence the implementation is right
   */
  it.each([
    ...Object.entries(relay),
    ...Object.entries(temperatureSensor),
  ])("accepts the manufacturer's frame %s", (_name, frame) => {
    expect(isValid(frame)).toBe(true);
  });

  it("rejects a frame with a corrupted payload", () => {
    const corrupted = Uint8Array.from(relay.readAddressResponse);
    corrupted[4] = corrupted[4]! ^ 0x01;

    expect(isValid(corrupted)).toBe(false);
  });

  it("rejects a frame with a corrupted check byte", () => {
    const corrupted = Uint8Array.from(relay.readAddressResponse);
    corrupted[corrupted.length - 1] = corrupted[corrupted.length - 1]! ^ 0x01;

    expect(isValid(corrupted)).toBe(false);
  });

  it("rejects anything too short to be a frame", () => {
    expect(isValid(bytes("FF 03 02"))).toBe(false);
    expect(isValid(new Uint8Array(0))).toBe(false);
  });
});

describe("byte order", () => {
  /**
   * The one part of Modbus that is little-endian. Everything else — register addresses, counts,
   * values — goes high byte first, and only the CRC is reversed
   */
  it("puts the CRC on the wire low byte first", () => {
    // The manual prints the check code for this frame as 0x99E4, sent as 99 then E4
    const frame = relay.closeRelayRequest;

    expect(read(frame, frame.length - 2)).toBe(0xe499);
    expect(frame[frame.length - 2]).toBe(0x99);
    expect(frame[frame.length - 1]).toBe(0xe4);
  });

  it("append fills the last two bytes so the frame validates", () => {
    const frame = Uint8Array.from(relay.readAddressRequest);
    frame[frame.length - 2] = 0;
    frame[frame.length - 1] = 0;

    append(frame);

    expect(frame).toEqual(relay.readAddressRequest);
  });
});
