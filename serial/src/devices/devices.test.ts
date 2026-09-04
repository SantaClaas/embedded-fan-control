import { describe, expect, it } from "vitest";
import {
  deviceById,
  devices,
  noContext,
  radical,
  referencesFor,
  registersIn,
  relay,
  runsOf,
  temperatureSensor,
  type Context,
  type Register,
} from "./index";
import { MAXIMUM_SPEED, SET_POINT_MAX, VOLTAGE_REFERENCE, CURRENT_REFERENCE } from "./radical";
import { ADDRESS_QUERY_UNIT, CHANNEL_COUNT, DEVICE_ADDRESS } from "./relay";
import { readCoils, readDiscreteInputs, readHoldingRegisters } from "../modbus/pdu";
import { relay as relayFrames } from "../modbus/frames.fixture";

function find(register: readonly Register[], address: number, space: Register["space"]): Register {
  const found = register.find((entry) => entry.address === address && entry.space === space);
  if (!found) throw new Error(`no ${space} register at 0x${address.toString(16)}`);
  return found;
}

function withHolding(entries: readonly (readonly [number, number])[]): Context {
  return { holding: new Map(entries) };
}

describe("RadiCal input registers", () => {
  const input = (address: number) => find(radical.registers, address, "input");

  /**
   * Section 3.8, and the same arithmetic `fan_sensor` does in Rust. Half of the range in, half of
   * the configured maximum out
   */
  it("scales Istdrehzahl against Maximale Drehzahl", () => {
    const speed = input(0xd010);
    const context = withHolding([[MAXIMUM_SPEED, 3_000]]);

    expect(speed.decode(SET_POINT_MAX / 2, context)).toMatchObject({ value: 1_500, unit: "rpm" });
    expect(speed.decode(SET_POINT_MAX, context)).toMatchObject({ value: 3_000 });
    expect(speed.decode(0, context)).toMatchObject({ value: 0 });
  });

  /** The manual: above 1.02 × maximum the reading is pinned to 0xFF00 */
  it("carries the capped reading through rather than clamping it again", () => {
    const speed = input(0xd010);

    expect(speed.decode(0xff00, withHolding([[MAXIMUM_SPEED, 3_000]]))).toMatchObject({ value: 3_060 });
  });

  /**
   * The honest answer, and the same choice `fan_sensor` makes when it reports the speed as `null`:
   * without the maximum there is no rate to state
   */
  it("says a speed is unavailable rather than inventing one", () => {
    const decoded = input(0xd010).decode(32_000, noContext);

    expect(decoded.kind).toBe("unavailable");
    expect(decoded).toMatchObject({ because: expect.stringContaining("D119") });
  });

  /** Sections 3.13 and 3.14: plain signed degrees. A fan in an unheated loft reads below zero */
  it("reads both temperatures as signed whole degrees", () => {
    expect(input(0xd016).decode(0x002a, noContext)).toMatchObject({ value: 42, unit: "°C" });
    expect(input(0xd016).decode(0xfffb, noContext)).toMatchObject({ value: -5 });
    expect(input(0xd017).decode(0x0026, noContext)).toMatchObject({ value: 38 });
  });

  /** Section 3.15: `Datenbytes / 65536 · 100 %`, not a fraction of 64000 */
  it("reads Aussteuergrad as a percentage of 65536", () => {
    expect(input(0xd019).decode(32_768, noContext)).toMatchObject({ value: 50, unit: "%" });
    expect(input(0xd019).decode(65_535, noContext)).toMatchObject({ value: 100 });
  });

  /** Section 3.20.2: watts directly, which is why the firmware polls this one and not D021 */
  it("reads Aktuelle Leistung Watt with no reference values at all", () => {
    expect(input(0xd027).decode(25, noContext)).toMatchObject({ value: 25, unit: "W" });
  });

  /** Section 3.20.1 */
  it("reads Aktuelle Leistung relativ against both reference values", () => {
    const context = withHolding([
      [VOLTAGE_REFERENCE, 400],
      [CURRENT_REFERENCE, 10],
    ]);

    expect(input(0xd021).decode(32_768, context)).toMatchObject({ value: 2_000, unit: "W" });
    expect(input(0xd021).decode(32_768, noContext).kind).toBe("unavailable");
  });

  /** Sections 3.11 and 3.12: `Datenbyte / 256 · Bezugswert` */
  it("reads the DC link voltage and current against their references", () => {
    const context = withHolding([
      [VOLTAGE_REFERENCE, 400],
      [CURRENT_REFERENCE, 10],
    ]);

    expect(input(0xd013).decode(128, context)).toMatchObject({ value: 200, unit: "V" });
    expect(input(0xd014).decode(128, context)).toMatchObject({ value: 5, unit: "A" });
  });

  /**
   * Section 3.17.3. The RadiCal's own humidity is a fraction of 65536, while the standalone
   * temperature sensor's is tenths. Same quantity, same bus, different coding — the mistake this
   * pins down is decoding one with the other's rule
   */
  it("reads the attached sensor's temperature as tenths and its humidity as a fraction of 65536", () => {
    expect(input(0xd02e).decode(305, noContext)).toMatchObject({ value: 30.5, unit: "°C" });
    expect(input(0xd02e).decode(0xff33, noContext)).toMatchObject({ value: -20.5 });
    expect(input(0xd02f).decode(32_768, noContext)).toMatchObject({ value: 50, unit: "%" });
  });

  /** Section 3.17.7: 32767 is a sentinel for out of range, not a temperature */
  it("reports a PT1000 outside its range as unavailable", () => {
    expect(input(0xd038).decode(250, noContext)).toMatchObject({ value: 250, unit: "°C" });
    expect(input(0xd038).decode(0xfff0, noContext)).toMatchObject({ value: -16 });
    expect(input(0xd038).decode(32_767, noContext).kind).toBe("unavailable");
  });

  /**
   * Section 3.22, and the reason `fan-controller` stopped reading it: both fans on this bus answer
   * 0xFFFF, so reporting 65535 Wh would be inventing a number
   */
  it("does not report the energy counter's 0xFFFF as a reading", () => {
    expect(input(0xd029).decode(0xffff, noContext).kind).toBe("unavailable");
    expect(input(0xd029).decode(1_234, noContext)).toMatchObject({ value: 1_234 });
  });

  /** Section 3.9. The manual draws two byte rows; as one register the MSB row is bits 15…8 */
  it("decodes the Motorstatus bits at the positions the manual draws them", () => {
    expect(input(0xd011).decode(0x0000, noContext)).toMatchObject({ set: [], text: "None" });

    // BLK, bit 7 of the low byte
    expect(input(0xd011).decode(0x0080, noContext)).toMatchObject({
      set: [expect.stringContaining("Motor blockiert")],
    });

    // UzLow, bit 4 of the high byte
    expect(input(0xd011).decode(0x1000, noContext)).toMatchObject({
      set: [expect.stringContaining("Zwischenkreisunterspannung")],
    });

    // "Fan Bad" is set on every fault, so it travels with the specific one
    const overheated = input(0xd011).decode(0x0030, noContext);
    if (overheated.kind !== "flags") throw new Error(`expected flags, got ${overheated.kind}`);
    expect(overheated.set).toHaveLength(2);
  });

  it("decodes the Warnung bits", () => {
    // TM_high, bit 4 of the low byte
    expect(input(0xd012).decode(0x0010, noContext)).toMatchObject({
      set: [expect.stringContaining("Temperatur Motor hoch")],
    });
  });

  /** Section 3.19 */
  it("names the Wirksinn rather than showing 0 or 1", () => {
    expect(input(0xd01e).decode(0, noContext)).toMatchObject({ text: expect.stringContaining("Positiv") });
    expect(input(0xd01e).decode(1, noContext)).toMatchObject({ text: expect.stringContaining("Negativ") });
  });

  /** Section 3.21, including the value that only appears in emergency running */
  it("names the Sollwert Quelle", () => {
    expect(input(0xd028).decode(1, noContext)).toMatchObject({ text: expect.stringContaining("RS485") });
    expect(input(0xd028).decode(255, noContext)).toMatchObject({ text: expect.stringContaining("Notlauf") });
  });

  /** Section 3.2: this project's fans identify as 0x0010 */
  it("identifies the RadiCal in a scroll housing", () => {
    expect(input(0xd000).decode(0x0010, noContext)).toMatchObject({
      text: "ebm-papst RadiCal im Spiralgehäuse, spec 1.00",
    });
  });

  it("shows an unrecognised coded value as a number instead of guessing", () => {
    expect(input(0xd000).decode(0x00ff, noContext)).toMatchObject({ text: "Unknown (0x00FF)" });
  });
});

