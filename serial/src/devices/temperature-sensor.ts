/**
 * The RS-485 temperature and humidity sensor.
 *
 * Documented only by `docs/temperature-sensor.md`, which is a transcription of what was found
 * rather than a manufacturer PDF — and which had two wrong CRCs in it until the tests in
 * `src/modbus` caught them. Treat it accordingly: this is the least trustworthy of the three
 * devices' documentation, and anything surprising should be checked against the device.
 *
 * It is also the odd one out on the wire. It defaults to 9600 baud with **no** parity, while the
 * RadiCal fans want 19200 with **even** parity, so it cannot share an open port with them.
 */

import { asSigned, choice, quantity, type Device, type Register } from "./register";

/** The doc's register table. Note these are 1-based, unlike almost every other device */
export const InputRegister = {
  Temperature: 0x0001,
  Humidity: 0x0002,
} as const;

export const HoldingRegister = {
  DeviceAddress: 0x0101,
  BaudRate: 0x0102,
  TemperatureCorrection: 0x0103,
  HumidityCorrection: 0x0104,
} as const;

/**
 * Both measurements are signed tenths. The document is explicit for temperature ("temperature
 * value=0xFF33, converted to decimal -205, actual temperature = -20.5℃") and shows the same
 * divisor for humidity (0x222 → 546 → 54.6 %).
 *
 * Worth stating plainly because the RadiCal codes *its* humidity completely differently, as
 * `Datenbytes / 65536 · 100 %` — same quantity, same bus, different scale
 */
function tenths(unit: string) {
  return (raw: number) => quantity(asSigned(raw) / 10, unit);
}

/** The doc: "Baud rate 0:9600 1:14400 2:19200" */
export const baudRates = new Map([
  [0, "9600 Bit/s (default)"],
  [1, "14400 Bit/s"],
  [2, "19200 Bit/s"],
]);

/**
 * A correction is a signed offset in tenths, −10.0 … +10.0, so the UI works in the same units the
 * user reads off the display rather than in raw register counts
 */
const correction = {
  input: { control: "number", min: -10, max: 10, step: 0.1 } as const,
  encode: (value: number) => {
    const tenths_ = Math.round(value * 10);
    // Written back as two's complement, which is how the device reads a negative offset
    return tenths_ < 0 ? tenths_ + 0x10000 : tenths_;
  },
};

export const registers: readonly Register[] = [
  {
    address: InputRegister.Temperature,
    space: "input",
    name: "Temperature value",
    reference: "docs/temperature-sensor.md, register table",
    decode: tenths("°C"),
  },
  {
    address: InputRegister.Humidity,
    space: "input",
    name: "Humidity value",
    gloss: "Relative humidity. Tenths here, unlike the RadiCal's /65536 coding for the same quantity",
    reference: "docs/temperature-sensor.md, register table",
    decode: tenths("%"),
  },
  {
    address: HoldingRegister.DeviceAddress,
    space: "holding",
    name: "Device address",
    gloss: "1 … 247, default 1",
    reference: "docs/temperature-sensor.md, register table",
    decode: (raw) => quantity(raw, "", 0),
    write: { input: { control: "number", min: 1, max: 247, step: 1 }, encode: (value) => value },
  },
  {
    address: HoldingRegister.BaudRate,
    space: "holding",
    name: "Baud rate",
    reference: "docs/temperature-sensor.md, register table",
    decode: choice(baudRates),
    write: { input: { control: "choice", options: baudRates }, encode: (value) => value },
  },
  {
    address: HoldingRegister.TemperatureCorrection,
    space: "holding",
    name: "Temperature correction value",
    gloss: "Offset added to the reading, −10.0 … +10.0 °C",
    reference: "docs/temperature-sensor.md, register table",
    decode: tenths("°C"),
    write: correction,
  },
  {
    address: HoldingRegister.HumidityCorrection,
    space: "holding",
    name: "Humidity correction value",
    gloss: "Offset added to the reading, −10.0 … +10.0 %",
    reference: "docs/temperature-sensor.md, register table",
    decode: tenths("%"),
    write: correction,
  },
];

export const temperatureSensor: Device = {
  id: "temperature-sensor",
  name: "RS-485 temperature / humidity sensor",
  documentation: "docs/temperature-sensor.md",
  defaults: {
    address: 1,
    baudRate: 9600,
    parity: "none",
    addressRange: [1, 247],
  },
  registers,
};
