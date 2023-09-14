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

export const supportedBaudRates: BaudRate[] = [9600, 14400, 19200];

export function isValidCorrectionValue(value: number) {
  // Value can only have 1 decimal place
  return Number.isInteger(value * 10) && value > -10 && value < 10;
}

export async function readTemperature(port: SerialPort, address: number) {
  if (!isValidAddress(address) || !port.writable || !port.readable) return;

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
    await write(port, frame);
  } catch (error: unknown) {
    console.warn("Non-fatal write error:", error);
    return;
  }

  let value;
  try {
    value = await read(port);
  } catch (error: unknown) {
    console.warn("Non-fatal read error:", error);
    return;
  }

  if (!value) return;

  if (!isResponseValid(value, address, FunctionCode.ReadInputRegister, 2))
    return;

  const temperature = new DataView(value.buffer).getUint16(3);
  return temperature / 10;
}

export async function readHumidity(port: SerialPort, address: number) {
  if (!isValidAddress(address) || !port.writable || !port.readable) return;

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
    await write(port, frame);
  } catch (error: unknown) {
    console.warn("Non-fatal write error:", error);
    return;
  }

  let value;
  try {
    value = await read(port);
  } catch (error: unknown) {
    console.warn("Non-fatal read error:", error);
    return;
  }

  if (!value) return;

  if (!isResponseValid(value, address, FunctionCode.ReadInputRegister, 2))
    return;

  const humidity = new DataView(value.buffer).getUint16(3);
  return humidity / 10;
}

export async function readAddress(port: SerialPort, address: number) {
  if (!isValidAddress(address) || !port.writable || !port.readable) return;

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
    await write(port, frame);
  } catch (error: unknown) {
    console.warn("Non-fatal write error:", error);
    return;
  }

  let value;
  try {
    value = await read(port);
  } catch (error: unknown) {
    console.warn("Non-fatal read error:", error);
    return;
  }

  if (!value) return;

  if (!isResponseValid(value, address, FunctionCode.ReadHoldingRegister, 2))
    return;

  const deviceAddress = new DataView(value.buffer).getUint16(3);
  return deviceAddress;
}

export async function readBaudRate(port: SerialPort, address: number) {
  if (!isValidAddress(address) || !port.writable || !port.readable) return;

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
    await write(port, frame);
  } catch (error: unknown) {
    console.warn("Non-fatal write error:", error);
    return;
  }

  let value;
  try {
    value = await read(port);
  } catch (error: unknown) {
    console.warn("Non-fatal read error:", error);
    return;
  }

  if (!value) return;

  if (!isResponseValid(value, address, FunctionCode.ReadHoldingRegister, 2))
    return;

  const baudRateKey = new DataView(value.buffer).getUint16(3);
  return baudRateKey;
}

export async function readTemperatureCorrection(
  port: SerialPort,
  address: number
) {
  if (!isValidAddress(address) || !port.writable || !port.readable) return;

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
    await write(port, frame);
  } catch (error: unknown) {
    console.warn("Non-fatal write error:", error);
    return;
  }

  let value;
  try {
    value = await read(port);
  } catch (error: unknown) {
    console.warn("Non-fatal read error:", error);
    return;
  }

  if (!value) return;

  if (!isResponseValid(value, address, FunctionCode.ReadHoldingRegister, 2))
    return;

  const correctionValue = new DataView(value.buffer).getInt16(3);
  return correctionValue / 10;
}

export async function readHumidityCorrection(
  port: SerialPort,
  address: number
) {
  if (!isValidAddress(address) || !port.writable || !port.readable) return;

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
    await write(port, frame);
  } catch (error: unknown) {
    console.warn("Non-fatal write error:", error);
    return;
  }

  let value;
  try {
    value = await read(port);
  } catch (error: unknown) {
    console.warn("Non-fatal read error:", error);
    return;
  }

  if (!value) return;

  if (!isResponseValid(value, address, FunctionCode.ReadHoldingRegister, 2))
    return;

  const correctionValue = new DataView(value.buffer).getInt16(3);
  return correctionValue / 10;
}

/**
 * Validates a response to determine that the response is intended for us as it is a shared bus medium
 */
function isResponseValid(
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

function isValidAddress(value: number) {
  // Device address (1~247)
  return Number.isInteger(value) && value < 248 && value > 0;
}

export async function writeTemperatureCorrection(
  port: SerialPort,
  address: number,
  correction: number
) {
  console.log("Hug 2", isValidCorrectionValue(correction));
  if (!isValidAddress(address) || !isValidCorrectionValue(correction)) return;

  const length = 8;
  const crcOffset = length - 2;
  const frame = new ArrayBuffer(length);

  const view = new DataView(frame);
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

  console.log("Writing", frame);
  try {
    await write(port, frame);
  } catch (error: unknown) {
    console.warn("Non-fatal write error:", error);
    return;
  }

  let value;
  try {
    value = await read(port);
  } catch (error: unknown) {
    console.warn("Non-fatal read error:", error);
    return;
  }

  if (!value) return;

  console.log("Write response", value.buffer);

  if (!isResponseValid(value, address, FunctionCode.ReadHoldingRegister, 2))
    return;

  const correctionValue = new DataView(value.buffer).getInt16(3);
  return correctionValue / 10;
}
