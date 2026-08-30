import type { Request, Response } from "../modbus/pdu";

export function toHex(value: number, width = 2): string {
  return value.toString(16).toUpperCase().padStart(width, "0");
}

/** Frames read best as space-separated hex pairs, the way both manuals print them */
export function frameToHex(frame: Uint8Array): string {
  return Array.from(frame, (byte) => toHex(byte)).join(" ");
}

/**
 * A register address, written the way the device's own documentation writes it.
 *
 * The RadiCal manual uses `D010`, and its address space is documented as D000 … D614 — so that
 * range, and only that range, is rendered in its style. Every other device's manual uses plain
 * hex, and the relay in particular has registers at 0x03E8 that would read as nonsense with a
 * `D` in front of them
 */
export function registerAddress(address: number): string {
  const isRadicalSpace = address >= 0xd000 && address <= 0xd614;

  return isRadicalSpace ? `D${toHex(address, 4).slice(1)}` : `0x${toHex(address, 4)}`;
}

const spaceNames: Record<string, readonly [singular: string, plural: string]> = {
  coils: ["coil", "coils"],
  discreteInputs: ["discrete input", "discrete inputs"],
  holdingRegisters: ["holding register", "holding registers"],
  inputRegisters: ["input register", "input registers"],
};

/** Reading "1 holding registers" is a small thing, but it is the kind of small thing that grates */
function count(of: string, quantity: number): string {
  const names = spaceNames[of];
  if (!names) return of;

  return quantity === 1 ? names[0] : names[1];
}

/** A one-line description of what a frame says, for the monitor's summary column */
export function describe(decoded: Request | Response | null): string {
  if (decoded === null) return "Not decoded";

  switch (decoded.kind) {
    case "read":
      return `Read ${decoded.quantity} ${count(decoded.of, decoded.quantity)} from ${registerAddress(decoded.start)}`;

    case "writeSingleCoil":
      return `Write coil ${registerAddress(decoded.address)} ${decoded.on ? "closed" : "open"}`;

    case "writeSingleRegister":
      return `Write ${registerAddress(decoded.address)} = ${decoded.value}`;

    case "writeMultipleCoils":
      return `Write ${decoded.quantity} ${count("coils", decoded.quantity)} from ${registerAddress(decoded.start)}`;

    case "writeMultipleRegisters":
      return `Write ${decoded.values.length} ${count("holdingRegisters", decoded.values.length)} from ${registerAddress(decoded.start)}`;

    case "registers":
      return `${decoded.values.length} ${count(decoded.of, decoded.values.length)}: ${decoded.values.map((value) => toHex(value, 4)).join(" ")}`;

    case "bits":
      return `${count(decoded.of, 2)}: ${decoded.values.map((value) => (value ? "1" : "0")).join("")}`;

    case "writeAcknowledged":
      return `Acknowledged ${decoded.quantity} from ${registerAddress(decoded.start)}`;

    case "exception":
      return `Exception ${toHex(decoded.code)} — ${decoded.text}`;
  }
}

/** Milliseconds since the page loaded, shown relative to the first frame seen */
export function elapsed(at: number, since: number): string {
  const seconds = (at - since) / 1000;
  return `${seconds.toFixed(3)} s`;
}
