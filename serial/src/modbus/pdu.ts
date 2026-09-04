/**
 * Modbus RTU frames: how long one is, and what it says.
 *
 * A frame is `address · function code · payload · CRC-16`. Nothing in it records its own length,
 * which is the whole difficulty of reading a bus you are not driving — see `monitor.ts`. What the
 * frame does carry is a function code, and for every code the length is either fixed or written
 * into one byte near the front. That is what `requestLength` and `responseLength` exploit.
 *
 * Request and response are *different shapes for the same function code*, and neither says which
 * it is. A read-holding-registers request is always 8 bytes; the response to it is 5 plus however
 * many bytes it carries. Telling them apart is the monitor's job, not this module's.
 */

import { crc16, read as readCrc } from "./crc";

export const FunctionCode = {
  ReadCoils: 0x01,
  ReadDiscreteInputs: 0x02,
  ReadHoldingRegisters: 0x03,
  ReadInputRegisters: 0x04,
  WriteSingleCoil: 0x05,
  WriteSingleRegister: 0x06,
  WriteMultipleCoils: 0x0f,
  WriteMultipleRegisters: 0x10,
} as const;

export type FunctionCode = (typeof FunctionCode)[keyof typeof FunctionCode];

/** Set in the function code of a response that is reporting a refusal rather than an answer */
export const EXCEPTION_FLAG = 0x80;

/** The value written to a coil to close it. Anything else, in practice 0x0000, opens it */
export const COIL_ON = 0xff00;
export const COIL_OFF = 0x0000;

/**
 * Modbus' own exception codes, plus the two the RadiCal manual documents beyond the standard
 * meanings (section 1.3.1). The fan answers 0x02 for a register outside D000…D614 and for a read
 * of more than 37 registers, and 0x04 when the electronics cannot read a register at all
 */
export const exceptionText: Readonly<Record<number, string>> = {
  0x01: "Illegal function",
  0x02: "Illegal data address",
  0x03: "Illegal data value",
  0x04: "Device failure",
  0x05: "Acknowledge",
  0x06: "Device busy",
  0x08: "Memory parity error",
  0x0a: "Gateway path unavailable",
  0x0b: "Gateway target failed to respond",
};

/**
 * Whether a successful answer to this function code is the request sent back byte for byte.
 *
 * A single write is confirmed by returning exactly what was asked for, so for 0x05 and 0x06 — and
 * only for those two — a frame identical to the request is as likely to be the device agreeing as
 * it is to be an adapter echoing. Every other code answers in a shape of its own: a read carries a
 * byte count, and a multiple write answers with a header shorter than the request that carried the
 * payload
 */
export function answersWithTheRequest(functionCode: number): boolean {
  return functionCode === FunctionCode.WriteSingleCoil || functionCode === FunctionCode.WriteSingleRegister;
}

/**
 * How long a frame is, once its function code and any count byte have been seen.
 *
 * `"incomplete"` means the bytes that carry the length have not arrived yet and the caller should
 * ask again with more. `"unknown"` means the function code is not one this module can size — the
 * RadiCal's serial-number-addressed commands (0x43, 0x44, 0x46, 0x50) are the ones that turn up in
 * practice — and the caller cannot do better than resynchronise
 */
export type FrameLength = number | "incomplete" | "unknown";

/** A read or a single write, in either direction, is always this long */
const FIXED_LENGTH = 8;

export function requestLength(buffer: Uint8Array): FrameLength {
  if (buffer.length < 2) return "incomplete";

  switch (buffer[1]) {
    case FunctionCode.ReadCoils:
    case FunctionCode.ReadDiscreteInputs:
    case FunctionCode.ReadHoldingRegisters:
    case FunctionCode.ReadInputRegisters:
    case FunctionCode.WriteSingleCoil:
    case FunctionCode.WriteSingleRegister:
      return FIXED_LENGTH;

    // address · code · start(2) · quantity(2) · byte count · payload · CRC(2)
    case FunctionCode.WriteMultipleCoils:
    case FunctionCode.WriteMultipleRegisters: {
      if (buffer.length < 7) return "incomplete";
      return 7 + buffer[6]! + 2;
    }

    default:
      return "unknown";
  }
}

