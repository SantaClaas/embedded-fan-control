import type { BaudRate } from "./BaudRateSelect.svelte";
import { crc16 } from "./crc";
import { read, write } from "./serial";

export type SensorState =
  | "not connected"
  | {
      // Information required for connection
      address: number;
      baudRate: number;
      // Information that needs to be read
      temperatureCelsius?: number;
      humidityPercent?: number;
      temperatureCorrection?: number;
      humidityCorrection?: number;
    };

enum FunctionCode {
  ReadHoldingRegister = 0x03,
  ReadInputRegister = 0x04,
  WriteSingleHoldingRegister = 0x06,
  WriteMultipleHoldingRegisters = 0x10,
}

/**
 * Input register addresses for the temperature sensor
 */
enum InputRegisterAddress {
  Temperature = 0x0001,
  Humidity = 0x0002,
}

/**
 * Holding register addresses for the temperature sensor
 */
enum HoldingRegisterAddress {
  DeviceAddress = 0x0101,
  BaudRate = 0x0102,
  TemperatureCorrectionValue = 0x0103,
  HumidityCorrectionValue = 0x0104,
}

export const supportedBaudRates = [9600, 14400, 19200] as const;

type TemperatureSensorBaudRate = (typeof supportedBaudRates)[number];

export function isValidCorrectionValue(value: any): value is number {
  // Value can only have 1 decimal place
  return Number.isInteger(value * 10) && value > -10 && value < 10;
}

export async function readTemperature(
  port: SerialPort,
  address: number,
  signal: AbortSignal
) {
  if (
    signal.aborted ||
    !isValidAddress(address) ||
    !port.writable ||
    !port.readable
  )
    return;

  const length = 8;
  const crcOffset = length - 2;
  const frame = new ArrayBuffer(length);

  const view = new DataView(frame);
  // Address
  view.setUint8(0, address);
  // Function
  view.setUint8(1, FunctionCode.ReadInputRegister);
  // Register address
  view.setUint16(2, InputRegisterAddress.Temperature);
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

  if (!isReadResponseValid(value, address, FunctionCode.ReadInputRegister, 2))
    return;

  const temperature = new DataView(value.buffer).getUint16(3);
  return temperature / 10;
}

export async function readHumidity(
  port: SerialPort,
  address: number,
  signal: AbortSignal
) {
  if (
    signal.aborted ||
    !isValidAddress(address) ||
    !port.writable ||
    !port.readable
  )
    return;

  const length = 8;
  const crcOffset = length - 2;
  const frame = new ArrayBuffer(length);

  const view = new DataView(frame);
  // Address
  view.setUint8(0, address);
  // Function
  view.setUint8(1, FunctionCode.ReadInputRegister);
  // Register address
  view.setUint16(2, InputRegisterAddress.Humidity);
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

  if (!isReadResponseValid(value, address, FunctionCode.ReadInputRegister, 2))
    return;

  const humidity = new DataView(value.buffer).getUint16(3);
  return humidity / 10;
}

export async function readAddress(
  port: SerialPort,
  address: number,
  signal: AbortSignal
) {
  if (
    signal.aborted ||
    !isValidAddress(address) ||
    !port.writable ||
    !port.readable
  )
    return;

  const length = 8;
  const crcOffset = length - 2;
  const frame = new ArrayBuffer(length);

  const view = new DataView(frame);
  // Address
  view.setUint8(0, address);
  // Function
  view.setUint8(1, FunctionCode.ReadHoldingRegister);
  // Register address
  view.setUint16(2, HoldingRegisterAddress.DeviceAddress);
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

  if (!isReadResponseValid(value, address, FunctionCode.ReadHoldingRegister, 2))
    return;

  const deviceAddress = new DataView(value.buffer).getUint16(3);
  return deviceAddress;
}

