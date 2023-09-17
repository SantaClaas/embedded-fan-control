import { crc16 } from "./crc";
import { read, write } from "./serial";

export enum FunctionCode {
  ReadHoldingRegister = 0x03,
  ReadInputRegister = 0x04,
  WriteSingleHoldingRegister = 0x06,
  Diagnostics = 0x08,
  WriteMultipleHoldingRegisters = 0x10,
}

/**
 * Validates that the given value is a valid device address between 1 and 247
 */
function isValidAddress(value: any): value is number {
  // Device address (1-247), 0 is broadcast
  return Number.isInteger(value) && value < 248 && value > 0;
}

/**
 * Validates a response to determine that the response is intended for us as it is a shared bus medium
 */
function isReadResponseValid(
  response: { view: DataView; frame: Uint8Array },
  expectedAddress: number,
  expectedFunctionCode: FunctionCode,
  expectedDataLength: number
) {
  const crcStartIndex = response.frame.byteLength - 2;
  const crc = crc16(response.frame.subarray(0, crcStartIndex));
  const expectedCrc = response.view.getUint16(5);

  if (crc !== expectedCrc) return false;

  const slaveAddress = response.view.getUint8(0);
  if (expectedAddress !== slaveAddress) return false;

  const functionCode = response.view.getUint8(1);
  if (functionCode !== expectedFunctionCode) return false;

  const dataLength = response.view.getUint8(2);
  return dataLength === expectedDataLength && 2 + dataLength < crcStartIndex;
}

/**
 * Validates that the response to a write is valid, that is the response mirrors the command
 */
function isWriteResponseValid(frame: Uint8Array, response: Uint8Array) {
  if (frame.length !== response.length) return false;

  for (let index = 0; index < frame.length; index++) {
    if (frame[index] !== response[index]) return false;
  }

  return true;
}

type Register = {
  /**
   * A valid register address is a unsigned 16 bit number
   */
  address: number;
  /**
   * The length in bytes of the data contained in the register
   */
  length: number;
};

async function readRegister(
  port: SerialPort,
  deviceAddress: number,
  register: Register,
  registerType:
    | FunctionCode.ReadInputRegister
    | FunctionCode.ReadHoldingRegister,
  signal: AbortSignal
) {
  if (
    signal.aborted ||
    !isValidAddress(deviceAddress) ||
    !port.writable ||
    !port.readable
  )
    return;

  const length = 8;
  const crcOffset = length - 2;
  const frame = new ArrayBuffer(length);

  const view = new DataView(frame);
  // Address
  view.setUint8(0, deviceAddress);
  // Function
  view.setUint8(1, registerType);
  // Register address
  view.setUint16(2, register.address);
  // Number of registers
  view.setUint16(4, 1);
  // CRC
  const crc = crc16(new Uint8Array(frame.slice(0, crcOffset)));
  view.setUint16(crcOffset, crc);

  try {
    await write(port, frame, signal);
  } catch (error: unknown) {
    console.warn("Non-fatal write error:", error);
    return;
  }

  let value;
  try {
    value = await read(port, signal);
  } catch (error: unknown) {
    console.warn("Non-fatal read error:", error);
    return;
  }

  if (!value) return;

  const response = { view: new DataView(value.buffer), frame: value };
  if (
    !isReadResponseValid(response, deviceAddress, registerType, register.length)
  )
    return;

  // Length is already validated at this point
  // Interpretation of data is left to caller
  return new DataView(response.frame.buffer, 3, register.length);
}

export function readInputRegister(
  port: SerialPort,
  deviceAddress: number,
  register: Register,
  signal: AbortSignal
) {
  return readRegister(
    port,
    deviceAddress,
    register,
    FunctionCode.ReadInputRegister,
    signal
  );
}

export function readHoldingRegister(
  port: SerialPort,
  deviceAddress: number,
  register: Register,
  signal: AbortSignal
) {
  return readRegister(
    port,
    deviceAddress,
    register,
    FunctionCode.ReadHoldingRegister,
    signal
  );
}

/**
 *
 * @param port The serial port used to write to write the holding register on the modbus device
 * @param deviceAddress the 8 bit modbus device address
 * @param register the object describing the holding register
 * @param value the value to write to the holding register. Only 16 bit signed integers are currently supported.
 * @param signal the abort signal to cancel the writing
 * @returns true if the value was written successfully, else false
 */
export async function writeHoldingRegister(
  port: SerialPort,
  deviceAddress: number,
  register: Register,
  value: number,
  signal: AbortSignal
) {
  if (signal.aborted || !isValidAddress(deviceAddress)) return false;

  const length = 8;
  const crcOffset = length - 2;
  const frame = new Uint8Array(length);
  const view = new DataView(frame.buffer);
  // Address
  view.setUint8(0, deviceAddress);
  // Function
  view.setUint8(1, FunctionCode.WriteSingleHoldingRegister);
  // Register address
  view.setUint16(2, register.address);
  // Register value
  view.setInt16(4, value);
  // CRC
  const crc = crc16(new Uint8Array(frame.slice(0, crcOffset)));
  view.setUint16(crcOffset, crc);

  try {
    await write(port, frame, signal);
  } catch (error: unknown) {
    console.warn("Non-fatal write error:", error);
    return false;
  }

  let responseFrame;
  try {
    responseFrame = await read(port, signal);
  } catch (error: unknown) {
    console.warn("Non-fatal read error:", error);
    return false;
  }

  if (!responseFrame) return false;

  return isWriteResponseValid(frame, responseFrame);
}
