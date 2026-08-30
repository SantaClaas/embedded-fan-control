/**
 * What a register is, independent of which device it belongs to.
 *
 * A device here is a list of registers, each of which knows its address, what the manual calls it,
 * and how to turn the raw sixteen bits into the quantity they stand for. The UI is generated from
 * that list, so adding a register is adding an entry rather than adding a component.
 *
 * ## On naming
 *
 * `name` is the manual's own heading for the register, in the manual's own language. For the
 * RadiCal that means German — *Aussteuergrad*, *Wirksinn*, *Sollwert* — because those are the
 * words that can be looked up in the document that defines them. `gloss` carries an English
 * explanation alongside, and `reference` says which section to check.
 *
 * This is deliberate. Translating *Aussteuergrad* to something plausible-sounding and then showing
 * only the translation is how a reader ends up unable to find the register in the manual at all.
 */

/** Which of the four Modbus address spaces a register lives in */
export type Space = "input" | "holding" | "coil" | "discreteInput";

/**
 * Values already read from the device that other registers are expressed relative to.
 *
 * The RadiCal reports almost nothing in absolute terms: a speed is a fraction of the configured
 * maximum speed, a voltage is a fraction of a reference voltage. Those references are holding
 * registers that change only when the fan is reconfigured, so they are read once and passed in
 * here rather than re-read on every poll
 */
export type Context = {
  holding: ReadonlyMap<number, number>;
};

export const noContext: Context = { holding: new Map() };

/** What a raw register value turns into once it is understood */
export type Value =
  | { kind: "quantity"; value: number; unit: string; text: string }
  | { kind: "text"; text: string }
  /** A bit field, such as the RadiCal's motor status. `set` lists the flags that are raised */
  | { kind: "flags"; set: readonly string[]; text: string }
  | { kind: "boolean"; value: boolean; text: string }
  /**
   * The register was read, but its value cannot be stated. Distinct from an error: the read
   * succeeded and this *is* the honest answer — a speed with no maximum to scale it against, a
   * PT1000 reading its out-of-range sentinel, a counter the fan never populates
   */
  | { kind: "unavailable"; text: string; because: string };

export type Decode = (raw: number, context: Context) => Value;

/** How a writable register's input is constrained, so the UI can render the right control */
export type Input =
  | { control: "number"; min: number; max: number; step?: number; unit?: string }
  | { control: "choice"; options: ReadonlyMap<number, string> }
  | { control: "toggle" };

export type Register = {
  address: number;
  space: Space;
  /** The manual's own heading, in the manual's own language. Not translated */
  name: string;
  /** An English explanation of what `name` means, for readers who need one */
  gloss?: string;
  /** Where in the manual this is defined, so the reader can check rather than trust */
  reference: string;
  decode: Decode;
  /**
   * Holding register addresses `decode` needs in its `Context`. The UI reads these once before
   * polling anything that depends on them
   */
  dependsOn?: readonly number[];
  /** Present only when the register can be written. Turns a user's number into the raw value */
  write?: {
    input: Input;
    encode: (input: number) => number;
  };
};

export type Device = {
  id: string;
  name: string;
  /** Where this device's registers are documented */
  documentation: string;
  /** What the port has to be set to for the device to answer at all */
  defaults: {
    address: number;
    baudRate: number;
    parity: "none" | "even" | "odd";
    /** The addresses the device will accept, for validating a change to its own address */
    addressRange: readonly [number, number];
  };
  registers: readonly Register[];
};

/* -------------------------------------------------------------------------- */
/* Decoders shared across devices                                             */
/* -------------------------------------------------------------------------- */

/** Sixteen bits read as a signed number, which the register itself does not say */
export function asSigned(raw: number): number {
  return raw >= 0x8000 ? raw - 0x10000 : raw;
}

/** Rounds for display without pretending to a precision the device does not have */
function round(value: number, places: number): number {
  const factor = 10 ** places;
  return Math.round(value * factor) / factor;
}

export function quantity(value: number, unit: string, places = 1): Value {
  const rounded = round(value, places);
  return { kind: "quantity", value: rounded, unit, text: `${rounded} ${unit}` };
}

export function unavailable(because: string): Value {
  return { kind: "unavailable", text: "—", because };
}

/** A register whose value is an index into a table of meanings */
export function choice(table: ReadonlyMap<number, string>): Decode {
  return (raw) => {
    const text = table.get(raw);
    return text === undefined
      ? { kind: "text", text: `Unknown (0x${hex(raw)})` }
      : { kind: "text", text };
  };
}

/**
 * A register whose bits each mean something. Bit numbers are counted from the least significant
 * bit of the sixteen-bit register, so a manual that draws the two bytes separately has to be read
 * with its MSB row occupying bits 15 down to 8
 */
export function flags(table: ReadonlyMap<number, string>): Decode {
  return (raw) => {
    const set: string[] = [];

    for (const [bit, meaning] of table) {
      if (raw & (1 << bit)) set.push(meaning);
    }

    return {
      kind: "flags",
      set,
      text: set.length === 0 ? "None" : set.join(", "),
    };
  };
}

export function hex(value: number, width = 4): string {
  return value.toString(16).toUpperCase().padStart(width, "0");
}

/** Looks a reference value up in the context, for the many registers that are relative to one */
export function reference(context: Context, address: number): number | undefined {
  return context.holding.get(address);
}
