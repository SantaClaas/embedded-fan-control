import { describe, expect, it } from "vitest";
import { POWER_UP_GREETING, relay } from "../devices/relay";
import { radical } from "../devices/radical";
import { describeFailure, settingsMismatch } from "./client";
import { RequestFailed, defaultSettings, type PortSettings } from "./connection";
import { ascii, bytes } from "../modbus/frames.fixture";

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

describe("describeFailure", () => {
  const noise = (stray: Uint8Array) =>
    new RequestFailed("No reply from address 255 within 1000 ms, but bytes arrived", "noise", stray);

  /**
   * The failure this exists for: a coil write that reached the module, switched the relay and was
   * never confirmed, because the module reset as the coil pulled in. The bytes said so all along —
   * a device only greets the line when it boots — but reading that off a quoted string was left to
   * whoever was at the keyboard
   */
  it("reads the module's greeting as a restart rather than as noise", () => {
    const described = describeFailure(noise(ascii(POWER_UP_GREETING)));

    expect(described).toContain("No reply from address 255");
    expect(described).toContain("restarted");
    expect(described).toContain("supply");
  });

  /** What actually arrives: the tail of a half-sent answer, then the greeting, boundary and all */
  it("recognises it behind the bytes the module managed before it died", () => {
    const spoiled = new Uint8Array([...bytes("FF 05 00 00 FF 80 0A"), ...ascii(POWER_UP_GREETING)]);

    expect(describeFailure(noise(spoiled))).toContain("restarted");
  });

  it("says nothing extra about a line that simply stayed silent", () => {
    const silent = new RequestFailed("No reply from address 255 within 1000 ms", "timeout");

    expect(describeFailure(silent)).toBe("No reply from address 255 within 1000 ms");
  });

  it("says nothing extra about stray bytes that are not the greeting", () => {
    expect(describeFailure(noise(bytes("00 11 22 33")))).not.toContain("restarted");
  });

  it("passes an ordinary error through unchanged", () => {
    expect(describeFailure(new Error("The device refused the write: Illegal data value"))).toBe(
      "The device refused the write: Illegal data value",
    );
  });
});
