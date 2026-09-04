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
import { carriesPowerUpGreeting } from "../devices/relay";
import { RequestFailed, type Connection, type PortSettings } from "./connection";

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

/**
 * A failure as the panel should show it: what happened, and what it means where the bytes say more
 * than that they were not an answer.
 *
 * `connection.ts` reports the fault it can see — nothing answered, or something arrived that was
 * not the answer, quoted when it is legible. It cannot go further without knowing the device, and
 * this is where the device is known
 */
export function describeFailure(cause: unknown): string {
  const message = cause instanceof Error ? cause.message : String(cause);
  const meaning = restartedMidExchange(cause);

  return meaning === undefined ? message : `${message} — ${meaning}`;
}

/**
 * The relay module's greeting, arriving in place of an answer.
 *
 * The module sends that line when it boots and at no other time, so hearing it in the middle of an
 * exchange is not noise on the line: it is the module having restarted between being asked and
 * answering. A write is when its coil pulls in and its supply is asked for the most it will ever be
 * asked for, which is why this is nearly always a write.
 *
 * No device check is needed to be sure it is the relay — it is the only thing on any of these buses
 * that announces itself in English
 */
function restartedMidExchange(cause: unknown): string | undefined {
  if (!(cause instanceof RequestFailed) || !carriesPowerUpGreeting(cause.stray)) return undefined;

  return (
    "that is the module's power-up greeting, which it only sends when it boots, so it restarted " +
    "mid-exchange rather than answering. Switching the relay is when its coil draws current, so " +
    "suspect the supply before the bus: give the module its own supply rather than a rail shared " +
    "with something else, or bulk capacitance at its power input, and the relay will hold instead " +
    "of flickering."
  );
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
 * A run may name the unit it has to be addressed to — the relay's device address register is read
 * from unit 0 whatever address the module is set to, which is what makes it findable when the
 * address has been forgotten — so `address` is what the runs fall back to rather than what they
 * all use.
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
      const frame = read(run.unit ?? address, run.start, run.quantity);
      const response = await connection.request(frame, options.timeoutMs);
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
        start: run.start,
        quantity: run.quantity,
        message: decoded?.kind === "exception" ? decoded.text : "The device answered with something unexpected",
      });
    } catch (error) {
      refused.push({ start: run.start, quantity: run.quantity, message: describeFailure(error) });
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