export async function readBaudRate(
  port: SerialPort,
  address: number,
  signal: AbortSignal
) {
  if (
    signal.aborted ||
    !isValidAddress(address) ||
    !port.writable ||
    !port.readable
  )
    return;

  const length = 8;
  const crcOffset = length - 2;
  const frame = new ArrayBuffer(length);

  const view = new DataView(frame);
  // Address
  view.setUint8(0, address);
  // Function
  view.setUint8(1, FunctionCode.ReadHoldingRegister);
  // Register address
  view.setUint16(2, HoldingRegisterAddress.BaudRate);
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

  if (!isReadResponseValid(value, address, FunctionCode.ReadHoldingRegister, 2))
    return;

  const baudRateKey = new DataView(value.buffer).getUint16(3);
  return baudRateKey;
}

export async function readTemperatureCorrection(
  port: SerialPort,
  address: number,
  signal: AbortSignal
) {
  if (
    signal.aborted ||
    !isValidAddress(address) ||
    !port.writable ||
    !port.readable
  )
    return;

  const length = 8;
  const crcOffset = length - 2;
  const frame = new ArrayBuffer(length);

  const view = new DataView(frame);
  // Address
  view.setUint8(0, address);
  // Function
  view.setUint8(1, FunctionCode.ReadHoldingRegister);
  // Register address
  view.setUint16(2, HoldingRegisterAddress.TemperatureCorrectionValue);
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

  if (!isReadResponseValid(value, address, FunctionCode.ReadHoldingRegister, 2))
    return;

  const correctionValue = new DataView(value.buffer).getInt16(3);
  return correctionValue / 10;
}

export async function readHumidityCorrection(
  port: SerialPort,
  address: number,
  signal: AbortSignal
) {
  if (
    signal.aborted ||
    !isValidAddress(address) ||
    !port.writable ||
    !port.readable
  )
    return;

  const length = 8;
  const crcOffset = length - 2;
  const frame = new ArrayBuffer(length);

  const view = new DataView(frame);
  // Address
  view.setUint8(0, address);
  // Function
  view.setUint8(1, FunctionCode.ReadHoldingRegister);
  // Register address
  view.setUint16(2, HoldingRegisterAddress.HumidityCorrectionValue);
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

  if (!isReadResponseValid(value, address, FunctionCode.ReadHoldingRegister, 2))
    return;

  const correctionValue = new DataView(value.buffer).getInt16(3);
  return correctionValue / 10;
}

/**
 * Validates a response to determine that the response is intended for us as it is a shared bus medium
 */