export function responseLength(buffer: Uint8Array): FrameLength {
  if (buffer.length < 2) return "incomplete";

  const code = buffer[1]!;

  // address · code · exception · CRC(2). Checked before the switch because the flag makes the
  // function code an entirely different number
  if (code & EXCEPTION_FLAG) return 5;

  switch (code) {
    // address · code · byte count · payload · CRC(2)
    case FunctionCode.ReadCoils:
    case FunctionCode.ReadDiscreteInputs:
    case FunctionCode.ReadHoldingRegisters:
    case FunctionCode.ReadInputRegisters: {
      if (buffer.length < 3) return "incomplete";
      return 3 + buffer[2]! + 2;
    }

    // A write is answered by echoing the header back
    case FunctionCode.WriteSingleCoil:
    case FunctionCode.WriteSingleRegister:
    case FunctionCode.WriteMultipleCoils:
    case FunctionCode.WriteMultipleRegisters:
      return FIXED_LENGTH;

    default:
      return "unknown";
  }
}

export type Request =
  | { kind: "read"; of: "coils" | "discreteInputs" | "holdingRegisters" | "inputRegisters"; start: number; quantity: number }
  | { kind: "writeSingleCoil"; address: number; on: boolean }
  | { kind: "writeSingleRegister"; address: number; value: number }
  | { kind: "writeMultipleCoils"; start: number; quantity: number; values: boolean[] }
  | { kind: "writeMultipleRegisters"; start: number; values: number[] };

export type Response =
  | { kind: "bits"; of: "coils" | "discreteInputs"; values: boolean[] }
  | { kind: "registers"; of: "holdingRegisters" | "inputRegisters"; values: number[] }
  | { kind: "writeSingleCoil"; address: number; on: boolean }
  | { kind: "writeSingleRegister"; address: number; value: number }
  | { kind: "writeAcknowledged"; start: number; quantity: number }
  | { kind: "exception"; functionCode: number; code: number; text: string };

const readTarget: Readonly<Record<number, "coils" | "discreteInputs" | "holdingRegisters" | "inputRegisters">> = {
  [FunctionCode.ReadCoils]: "coils",
  [FunctionCode.ReadDiscreteInputs]: "discreteInputs",
  [FunctionCode.ReadHoldingRegisters]: "holdingRegisters",
  [FunctionCode.ReadInputRegisters]: "inputRegisters",
};

/**
 * Decodes the payload of a frame already known to be complete and CRC-valid.
 *
 * Returns `null` for a function code it does not understand rather than throwing, because the
 * monitor shows unparseable traffic as raw bytes instead of dropping it
 */
export function decodeRequest(frame: Uint8Array): Request | null {
  const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
  const code = frame[1]!;

  const target = readTarget[code];
  if (target !== undefined) {
    return { kind: "read", of: target, start: view.getUint16(2), quantity: view.getUint16(4) };
  }

  switch (code) {
    case FunctionCode.WriteSingleCoil:
      return { kind: "writeSingleCoil", address: view.getUint16(2), on: view.getUint16(4) === COIL_ON };

    case FunctionCode.WriteSingleRegister:
      return { kind: "writeSingleRegister", address: view.getUint16(2), value: view.getUint16(4) };

    case FunctionCode.WriteMultipleCoils: {
      const quantity = view.getUint16(4);
      return { kind: "writeMultipleCoils", start: view.getUint16(2), quantity, values: unpackBits(frame, 7, quantity) };
    }

    case FunctionCode.WriteMultipleRegisters: {
      const count = frame[6]!;
      const values: number[] = [];
      for (let offset = 0; offset + 1 < count; offset += 2) values.push(view.getUint16(7 + offset));
      return { kind: "writeMultipleRegisters", start: view.getUint16(2), values };
    }

    default:
      return null;
  }
}

