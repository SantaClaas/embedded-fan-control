import { crc16 } from "./crc";
import {
  FunctionCode,
  readHoldingRegister,
  readInputRegister,
  writeHoldingRegister,
} from "./modbus";

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

export function isValidAddress(value: any): value is number {
  // Device address (1~247)
  return Number.isInteger(value) && value < 248 && value > 0;
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
  const response = await readInputRegister(
    port,
    address,
    { address: InputRegisterAddress.Temperature, length: 2 },
    signal
  );

  if (!response) return;
  return response.getUint16(0) / 10;
}

export async function readHumidity(
  port: SerialPort,
  address: number,
  signal: AbortSignal
) {
  const response = await readInputRegister(
    port,
    address,
    { address: InputRegisterAddress.Humidity, length: 2 },
    signal
  );

  if (!response) return;
  return response.getUint16(0) / 10;
}

export async function readAddress(
  port: SerialPort,
  address: number,
  signal: AbortSignal
) {
  const response = await readHoldingRegister(
    port,
    address,
    { address: HoldingRegisterAddress.DeviceAddress, length: 2 },
    signal
  );

  if (!response) return;
  return response.getUint16(0);
}

export async function readBaudRate(
  port: SerialPort,
  address: number,
  signal: AbortSignal
) {
  const response = await readHoldingRegister(
    port,
    address,
    { address: HoldingRegisterAddress.BaudRate, length: 2 },
    signal
  );

  if (!response) return;
  return response.getUint16(0);
}

export async function readTemperatureCorrection(
  port: SerialPort,
  address: number,
  signal: AbortSignal
) {
  const response = await readHoldingRegister(
    port,
    address,
    { address: HoldingRegisterAddress.TemperatureCorrectionValue, length: 2 },
    signal
  );

  if (!response) return;
  return response.getInt16(0) / 10;
}

export async function readHumidityCorrection(
  port: SerialPort,
  address: number,
  signal: AbortSignal
) {
  const response = await readHoldingRegister(
    port,
    address,
    { address: HoldingRegisterAddress.HumidityCorrectionValue, length: 2 },
    signal
  );

  if (!response) return;
  return response.getInt16(0) / 10;
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
  if (signal.aborted || !isValidCorrectionValue(correction)) return false;

  return await writeHoldingRegister(
    port,
    address,
    { address: HoldingRegisterAddress.TemperatureCorrectionValue, length: 2 },
    correction * 10,
    signal
  );
}

export async function writeHumidityCorrection(
  port: SerialPort,
  address: number,
  correction: number,
  signal: AbortSignal
): Promise<boolean> {
  if (signal.aborted || !isValidCorrectionValue(correction)) return false;

  return await writeHoldingRegister(
    port,
    address,
    { address: HoldingRegisterAddress.HumidityCorrectionValue, length: 2 },
    correction * 10,
    signal
  );
}

export async function writeAddress(
  port: SerialPort,
  address: number,
  newAddress: number,
  signal: AbortSignal
): Promise<boolean> {
  if (signal.aborted || !isValidAddress(newAddress)) return false;

  return await writeHoldingRegister(
    port,
    address,
    { address: HoldingRegisterAddress.DeviceAddress, length: 2 },
    newAddress,
    signal
  );
}

export async function writeBaudRate(
  port: SerialPort,
  address: number,
  baudRate: TemperatureSensorBaudRate,
  signal: AbortSignal
): Promise<boolean> {
  if (signal.aborted || !isValidBaudRate(baudRate)) return false;

  return await writeHoldingRegister(
    port,
    address,
    { address: HoldingRegisterAddress.BaudRate, length: 2 },
    baudRate,
    signal
  );
}