function isReadResponseValid(
  responseView: Uint8Array,
  expectedAddress: number,
  expectedFunctionCode: FunctionCode,
  expectedDataLength: number
) {
  const view = new DataView(responseView.buffer);
  const crc = crc16(responseView.subarray(0, responseView.length - 2));
  const expectedCrc = view.getUint16(5);

  if (crc !== expectedCrc) return false;

  const slaveAddress = view.getUint8(0);
  if (expectedAddress !== slaveAddress) return false;

  const functionCode = view.getUint8(1);
  if (functionCode !== expectedFunctionCode) return false;

  const dataLength = view.getUint8(2);
  return dataLength === expectedDataLength;
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

export function isValidAddress(value: any): value is number {
  // Device address (1~247)
  return Number.isInteger(value) && value < 248 && value > 0;
}

export function isValidBaudRate(
  baudRate: any
): baudRate is TemperatureSensorBaudRate {
  return (
    supportedBaudRates.indexOf(baudRate as TemperatureSensorBaudRate) !== -1
  );
}

export async function writeTemperatureCorrection(
  port: SerialPort,
  address: number,
  correction: number,
  signal: AbortSignal
): Promise<boolean> {
  if (
    signal.aborted ||
    !isValidAddress(address) ||
    !isValidCorrectionValue(correction)
  )
    return false;

  const length = 8;
  const crcOffset = length - 2;
  const frame = new Uint8Array(length);
  const view = new DataView(frame.buffer);
  // Address
  view.setUint8(0, address);
  // Function
  view.setUint8(1, FunctionCode.WriteSingleHoldingRegister);
  // Register address
  view.setUint16(2, HoldingRegisterAddress.TemperatureCorrectionValue);
  // Register value
  view.setInt16(4, correction * 10);
  // CRC
  const crc = crc16(new Uint8Array(frame.slice(0, crcOffset)));
  view.setUint16(crcOffset, crc);

  try {
    await write(port, frame, signal);
  } catch (error: unknown) {
    console.warn("Non-fatal write error:", error);
    return false;
  }

  let value;
  try {
    value = await read(port, signal);
  } catch (error: unknown) {
    console.warn("Non-fatal read error:", error);
    return false;
  }

  if (!value) return false;

  if (!isWriteResponseValid(frame, value)) return false;

  return true;
}

export async function writeHumidityCorrection(
  port: SerialPort,
  address: number,
  correction: number,
  signal: AbortSignal
): Promise<boolean> {
  if (
    signal.aborted ||
    !isValidAddress(address) ||
    !isValidCorrectionValue(correction)
  )
    return false;

  const length = 8;
  const crcOffset = length - 2;
  const frame = new Uint8Array(length);
  const view = new DataView(frame.buffer);
  // Address
  view.setUint8(0, address);
  // Function
  view.setUint8(1, FunctionCode.WriteSingleHoldingRegister);
  // Register address
  view.setUint16(2, HoldingRegisterAddress.HumidityCorrectionValue);
  // Register value
  view.setInt16(4, correction * 10);
  // CRC
  const crc = crc16(new Uint8Array(frame.slice(0, crcOffset)));
  view.setUint16(crcOffset, crc);

  try {
    await write(port, frame, signal);
  } catch (error: unknown) {
    console.warn("Non-fatal write error:", error);
    return false;
  }

  let value;
  try {
    value = await read(port, signal);
  } catch (error: unknown) {
    console.warn("Non-fatal read error:", error);
    return false;
  }

  if (!value) return false;

  if (!isWriteResponseValid(frame, value)) return false;

  return true;
}

export async function writeAddress(
  port: SerialPort,
  address: number,
  newAddress: number,
  signal: AbortSignal
): Promise<boolean> {
  if (signal.aborted || !isValidAddress(address) || !isValidAddress(newAddress))
    return false;

  const length = 8;
  const crcOffset = length - 2;
  const frame = new Uint8Array(length);
  const view = new DataView(frame.buffer);
  // Address
  view.setUint8(0, address);
  // Function
  view.setUint8(1, FunctionCode.WriteSingleHoldingRegister);
  // Register address
  view.setUint16(2, HoldingRegisterAddress.DeviceAddress);
  // Register value
  view.setUint16(4, newAddress);
  // CRC
  const crc = crc16(new Uint8Array(frame.slice(0, crcOffset)));
  view.setUint16(crcOffset, crc);

  try {
    await write(port, frame, signal);
  } catch (error: unknown) {
    console.warn("Non-fatal write error:", error);
    return false;
  }

  let value;
  try {
    value = await read(port, signal);
  } catch (error: unknown) {
    console.warn("Non-fatal read error:", error);
    return false;
  }

  if (!value) return false;

  if (!isWriteResponseValid(frame, value)) return false;

  return true;
}

export async function writeBaudRate(
  port: SerialPort,
  address: number,
  baudRate: TemperatureSensorBaudRate,
  signal: AbortSignal
): Promise<boolean> {
  if (signal.aborted || !isValidAddress(address) || !isValidBaudRate(baudRate))
    return false;

  const length = 8;
  const crcOffset = length - 2;
  const frame = new Uint8Array(length);
  const view = new DataView(frame.buffer);
  // Address
  view.setUint8(0, address);
  // Function
  view.setUint8(1, FunctionCode.WriteSingleHoldingRegister);
  // Register address
  view.setUint16(2, HoldingRegisterAddress.BaudRate);
  // Register value
  view.setUint16(4, baudRate);
  // CRC
  const crc = crc16(frame.subarray(0, crcOffset));
  view.setUint16(crcOffset, crc);

  try {
    await write(port, frame, signal);
  } catch (error: unknown) {
    console.warn("Non-fatal write error:", error);
    return false;
  }

  let value;
  try {
    value = await read(port, signal);
  } catch (error: unknown) {
    console.warn("Non-fatal read error:", error);
    return false;
  }

  if (!value) return false;

  if (!isWriteResponseValid(frame, value)) return false;

  return true;
}
