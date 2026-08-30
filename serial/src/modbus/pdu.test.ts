import { describe, expect, it } from "vitest";
import { isValid } from "./crc";
import { relay, temperatureSensor } from "./frames.fixture";
import {
  decodeRequest,
  decodeResponse,
  readHoldingRegisters,
  readInputRegisters,
  requestLength,
  responseLength,
  writeMultipleRegisters,
  writeSingleCoil,
  writeSingleRegister,
} from "./pdu";

describe("requestLength", () => {
  it("sizes a read from its function code alone", () => {
    expect(requestLength(relay.readAddressRequest)).toBe(8);
    expect(requestLength(temperatureSensor.readBothRequest)).toBe(8);
  });

  it("sizes a single write from its function code alone", () => {
    expect(requestLength(relay.closeRelayRequest)).toBe(8);
  });

  it("sizes a multiple write from its byte count", () => {
    // 7 header + 1 payload + 2 CRC
    expect(requestLength(relay.closeAllRequest)).toBe(10);
    // 7 header + 2 payload + 2 CRC
    expect(requestLength(relay.setBaudRequest)).toBe(11);
  });

  it("asks for more bytes when the count has not arrived", () => {
    expect(requestLength(relay.closeAllRequest.subarray(0, 1))).toBe("incomplete");
    expect(requestLength(relay.closeAllRequest.subarray(0, 6))).toBe("incomplete");
  });

  it("gives up on a function code it cannot size", () => {
    // 0x43 is the RadiCal's read-addressed-by-serial-number, which this module does not model
    expect(requestLength(Uint8Array.of(0x02, 0x43, 0x00))).toBe("unknown");
  });
});

describe("responseLength", () => {
  it("sizes a read response from its byte count", () => {
    expect(responseLength(relay.readAddressResponse)).toBe(7);
    expect(responseLength(relay.readRelayStateResponse)).toBe(6);
    expect(responseLength(temperatureSensor.readBothResponse)).toBe(9);
  });

  it("sizes a write acknowledgement as the echoed header", () => {
    expect(responseLength(relay.writeCoilsResponse)).toBe(8);
    expect(responseLength(relay.setBaudResponse)).toBe(8);
  });

  it("sizes an exception response", () => {
    expect(responseLength(Uint8Array.of(0x02, 0x83, 0x02))).toBe(5);
  });

  it("asks for more bytes when the count has not arrived", () => {
    expect(responseLength(relay.readAddressResponse.subarray(0, 2))).toBe("incomplete");
  });
});

describe("decodeRequest", () => {
  it("reads the register range out of a read", () => {
    expect(decodeRequest(relay.readBaudRequest)).toEqual({
      kind: "read",
      of: "holdingRegisters",
      start: 0x03e8,
      quantity: 1,
    });
  });

  it("distinguishes input registers from holding registers", () => {
    expect(decodeRequest(temperatureSensor.readBothRequest)).toEqual({
      kind: "read",
      of: "inputRegisters",
      start: 0x0001,
      quantity: 2,
    });
  });

  it("reads a coil write as on or off, not as the raw 0xFF00", () => {
    expect(decodeRequest(relay.closeRelayRequest)).toEqual({ kind: "writeSingleCoil", address: 0, on: true });
    expect(decodeRequest(relay.openRelayRequest)).toEqual({ kind: "writeSingleCoil", address: 0, on: false });
  });

  it("unpacks a multiple coil write low bit first", () => {
    const decoded = decodeRequest(relay.closeAllRequest);

    expect(decoded).toEqual({
      kind: "writeMultipleCoils",
      start: 0,
      quantity: 8,
      values: Array.from({ length: 8 }, () => true),
    });
  });

  it("unpacks a multiple register write", () => {
    // Baud rate 19 200 written to holding register 0x03E9 as the value 4
    expect(decodeRequest(relay.setBaudRequest)).toEqual({
      kind: "writeMultipleRegisters",
      start: 0x03e9,
      values: [4],
    });
  });
});

describe("decodeResponse", () => {
  it("reads register values", () => {
    expect(decodeResponse(relay.readAddressResponse)).toEqual({
      kind: "registers",
      of: "holdingRegisters",
      values: [0x00ff],
    });
  });

  it("reads the temperature and humidity out of one response", () => {
    expect(decodeResponse(temperatureSensor.readBothResponse)).toEqual({
      kind: "registers",
      of: "inputRegisters",
      values: [0x0131, 0x0222],
    });
  });

  it("reads coil bits", () => {
    const decoded = decodeResponse(relay.readRelayStateResponse);

    // The manual: bit 0 is relay 1, and 1 means closed
    expect(decoded).toEqual({
      kind: "bits",
      of: "coils",
      values: [true, false, false, false, false, false, false, false],
    });
  });

  it("reads an exception and names it", () => {
    // The RadiCal answers 0x02 for a register outside D000…D614
    expect(decodeResponse(Uint8Array.of(0x02, 0x83, 0x02, 0, 0))).toEqual({
      kind: "exception",
      functionCode: 0x03,
      code: 0x02,
      text: "Illegal data address",
    });
  });

  it("names an exception code it does not know rather than dropping it", () => {
    const decoded = decodeResponse(Uint8Array.of(0x02, 0x84, 0x7f, 0, 0));

    expect(decoded).toMatchObject({ kind: "exception", code: 0x7f, text: "Unknown exception 0x7f" });
  });
});

describe("building frames", () => {
  /**
   * The strongest check available: build the frame from scratch and compare it, CRC and all, to
   * the bytes the manufacturer printed
   */
  it("builds the relay's own read-address request", () => {
    expect(readHoldingRegisters(0x00, 0x0000, 1)).toEqual(relay.readAddressRequest);
  });

  it("builds the relay's own read-baud request", () => {
    expect(readHoldingRegisters(0xff, 0x03e8, 1)).toEqual(relay.readBaudRequest);
  });

  it("builds the relay's own coil writes", () => {
    expect(writeSingleCoil(0xff, 0x0000, true)).toEqual(relay.closeRelayRequest);
    expect(writeSingleCoil(0xff, 0x0000, false)).toEqual(relay.openRelayRequest);
  });

  it("builds the relay's own set-baud request", () => {
    expect(writeMultipleRegisters(0xff, 0x03e9, [4])).toEqual(relay.setBaudRequest);
  });

  it("builds the temperature sensor's own read requests", () => {
    expect(readInputRegisters(0x01, 0x0001, 1)).toEqual(temperatureSensor.readTemperatureRequest);
    expect(readInputRegisters(0x01, 0x0002, 1)).toEqual(temperatureSensor.readHumidityRequest);
    expect(readInputRegisters(0x01, 0x0001, 2)).toEqual(temperatureSensor.readBothRequest);
  });

  it("builds frames that check out", () => {
    expect(isValid(writeSingleRegister(0x02, 0xd001, 32_000))).toBe(true);
    expect(isValid(writeMultipleRegisters(0x02, 0xd001, [1, 2, 3]))).toBe(true);
  });

  /** The set point the firmware writes, so the shape is worth pinning down */
  it("builds a RadiCal set point write", () => {
    const frame = writeSingleRegister(0x02, 0xd001, 32_000);

    expect(Array.from(frame.subarray(0, 6))).toEqual([0x02, 0x06, 0xd0, 0x01, 0x7d, 0x00]);
  });
});
