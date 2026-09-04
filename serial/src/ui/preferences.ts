/**
 * What the tool remembers about a port between visits.
 *
 * Everything the panel needs before it can do anything — how the port is configured, whether the
 * bus is being watched or a device talked to, which device and at which address — is a choice the
 * user made last time and would make again. Reloading the page is routine here: the app is served
 * as static files and refreshed to pick up a new build, and a port that is unplugged and plugged
 * back in reappears as a fresh panel. Rebuilding the same six choices each time is the cost this
 * removes.
 *
 * Only choices are remembered, never what was read. A register value is a statement about the
 * device as it was, and showing a restored one beside a live one would make the page lie about
 * what it knows; the read is one click, and the click is the point at which the user takes
 * responsibility for putting frames on the bus.
 *
 * The parsing is deliberately forgiving. Stored JSON outlives the code that wrote it — an older
 * build, a hand-edited entry, a device id that no longer exists — so every field is checked and
 * falls back on its own rather than discarding the whole record.
 */

import { baudRates, defaultSettings, type Parity, type PortSettings } from "../serial/connection";

/** Which of the two things the panel is doing with the port */
export type Mode = "monitor" | "devices";

export type Remembered = {
  settings: PortSettings;
  mode: Mode;
  /** The device last talked to, by id. Absent until one has been chosen */
  device?: string;
  /**
   * The Modbus address last used for each device, by device id. Per device rather than per port,
   * because the address is a property of the thing being addressed: a bus with fans at 0x02 and
   * 0x03 and a relay at 0x01 should offer each of them the address it was last reached at
   */
  addresses: Readonly<Record<string, number>>;
  /**
   * Whether to open this port as soon as it appears, without waiting for a click.
   *
   * Off until asked for. Opening a port asserts the adapter's control lines, and on an adapter
   * that drives the RS-485 transceiver from RTS that is the one thing a passive tool must not do
   * behind the user's back — so it stays a decision, made once and then remembered
   */
  reopen: boolean;
};

/** What a port the tool has never seen starts from */
export const nothingRemembered: Remembered = {
  settings: defaultSettings,
  mode: "monitor",
  addresses: {},
  reopen: false,
};

/**
 * How a port is recognised again after a reload.
 *
 * The Web Serial API hands out a fresh `SerialPort` object every time and gives it no identity of
 * its own, so the USB vendor and product are all there is to go on. Two identical adapters are
 * therefore indistinguishable, and are told apart by the order they were granted in — which is
 * stable while both stay plugged in, and is the best that can be done without an identity the
 * browser does not expose.
 *
 * `taken` is the keys already in use, so the second CH340 gets `1a86:7523#1` rather than the
 * first one's settings
 */
export function portKey(info: Partial<SerialPortInfo>, taken: readonly string[] = []): string {
  const { usbVendorId, usbProductId } = info;

  const base =
    usbVendorId === undefined || usbProductId === undefined
      ? "unknown"
      : `${hex(usbVendorId)}:${hex(usbProductId)}`;

  if (!taken.includes(base)) return base;

  for (let ordinal = 1; ; ordinal++) {
    const candidate = `${base}#${ordinal}`;
    if (!taken.includes(candidate)) return candidate;
  }
}

function hex(value: number): string {
  return value.toString(16).padStart(4, "0");
}

/**
 * Where one port's record lives.
 *
 * Versioned, so a later shape can be introduced without having to make sense of this one: an entry
 * this build cannot read is an entry it does not look at
 */
function entryKey(port: string): string {
  return `modbus-serial-tool/v1/port/${port}`;
}

/**
 * `localStorage`, when there is one to have.
 *
 * Reading the property itself throws in a browser configured to refuse site data, so the guard is
 * around the access rather than around the call
 */
function browserStorage(): Storage | undefined {
  try {
    return globalThis.localStorage ?? undefined;
  } catch {
    return undefined;
  }
}

export function load(port: string, storage = browserStorage()): Remembered {
  let stored: unknown;

  try {
    const raw = storage?.getItem(entryKey(port));
    if (raw === null || raw === undefined) return nothingRemembered;
    stored = JSON.parse(raw);
  } catch {
    // Unreadable or not JSON: the same situation as never having seen this port
    return nothingRemembered;
  }

  if (typeof stored !== "object" || stored === null) return nothingRemembered;
  const entry = stored as Record<string, unknown>;

  return {
    settings: asSettings(entry.settings),
    mode: entry.mode === "devices" ? "devices" : "monitor",
    device: typeof entry.device === "string" ? entry.device : undefined,
    addresses: asAddresses(entry.addresses),
    reopen: entry.reopen === true,
  };
}

/**
 * Writes the record back, and says nothing when it cannot.
 *
 * A full or refused `localStorage` costs the user the convenience and nothing else, so it must not
 * take down the click that happened to be the one that saved
 */
export function save(port: string, remembered: Remembered, storage = browserStorage()): void {
  try {
    storage?.setItem(entryKey(port), JSON.stringify(remembered));
  } catch {
    // Nothing to do and nothing worth saying: the panel works, it just will not be remembered
  }
}

function asSettings(stored: unknown): PortSettings {
  if (typeof stored !== "object" || stored === null) return defaultSettings;
  const entry = stored as Record<string, unknown>;

  return {
    baudRate: baudRates.includes(entry.baudRate as (typeof baudRates)[number])
      ? (entry.baudRate as number)
      : defaultSettings.baudRate,
    parity: isParity(entry.parity) ? entry.parity : defaultSettings.parity,
    dataBits: entry.dataBits === 7 || entry.dataBits === 8 ? entry.dataBits : defaultSettings.dataBits,
    stopBits: entry.stopBits === 1 || entry.stopBits === 2 ? entry.stopBits : defaultSettings.stopBits,
  };
}

function isParity(value: unknown): value is Parity {
  return value === "none" || value === "even" || value === "odd";
}

/**
 * The remembered addresses, keeping only the entries that are addresses.
 *
 * A half-typed field reads back as `NaN` and `JSON.stringify` turns that into `null`, so a record
 * written mid-keystroke would otherwise come back and be handed to the address input as a value it
 * cannot show
 */
function asAddresses(stored: unknown): Record<string, number> {
  if (typeof stored !== "object" || stored === null) return {};

  const addresses: Record<string, number> = {};
  for (const [device, address] of Object.entries(stored as Record<string, unknown>)) {
    if (typeof address === "number" && Number.isInteger(address)) addresses[device] = address;
  }

  return addresses;
}

/** Whether two port settings would open the same port the same way */
export function sameSettings(left: PortSettings, right: PortSettings): boolean {
  return (
    left.baudRate === right.baudRate &&
    left.parity === right.parity &&
    left.dataBits === right.dataBits &&
    left.stopBits === right.stopBits
  );
}
