import { describe, expect, it } from "vitest";
import { relay } from "../devices/relay";
import { radical } from "../devices/radical";
import { settingsMismatch } from "./client";
import { defaultSettings, type PortSettings } from "./connection";

/** The preset the relay and the temperature sensor both want */
const slow: PortSettings = { baudRate: 9600, parity: "none", dataBits: 8, stopBits: 1 };

describe("settingsMismatch", () => {
  it("says nothing when the port speaks what the device answers to", () => {
    expect(settingsMismatch(relay, slow)).toBeUndefined();
  });

  /**
   * The failure this exists for: the port opens on the RadiCal preset, the relay cannot hear a word
   * of it, and the only symptom is a timeout that reads like a wrong address
   */
  it("names both sides when the port is on the fans' settings and the device is not", () => {
    const message = settingsMismatch(relay, defaultSettings);

    expect(message).toContain("19200 baud with even parity");
    expect(message).toContain("9600 baud with no parity");
  });

  it("notices a bit rate that agrees while the parity does not", () => {
    expect(settingsMismatch(relay, { ...slow, parity: "even" })).toContain("even parity");
  });

  it("says nothing about a port that is not open", () => {
    expect(settingsMismatch(relay, undefined)).toBeUndefined();
  });

  it("holds for the fans as much as for the relay", () => {
    expect(settingsMismatch(radical, defaultSettings)).toBeUndefined();
    expect(settingsMismatch(radical, slow)).toContain("19200 baud with even parity");
  });
});