export function decodeResponse(frame: Uint8Array): Response | null {
  const view = new DataView(frame.buffer, frame.byteOffset, frame.byteLength);
  const code = frame[1]!;

  if (code & EXCEPTION_FLAG) {
    const exception = frame[2]!;
    return {
      kind: "exception",
      functionCode: code & ~EXCEPTION_FLAG,
      code: exception,
      text: exceptionText[exception] ?? `Unknown exception 0x${exception.toString(16).padStart(2, "0")}`,
    };
  }

  switch (code) {
    case FunctionCode.ReadCoils:
    case FunctionCode.ReadDiscreteInputs: {
      const count = frame[2]!;
      return {
        kind: "bits",
        of: code === FunctionCode.ReadCoils ? "coils" : "discreteInputs",
        // Without the request there is no way to know how many of the trailing bits are padding,
        // so every bit of every byte is reported and the caller takes the ones it asked for
        values: unpackBits(frame, 3, count * 8),
      };
    }

    case FunctionCode.ReadHoldingRegisters:
    case FunctionCode.ReadInputRegisters: {
      const count = frame[2]!;
      const values: number[] = [];
      for (let offset = 0; offset + 1 < count; offset += 2) values.push(view.getUint16(3 + offset));
      return {
        kind: "registers",
        of: code === FunctionCode.ReadHoldingRegisters ? "holdingRegisters" : "inputRegisters",
        values,
      };
    }

    case FunctionCode.WriteSingleCoil:
      return { kind: "writeSingleCoil", address: view.getUint16(2), on: view.getUint16(4) === COIL_ON };

    case FunctionCode.WriteSingleRegister:
      return { kind: "writeSingleRegister", address: view.getUint16(2), value: view.getUint16(4) };

    case FunctionCode.WriteMultipleCoils:
    case FunctionCode.WriteMultipleRegisters:
      return { kind: "writeAcknowledged", start: view.getUint16(2), quantity: view.getUint16(4) };

    default:
      return null;
  }
}

/** Modbus packs bits low-bit-first within each byte, starting at the lowest address */
function unpackBits(frame: Uint8Array, offset: number, quantity: number): boolean[] {
  const values: boolean[] = [];

  for (let index = 0; index < quantity; index++) {
    const byte = frame[offset + (index >> 3)];
    if (byte === undefined) break;
    values.push((byte & (1 << (index & 7))) !== 0);
  }

  return values;
}

/* -------------------------------------------------------------------------- */
/* Building frames                                                            */
/* -------------------------------------------------------------------------- */

/** `address · code · start · quantity · CRC`, the shape of every read and single write */
function fixedFrame(address: number, code: number, first: number, second: number): Uint8Array {
  const frame = new Uint8Array(FIXED_LENGTH);
  const view = new DataView(frame.buffer);

  frame[0] = address;
  frame[1] = code;
  view.setUint16(2, first);
  view.setUint16(4, second);
  writeCrc(frame);

  return frame;
}

export function readCoils(address: number, start: number, quantity: number): Uint8Array {
  return fixedFrame(address, FunctionCode.ReadCoils, start, quantity);
}

export function readDiscreteInputs(address: number, start: number, quantity: number): Uint8Array {
  return fixedFrame(address, FunctionCode.ReadDiscreteInputs, start, quantity);
}

export function readHoldingRegisters(address: number, start: number, quantity: number): Uint8Array {
  return fixedFrame(address, FunctionCode.ReadHoldingRegisters, start, quantity);
}

export function readInputRegisters(address: number, start: number, quantity: number): Uint8Array {
  return fixedFrame(address, FunctionCode.ReadInputRegisters, start, quantity);
}

export function writeSingleCoil(address: number, coil: number, on: boolean): Uint8Array {
  return fixedFrame(address, FunctionCode.WriteSingleCoil, coil, on ? COIL_ON : COIL_OFF);
}

export function writeSingleRegister(address: number, register: number, value: number): Uint8Array {
  return fixedFrame(address, FunctionCode.WriteSingleRegister, register, value);
}

export function writeMultipleRegisters(address: number, start: number, values: readonly number[]): Uint8Array {
  const count = values.length * 2;
  const frame = new Uint8Array(9 + count);
  const view = new DataView(frame.buffer);

  frame[0] = address;
  frame[1] = FunctionCode.WriteMultipleRegisters;
  view.setUint16(2, start);
  view.setUint16(4, values.length);
  frame[6] = count;
  values.forEach((value, index) => view.setUint16(7 + index * 2, value));
  writeCrc(frame);

  return frame;
}

/** Local to keep `crc.append`'s name from colliding with the array method of the same idea */
function writeCrc(frame: Uint8Array): void {
  const offset = frame.length - 2;
  const crc = crc16(frame, 0, offset);
  frame[offset] = crc & 0xff;
  frame[offset + 1] = crc >> 8;
}

/** Re-exported so callers checking a frame do not need to reach into `crc.ts` for the byte order */
export { readCrc };