describe("RadiCal holding registers", () => {
  const holding = (address: number) => find(radical.registers, address, "holding");

  /**
   * The register `fan-controller` writes. The manual notes the low four bits are ignored, which is
   * why the input steps in sixteens rather than pretending to a precision the fan does not have
   */
  it("bounds Vorgabesollwert at 64000 and steps past the ignored low bits", () => {
    const setPoint = holding(0xd001);

    expect(setPoint.write?.input).toMatchObject({ control: "number", min: 0, max: SET_POINT_MAX, step: 16 });
  });

  it("offers the manual's baud rates and parity configurations by name", () => {
    const baud = holding(0xd149).write?.input;
    const parity = holding(0xd14a).write?.input;

    expect(baud).toMatchObject({ control: "choice" });
    expect(parity).toMatchObject({ control: "choice" });

    // The two fan-controller is built for
    expect(holding(0xd149).decode(0x04, noContext)).toMatchObject({ text: expect.stringContaining("19200") });
    expect(holding(0xd14a).decode(0x00, noContext)).toMatchObject({ text: expect.stringContaining("8E1") });
  });

  it("keeps Maximale Drehzahl in rpm, because it alone is absolute", () => {
    expect(holding(MAXIMUM_SPEED).decode(3_000, noContext)).toMatchObject({ value: 3_000, unit: "rpm" });
  });

  /** Section 2.12, and the addresses fan-controller actually uses */
  it("bounds the fan address to what the manual allows", () => {
    expect(holding(0xd100).write?.input).toMatchObject({ min: 1, max: 247 });
  });
});

