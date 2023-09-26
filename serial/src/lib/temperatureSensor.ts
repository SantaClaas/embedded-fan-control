import { Device } from "./modbus";

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

export const supportedBaudRates = {
  9600: "9600 Bit/s",
  14400: "14400 Bit/s",
  19200: "19200 Bit/s",
} as const;

type TemperatureSensorBaudRate = keyof typeof supportedBaudRates;

export function isValidBaudRate(
  baudRate: any
): baudRate is TemperatureSensorBaudRate {
  return baudRate in supportedBaudRates;
}

export function isValidCorrectionValue(value: any): value is number {
  // Value can only have 1 decimal place
  return Number.isInteger(value * 10) && value > -10 && value < 10;
}
function toUint16(view: DataView) {
  return view.getUint16(0);
}
function toBaudRate(view: DataView) {
  return view.getUint16(0) as TemperatureSensorBaudRate;
}

function toTemperature(view: DataView) {
  return view.getInt16(0) / 10;
}

function fromTemperature(temperature: number) {
  return temperature * 10;
}

function identity<T>(value: T) {
  return value;
}

export class TemperatureSensor extends Device {
  inputRegisters = {
    temperature: this.createInputRegister({
      label: "Temperature (℃)",
      address: InputRegisterAddress.Temperature,
      length: 2,
      deserialize: toTemperature,
    }),

    humidity: this.createInputRegister({
      label: "Humidity (%)",
      address: InputRegisterAddress.Humidity,
      length: 2,
      deserialize: toTemperature,
    }),
  };

  holdingRegisters = {
    deviceAddress: this.createHoldingRegister({
      label: "Address",
      address: HoldingRegisterAddress.DeviceAddress,
      length: 2,
      deserialize: toUint16,
      serialize: identity,
      isValid: isValidAddress,
      input: {
        min: 1,
        max: 247,
        step: 1,
      },
    }),

    baudRate: this.createHoldingRegister({
      label: "Baud Rate",
      address: HoldingRegisterAddress.BaudRate,
      length: 2,
      deserialize: toBaudRate,
      serialize: identity<TemperatureSensorBaudRate>,
      isValid: isValidBaudRate,
      input: {
        options: supportedBaudRates,
      },
    }),

    temperatureCorrection: this.createHoldingRegister({
      label: "Temperature Correction Value",
      address: HoldingRegisterAddress.TemperatureCorrectionValue,
      length: 2,
      deserialize: toTemperature,
      serialize: fromTemperature,
      isValid: isValidCorrectionValue,
      input: {
        min: -10,
        max: 10,
        step: 0.1,
      },
    }),

    humidityCorrection: this.createHoldingRegister({
      label: "Humidity Correction Value",
      address: HoldingRegisterAddress.HumidityCorrectionValue,
      length: 2,
      deserialize: toTemperature,
      serialize: fromTemperature,
      isValid: isValidCorrectionValue,
      input: {
        min: -10,
        max: 10,
        step: 0.1,
      },
    }),
  };
}
