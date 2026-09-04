/**
 * Reading and writing a device's registers over an open connection.
 *
 * The register definitions say what exists and what it means; this turns that into requests. The
 * one piece of cleverness is batching: Modbus reads a *range*, so asking for D010 through D017
 * costs the same round trip as asking for D010 alone, and `runsOf` has already worked out which
 * neighbours are worth fetching together.
 */

import {
  decodeResponse,
  readCoils,
  readDiscreteInputs,
  readHoldingRegisters,
  readInputRegisters,
  writeMultipleRegisters,
  writeSingleCoil,
  writeSingleRegister,
} from "../modbus/pdu";
import { runsOf, type Device, type Register, type Space } from "../devices";
import type { Connection, PortSettings } from "./connection";

/**
 * Whether the port speaks what the device answers to, said plainly.
 *
 * A device at the wrong bit rate or parity is simply silent, and silence is the least informative
 * failure there is — it looks exactly like a wrong address or a wire that fell out. Both numbers
 * are known here long before a request goes out, so the mismatch is worth saying out loud rather
 * than leaving to be inferred from a timeout.
 *
 * It is a warning and not a bar: a device that has been reconfigured no longer matches the defaults
 * its manual describes, and reading it is exactly how that gets confirmed
 */
export function settingsMismatch(device: Device, settings: PortSettings | undefined): string | undefined {
  if (!settings) return undefined;

  const { baudRate, parity } = device.defaults;
  if (settings.baudRate === baudRate && settings.parity === parity) return undefined;

  return `The port is open at ${settings.baudRate} baud with ${parityName(settings.parity)}, but ${device.name} answers at ${baudRate} baud with ${parityName(parity)}. A device that cannot hear the request stays silent, which reads as "no reply".`;
}

function parityName(parity: PortSettings["parity"]): string {
  return parity === "none" ? "no parity" : `${parity} parity`;
}

const readers: Record<Space, (address: number, start: number, quantity: number) => Uint8Array> = {
  input: readInputRegisters,
  holding: readHoldingRegisters,
  coil: readCoils,
  discreteInput: readDiscreteInputs,
};

/** A read that the device refused, kept apart from a read that produced a value */
export type Refusal = { start: number; quantity: number; message: string };

export type ReadResult = {
  /** Raw sixteen-bit values by address. Coils and discrete inputs come back as 0 or 1 */
  values: Map<number, number>;
  /** Ranges the device would not answer, so the UI can say which ones and why */
  refused: Refusal[];
};

/**
 * Reads every register in `registers`, which must all be in `space`, in as few requests as the
 * device's limits allow.
 *
 * A refusal is recorded and the remaining runs are still attempted: one unreadable register
 * should not cost the reading of every other one, and the RadiCal does refuse individual
 * addresses when a variant does not implement them
 */
export async function readAll(
  connection: Connection,
  address: number,
  space: Space,
  registers: readonly Register[],
  options: { timeoutMs?: number } = {},
): Promise<ReadResult> {
  const values = new Map<number, number>();
  const refused: Refusal[] = [];
  const read = readers[space];

  for (const run of runsOf(registers)) {
    try {
      const response = await connection.request(read(address, run.start, run.quantity), options.timeoutMs);
      const decoded = decodeResponse(response);

      if (decoded?.kind === "registers") {
        decoded.values.forEach((value, index) => values.set(run.start + index, value));
        continue;
      }

      if (decoded?.kind === "bits") {
        decoded.values.slice(0, run.quantity).forEach((value, index) => values.set(run.start + index, value ? 1 : 0));
        continue;
      }

      refused.push({
        ...run,
        message: decoded?.kind === "exception" ? decoded.text : "The device answered with something unexpected",
      });
    } catch (error) {
      refused.push({ ...run, message: error instanceof Error ? error.message : String(error) });
    }
  }

  return { values, refused };
}

/**
 * Writes one register, choosing the function code the register's address space calls for.
 *
 * The relay is why this is not simply "function code 6": its device address is a holding register
 * that the manual only ever writes with 0x10, and its relay is a coil rather than a register at
 * all. `useMultiple` covers the first of those
 */
export async function write(
  connection: Connection,
  address: number,
  register: Register,
  raw: number,
  options: { timeoutMs?: number; useMultiple?: boolean; writeAddress?: number } = {},
): Promise<void> {
  // Some devices read a setting from one address and write it to another — see the relay's baud
  // rate, which is read from 0x03E8 and written to 0x03E9
  const target = options.writeAddress ?? register.address;

  const frame =
    register.space === "coil"
      ? writeSingleCoil(address, target, raw !== 0)
      : options.useMultiple
        ? writeMultipleRegisters(address, target, [raw])
        : writeSingleRegister(address, target, raw);

  const response = await connection.request(frame, options.timeoutMs);
  const decoded = decodeResponse(response);

  if (decoded?.kind === "exception") {
    throw new Error(`The device refused the write: ${decoded.text}`);
  }
}