describe("temperature sensor", () => {
  const register = (address: number, space: Register["space"]) => find(temperatureSensor.registers, address, space);

  /** The document's own worked examples */
  it("decodes the readings the document works through", () => {
    expect(register(0x0001, "input").decode(0x0131, noContext)).toMatchObject({ value: 30.5, unit: "°C" });
    expect(register(0x0001, "input").decode(0xff33, noContext)).toMatchObject({ value: -20.5 });
    expect(register(0x0002, "input").decode(0x0222, noContext)).toMatchObject({ value: 54.6, unit: "%" });
  });

  it("writes a negative correction back as two's complement", () => {
    const correction = register(0x0103, "holding");

    expect(correction.write?.encode(-2.5)).toBe(0xffe7);
    expect(correction.write?.encode(2.5)).toBe(25);
    // Round trip through the decoder it will be read back with
    expect(correction.decode(correction.write!.encode(-2.5), noContext)).toMatchObject({ value: -2.5 });
  });

  it("bounds a correction to the ±10.0 the document allows", () => {
    expect(register(0x0104, "holding").write?.input).toMatchObject({ min: -10, max: 10, step: 0.1 });
  });

  /** It cannot share an open port with the fans, which is a fact about the bus, not the UI */
  it("defaults to settings the RadiCal fans cannot share", () => {
    expect(temperatureSensor.defaults).toMatchObject({ baudRate: 9600, parity: "none" });
    expect(radical.defaults).toMatchObject({ baudRate: 19_200, parity: "even" });
  });
});

describe("relay", () => {
  it("puts the relay on a coil and the optocoupler on a discrete input", () => {
    expect(registersIn(relay, "coil")).toHaveLength(1);
    expect(registersIn(relay, "discreteInput")).toHaveLength(1);
  });

  it("reads the coil as closed or open rather than as a number", () => {
    const coil = find(relay.registers, 0x0000, "coil");

    expect(coil.decode(1, noContext)).toMatchObject({ kind: "boolean", value: true, text: "Closed" });
    expect(coil.decode(0, noContext)).toMatchObject({ value: false, text: "Open" });
  });

  /** Default address 255, which is what every example frame in the manual uses */
  it("defaults to the address the manual's examples use", () => {
    expect(relay.defaults.address).toBe(0xff);
  });

  /**
   * The failure that put this here: "Read all registers" asked `FF 03 00 00 00 01` for the address,
   * `FF 01 00 00 00 01` for the coil and `FF 02 00 00 00 01` for the input — three frames no manual
   * prints, and three timeouts from a module that was listening the whole time. Planning a read is
   * therefore checked against the manual's own bytes rather than against itself
   */
  describe("plans the reads the manual prints", () => {
    const only = (space: Register["space"]) => {
      const runs = runsOf(registersIn(relay, space));
      expect(runs).toHaveLength(1);
      return runs[0]!;
    };

    it("reads the coil eight wide, because the one-wide read is not a frame the module answers", () => {
      const run = only("coil");

      expect(run).toMatchObject({ start: 0x0000, quantity: CHANNEL_COUNT });
      expect(readCoils(relay.defaults.address, run.start, run.quantity)).toEqual(
        relayFrames.readRelayStateRequest,
      );
    });

    it("reads the optocoupler input eight wide for the same reason", () => {
      const run = only("discreteInput");

      expect(readDiscreteInputs(relay.defaults.address, run.start, run.quantity)).toEqual(
        relayFrames.readInputStateRequest,
      );
    });

    /** Unit 0 whatever the module's address is — that is what makes a forgotten address findable */
    it("asks unit 0 for the device address, not the module's own address", () => {
      const [addressRun, baudRun] = runsOf(registersIn(relay, "holding"));

      expect(addressRun).toMatchObject({ start: DEVICE_ADDRESS, unit: ADDRESS_QUERY_UNIT });
      expect(readHoldingRegisters(addressRun!.unit!, addressRun!.start, addressRun!.quantity)).toEqual(
        relayFrames.readAddressRequest,
      );

      // And the register beside it is untouched by that, still read from the module itself
      expect(baudRun!.unit).toBeUndefined();
      expect(readHoldingRegisters(relay.defaults.address, baudRun!.start, baudRun!.quantity)).toEqual(
        relayFrames.readBaudRequest,
      );
    });
  });

  it("names the baud rate codes from the manual's examples", () => {
    const baud = find(relay.registers, 0x03e8, "holding");

    expect(baud.decode(0x03, noContext)).toMatchObject({ text: expect.stringContaining("9600") });
    expect(baud.decode(0x04, noContext)).toMatchObject({ text: expect.stringContaining("19200") });
  });
});

