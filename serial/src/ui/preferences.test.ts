import { describe, expect, it } from "vitest";
import { defaultSettings, type PortSettings } from "../serial/connection";
import { load, nothingRemembered, portKey, sameSettings, save, type Remembered } from "./preferences";

/**
 * `localStorage` as far as this module uses it. The tests run in node, and the point of taking the
 * storage as an argument is that the parsing can be checked without a browser
 */
function storage(entries: Record<string, string> = {}): Storage & { entries: Record<string, string> } {
  return {
    entries,
    getItem: (key: string) => entries[key] ?? null,
    setItem: (key: string, value: string) => void (entries[key] = value),
    removeItem: (key: string) => void delete entries[key],
    clear: () => void (entries = {}),
    key: (index: number) => Object.keys(entries)[index] ?? null,
    get length() {
      return Object.keys(entries).length;
    },
  };
}

const relaySettings: PortSettings = { baudRate: 9600, parity: "none", dataBits: 8, stopBits: 1 };

describe("port keys", () => {
  const ch340 = { usbVendorId: 0x1a86, usbProductId: 0x7523 };

  it("names a port after the hardware behind it", () => {
    expect(portKey(ch340)).toBe("1a86:7523");
  });

  /**
   * Two of the same adapter cannot be told apart — the Web Serial API gives a port no identity of
   * its own — so the second one gets a key of its own rather than the first one's settings
   */
  it("distinguishes a second adapter of the same kind", () => {
    const first = portKey(ch340);
    expect(portKey(ch340, [first])).toBe("1a86:7523#1");
    expect(portKey(ch340, [first, "1a86:7523#1"])).toBe("1a86:7523#2");
  });

  /** A port on a serial header reports no USB ids at all, and still has to be filed somewhere */
  it("has a name for a port that is not USB", () => {
    expect(portKey({})).toBe("unknown");
    expect(portKey({}, ["unknown"])).toBe("unknown#1");
  });

  /** A gap left by an adapter that was unplugged is filled rather than skipped past */
  it("reuses a key nothing holds", () => {
    expect(portKey(ch340, ["1a86:7523#1"])).toBe("1a86:7523");
  });
});

describe("remembering", () => {
  it("has nothing for a port it has not seen", () => {
    expect(load("1a86:7523", storage())).toEqual(nothingRemembered);
  });

  it("reads back what it wrote", () => {
    const kept = storage();
    const remembered: Remembered = {
      settings: relaySettings,
      mode: "devices",
      device: "relay",
      addresses: { relay: 1, radical: 3 },
      reopen: true,
    };

    save("1a86:7523", remembered, kept);

    expect(load("1a86:7523", kept)).toEqual(remembered);
  });

  it("keeps ports apart", () => {
    const kept = storage();

    save("1a86:7523", { ...nothingRemembered, mode: "devices" }, kept);

    expect(load("0403:6001", kept).mode).toBe("monitor");
  });
});

describe("reading a record this build did not write", () => {
  const restored = (stored: unknown) =>
    load("1a86:7523", storage({ "modbus-serial-tool/v1/port/1a86:7523": JSON.stringify(stored) }));

  it("falls back on anything that is not a record", () => {
    expect(load("1a86:7523", storage({ "modbus-serial-tool/v1/port/1a86:7523": "not json" }))).toEqual(
      nothingRemembered,
    );
    expect(restored(null)).toEqual(nothingRemembered);
    expect(restored([1, 2, 3])).toEqual(nothingRemembered);
  });

  /**
   * A bit rate the UI does not offer cannot be chosen back out of the select, so a port opened
   * with it could never be corrected from the panel
   */
  it("refuses a bit rate that is not on offer", () => {
    expect(restored({ settings: { ...relaySettings, baudRate: 31_250 } }).settings.baudRate).toBe(
      defaultSettings.baudRate,
    );
  });

  it("refuses framing the port cannot be opened with", () => {
    const settings = restored({ settings: { parity: "mark", dataBits: 5, stopBits: 3 } }).settings;
    expect(settings).toEqual(defaultSettings);
  });

  /** Each field stands on its own, so one bad one does not cost the rest */
  it("keeps the fields it understands", () => {
    const settings = restored({ settings: { baudRate: 9600, parity: "sideways" } }).settings;
    expect(settings.baudRate).toBe(9600);
    expect(settings.parity).toBe(defaultSettings.parity);
  });

  /**
   * `NaN` from a field being typed into is written out as `null`, and would come back as a value
   * the address input cannot show
   */
  it("drops an address that is not one", () => {
    expect(restored({ addresses: { radical: null, relay: 1.5, sensor: 2 } }).addresses).toEqual({ sensor: 2 });
  });

  it("takes only the two modes it knows", () => {
    expect(restored({ mode: "devices" }).mode).toBe("devices");
    expect(restored({ mode: "something else" }).mode).toBe("monitor");
  });

  /** Opening a port by itself is a decision, so anything but a stored yes means no */
  it("only opens automatically when that is what was stored", () => {
    expect(restored({ reopen: true }).reopen).toBe(true);
    expect(restored({ reopen: "yes" }).reopen).toBe(false);
    expect(restored({}).reopen).toBe(false);
  });
});

describe("comparing settings", () => {
  it("is the four things a port is opened with", () => {
    expect(sameSettings(defaultSettings, { ...defaultSettings })).toBe(true);
    expect(sameSettings(defaultSettings, relaySettings)).toBe(false);
    expect(sameSettings(defaultSettings, { ...defaultSettings, stopBits: 2 })).toBe(false);
  });
});

describe("storage that refuses", () => {
  const refusing: Storage = {
    getItem: () => {
      throw new DOMException("denied", "SecurityError");
    },
    setItem: () => {
      throw new DOMException("quota", "QuotaExceededError");
    },
    removeItem: () => undefined,
    clear: () => undefined,
    key: () => null,
    length: 0,
  };

  /** A browser set to refuse site data costs the convenience, not the panel */
  it("carries on without remembering anything", () => {
    expect(load("1a86:7523", refusing)).toEqual(nothingRemembered);
    expect(() => save("1a86:7523", nothingRemembered, refusing)).not.toThrow();
  });

  it("does the same when there is no storage at all", () => {
    expect(load("1a86:7523", undefined)).toEqual(nothingRemembered);
    expect(() => save("1a86:7523", nothingRemembered, undefined)).not.toThrow();
  });
});
