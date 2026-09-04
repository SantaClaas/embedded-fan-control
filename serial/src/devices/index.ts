import { radical } from "./radical";
import { relay } from "./relay";
import { temperatureSensor } from "./temperature-sensor";
import type { Device, Register } from "./register";

export * from "./register";
export { radical, relay, temperatureSensor };

/**
 * Every device this tool knows how to talk to.
 *
 * They do not all agree on how the port should be set up: the RadiCal fans want 19200 8E1, both
 * the relay and the temperature sensor default to 9600 with no parity. One open port speaks one
 * of those settings, so the UI has to make that a choice rather than a detail
 */
export const devices: readonly Device[] = [radical, temperatureSensor, relay];

export function deviceById(id: string): Device | undefined {
  return devices.find((device) => device.id === id);
}

/** The registers of `device` in one address space, in address order */
export function registersIn(device: Device, space: Register["space"]): readonly Register[] {
  return device.registers.filter((register) => register.space === space);
}

/**
 * Every holding register any of `registers` needs read before it can state a value.
 *
 * The RadiCal expresses most of what it reports relative to a configured reference, so a poll of
 * its input registers is really two reads: the references once, then the values as often as you
 * like. This is what tells the caller which references to fetch
 */
export function referencesFor(registers: readonly Register[]): readonly number[] {
  const addresses = new Set<number>();

  for (const register of registers) {
    for (const address of register.dependsOn ?? []) addresses.add(address);
  }

  return [...addresses].sort((left, right) => left - right);
}

/** One read request: a range of addresses, and who to ask for it */
export type Run = {
  start: number;
  quantity: number;
  /** Set only for a register that is read from a unit other than the device's configured address */
  unit?: number;
};

/**
 * Groups consecutive registers into runs that can be fetched in one request.
 *
 * Modbus reads a range, so asking for D010 through D017 costs the same round trip as asking for
 * any one of them — but the RadiCal refuses more than 37 registers or an answer over 80 bytes
 * (section 1.3.1), and there is no point paying for a long stretch of reserved addresses in
 * between. `maxGap` is how many uninteresting registers are worth carrying to avoid a second
 * request.
 *
 * A register carrying a `read` is exempt from all of that: it names the only frame the device
 * answers for it, so it travels alone and neither absorbs a neighbour nor is absorbed by one
 */
export function runsOf(
  registers: readonly Register[],
  { maxRun = 37, maxGap = 4 }: { maxRun?: number; maxGap?: number } = {},
): readonly Run[] {
  const byAddress = new Map<number, Register>();
  for (const register of registers) {
    if (!byAddress.has(register.address)) byAddress.set(register.address, register);
  }

  const sorted = [...byAddress.values()].sort((left, right) => left.address - right.address);

  const runs: Run[] = [];
  // A run built from a register's own `read` is that frame and nothing else, so the next register
  // must start a run rather than extend it
  let isFixed = false;

  for (const register of sorted) {
    const insisted = register.read;

    if (insisted) {
      const run: Run = { start: register.address, quantity: insisted.quantity ?? 1 };
      if (insisted.unit !== undefined) run.unit = insisted.unit;

      runs.push(run);
      isFixed = true;
      continue;
    }

    const current = isFixed ? undefined : runs.at(-1);
    const end = current === undefined ? undefined : current.start + current.quantity;
    const address = register.address;

    if (current === undefined || end === undefined || address - end > maxGap || address - current.start >= maxRun) {
      runs.push({ start: address, quantity: 1 });
      isFixed = false;
      continue;
    }

    current.quantity = address - current.start + 1;
  }

  return runs;
}