describe("planning reads", () => {
  it("collects every reference a set of registers needs", () => {
    const references = referencesFor(registersIn(radical, "input"));

    expect(references).toContain(MAXIMUM_SPEED);
    expect(references).toContain(VOLTAGE_REFERENCE);
    expect(references).toContain(CURRENT_REFERENCE);
  });

  it("returns each reference once, in address order", () => {
    const references = referencesFor(registersIn(radical, "input"));

    expect(new Set(references).size).toBe(references.length);
    expect([...references]).toEqual([...references].sort((left, right) => left - right));
  });

  /** A range costs the same round trip as one register, so neighbours travel together */
  it("groups neighbouring registers into one read", () => {
    const runs = runsOf([
      { address: 0xd010 } as Register,
      { address: 0xd011 } as Register,
      { address: 0xd012 } as Register,
    ]);

    expect(runs).toEqual([{ start: 0xd010, quantity: 3 }]);
  });

  it("carries a small gap rather than paying for a second request", () => {
    const runs = runsOf([{ address: 0xd010 } as Register, { address: 0xd013 } as Register], { maxGap: 4 });

    expect(runs).toEqual([{ start: 0xd010, quantity: 4 }]);
  });

  it("splits when the gap is not worth carrying", () => {
    const runs = runsOf([{ address: 0xd010 } as Register, { address: 0xd027 } as Register], { maxGap: 4 });

    expect(runs).toEqual([
      { start: 0xd010, quantity: 1 },
      { start: 0xd027, quantity: 1 },
    ]);
  });

  /**
   * A register that names its own frame is that frame and nothing else. Merging a neighbour into it
   * would produce a request the device has never been asked and, on the relay, never answers
   */
  it("keeps a register that names its own frame out of its neighbours' run", () => {
    const runs = runsOf([
      { address: 0x0000, read: { quantity: 8 } } as Register,
      { address: 0x0001 } as Register,
      { address: 0x0002 } as Register,
    ]);

    expect(runs).toEqual([
      { start: 0x0000, quantity: 8 },
      { start: 0x0001, quantity: 2 },
    ]);
  });

  /** Section 1.3.1: the fan refuses more than 37 registers in one request */
  it("never asks for more registers than the fan will answer", () => {
    const many = Array.from({ length: 60 }, (_, index) => ({ address: 0xd000 + index }) as Register);

    for (const run of runsOf(many)) expect(run.quantity).toBeLessThanOrEqual(37);
  });

  it("plans the RadiCal's input registers into a handful of reads", () => {
    const runs = runsOf(registersIn(radical, "input"));

    expect(runs.length).toBeGreaterThan(0);
    for (const run of runs) expect(run.quantity).toBeLessThanOrEqual(37);
  });
});

describe("the registry", () => {
  it("lists all three devices and finds them by id", () => {
    expect(devices).toHaveLength(3);
    expect(deviceById("radical")).toBe(radical);
    expect(deviceById("relay")).toBe(relay);
    expect(deviceById("nothing")).toBeUndefined();
  });

  it("gives every register a manual reference, so a reader can check it", () => {
    for (const device of devices) {
      for (const register of device.registers) {
        expect(register.reference, `${device.id} 0x${register.address.toString(16)}`).not.toBe("");
      }
    }
  });

  it("has no duplicate register within one device and address space", () => {
    for (const device of devices) {
      const keys = device.registers.map((register) => `${register.space}:${register.address}`);
      expect(new Set(keys).size, device.id).toBe(keys.length);
    }
  });
});
