/**
 * The LC-Modbus-1R-D7 single-channel Modbus relay module (Shenzhen Elsay / LC Technology).
 *
 * From `docs/manufacturer/relay/`. This one is documented as a list of example frames rather than
 * a register table, so the addresses below are read out of those frames — every one is reproduced
 * in `src/modbus/frames.fixture.ts` and checked against the CRC implementation, which is the only
 * confirmation available that they were transcribed correctly.
 *
 * It is the only device here that uses coils and discrete inputs rather than registers: one relay
 * output on coil 0, one optocoupler input on discrete input 0. The board is a one-relay variant of
 * an eight-relay design, so the manual's addresses run to 8 while only the first exists here.
 */

import { hex, type Device, type Register } from "./register";

/** Relay 1. The manual lists 0x0000 … 0x0007 for an eight-channel board */
export const RELAY_COIL = 0x0000;

/** Optocoupler input 1, read with function code 0x02 */
export const INPUT_DISCRETE = 0x0000;

/** Read with 0x03, written with 0x10. Range 1 … 255, default 255 */
export const DEVICE_ADDRESS = 0x0000;

/**
 * The manual reads the baud rate from 0x03E8 and writes it to 0x03E9 — two different addresses for
 * the one setting, which is unusual enough to be worth stating rather than assuming a typo. Both
 * are taken straight from its worked examples:
 *
 *   read:  FF 03 03 E8 00 01 11 A4
 *   write: FF 10 03 E9 00 01 02 00 04 CA 0E
 */
export const BAUD_RATE_READ = 0x03e8;
export const BAUD_RATE_WRITE = 0x03e9;

export const baudRates = new Map([
  [0x02, "4800 Bit/s"],
  [0x03, "9600 Bit/s (default)"],
  [0x04, "19200 Bit/s"],
]);

/**
 * Flash mode: write two registers at 0x0003 — the mode, then a delay in tenths of a second. 0x0004
 * closes the relay and re-opens it after the delay; 0x0002 opens it and re-closes it. The maximum
 * delay is 0xFFFF tenths, about 6553 seconds
 */
export const FLASH_COMMAND = 0x0003;
export const FlashMode = { CloseThenOpen: 0x0004, OpenThenClose: 0x0002 } as const;
export const FLASH_TENTHS_MAX = 0xffff;

export const registers: readonly Register[] = [
  {
    address: RELAY_COIL,
    space: "coil",
    name: "Relay 1",
    gloss: "Closed shorts COM to NO and lights the board's indicator; open shorts COM to NC",
    reference: "Relay manual, instructions 1, 2 and 8",
    decode: (raw) => ({ kind: "boolean", value: raw !== 0, text: raw !== 0 ? "Closed" : "Open" }),
    write: { input: { control: "toggle" }, encode: (value) => (value ? 1 : 0) },
  },
  {
    address: INPUT_DISCRETE,
    space: "discreteInput",
    name: "Optocoupler input 1",
    gloss: "DC 3.3–30 V on IN. Reads the signal only — it does not drive the relay",
    reference: "Relay manual, instruction 9",
    decode: (raw) => ({ kind: "boolean", value: raw !== 0, text: raw !== 0 ? "High" : "Low" }),
  },
  {
    address: DEVICE_ADDRESS,
    space: "holding",
    name: "Device address",
    gloss: "1 … 255, default 255 (0xFF). Written with function code 0x10, not 0x06",
    reference: "Relay manual, instructions 5, 6 and 7",
    decode: (raw) => ({ kind: "quantity", value: raw, unit: "", text: `${raw} (0x${hex(raw, 2)})` }),
    write: { input: { control: "number", min: 1, max: 255, step: 1 }, encode: (value) => value },
  },
  {
    address: BAUD_RATE_READ,
    space: "holding",
    name: "Baud rate",
    gloss: "Read from 0x03E8 but written to 0x03E9 — the module uses different addresses for the two",
    reference: "Relay manual, instructions 10 to 13",
    decode: (raw) => {
      const text = baudRates.get(raw);
      return text === undefined ? { kind: "text", text: `Unknown (0x${hex(raw, 2)})` } : { kind: "text", text };
    },
  },
];

export const relay: Device = {
  id: "relay",
  name: "LC-Modbus-1R-D7 relay module",
  documentation: "docs/manufacturer/relay/ — Alssay single-way Modbus relay module",
  defaults: {
    address: 0xff,
    baudRate: 9600,
    parity: "none",
    addressRange: [1, 255],
  },
  registers,
};
