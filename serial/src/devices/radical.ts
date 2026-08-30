/**
 * The ebm-papst RadiCal in a scroll housing.
 *
 * Every address, coding and name here comes from *MODBUS Parameter RadiCal im Spiralgehäuse
 * V1.00*, in `docs/manufacturer/radical/`. Register names are the manual's German headings,
 * unchanged, with an English gloss beside them — see the note in `register.ts` for why.
 *
 * Almost nothing this fan reports is absolute. A speed is a fraction of the maximum speed it has
 * been configured for (`D119`), a voltage is a fraction of a reference voltage (`D1A0`), a
 * relative power needs both. Those references are holding registers that change only when the fan
 * is reconfigured, so they are declared in `dependsOn` and read once.
 */

import {
  asSigned,
  choice,
  flags,
  hex,
  quantity,
  reference,
  unavailable,
  type Device,
  type Register,
} from "./register";

/* -------------------------------------------------------------------------- */
/* Reference values other registers are expressed against                     */
/* -------------------------------------------------------------------------- */

/** Section 2.25 »Maximale Drehzahl«. Every speed the fan reports or accepts is relative to this */
export const MAXIMUM_SPEED = 0xd119;
/** Section 2.45 »Bezugswert Zwischenkreisspannung« */
export const VOLTAGE_REFERENCE = 0xd1a0;
/** Section 2.46 »Bezugswert Zwischenkreisstrom« */
export const CURRENT_REFERENCE = 0xd1a1;
/** Section 2.48 »Bezugswert Volumenstrom« */
export const VOLUME_FLOW_REFERENCE = 0xd1ed;
/** Section 2.49 »Bezugswert Massestrom« */
export const MASS_FLOW_REFERENCE = 0xd1ee;

/**
 * The full-scale value for every speed and set point. Section 2.3: "Der Maximalwert des
 * Vorgabesollwerts ist bei Drehzahlregelung 64000"
 */
export const SET_POINT_MAX = 64_000;

/* -------------------------------------------------------------------------- */
/* Codings                                                                    */
/* -------------------------------------------------------------------------- */

/** Section 3.8. `Istdrehzahl [rpm] = Datenbytes / 64000 · nMax` */
function speed(raw: number, context: { holding: ReadonlyMap<number, number> }) {
  const maximum = reference(context, MAXIMUM_SPEED);

  if (maximum === undefined) {
    return unavailable(
      "Every speed is a fraction of Maximale Drehzahl (D119), which has not been read yet",
    );
  }

  return quantity((raw * maximum) / SET_POINT_MAX, "rpm", 0);
}

/** Section 3.15. `a [%] = Datenbytes / 65536 · 100 %` */
function modulation(raw: number) {
  return quantity((raw / 65_536) * 100, "%");
}

/** Sections 3.13 and 3.14: plain signed degrees, no divisor */
function celsius(raw: number) {
  return quantity(asSigned(raw), "°C", 0);
}

/** Section 3.17.3: the temperature/humidity sensor's temperature is signed tenths */
function tenthsCelsius(raw: number) {
  return quantity(asSigned(raw) / 10, "°C");
}

/** Section 3.17.3: relative humidity is `Datenbytes / 65536 · 100 %`, not tenths */
function relativeHumidity(raw: number) {
  return quantity((raw / 65_536) * 100, "%");
}

/**
 * Section 3.17.7. The sensors read −40 °C to +250 °C and answer 32767 for anything outside that,
 * which is a sentinel rather than a temperature and is reported as such
 */
function pt1000(raw: number) {
  const value = asSigned(raw);

  if (value === 32_767) return unavailable("Outside the sensor's −40 °C … +250 °C range");

  return quantity(value, "°C", 0);
}

/** Section 3.11. `Uzk [V] = Datenbyte / 256 · Uzk,Bezug` */
function linkVoltage(raw: number, context: { holding: ReadonlyMap<number, number> }) {
  const scale = reference(context, VOLTAGE_REFERENCE);

  if (scale === undefined) {
    return unavailable("Relative to Bezugswert Zwischenkreisspannung (D1A0), which has not been read yet");
  }

  return quantity((raw / 256) * scale, "V");
}

/** Section 3.12. `Izk [A] = Datenbyte / 256 · Izk,Bezug` */
function linkCurrent(raw: number, context: { holding: ReadonlyMap<number, number> }) {
  const scale = reference(context, CURRENT_REFERENCE);

  if (scale === undefined) {
    return unavailable("Relative to Bezugswert Zwischenkreisstrom (D1A1), which has not been read yet");
  }

  return quantity((raw / 256) * scale, "A", 2);
}

/** Section 3.20.1. `P [W] = Datenbytes / 65536 · Uzk,Bezug · Izk,Bezug` */
function relativePower(raw: number, context: { holding: ReadonlyMap<number, number> }) {
  const voltage = reference(context, VOLTAGE_REFERENCE);
  const current = reference(context, CURRENT_REFERENCE);

  if (voltage === undefined || current === undefined) {
    return unavailable(
      "Needs both Bezugswert Zwischenkreisspannung (D1A0) and Bezugswert Zwischenkreisstrom (D1A1)",
    );
  }

  return quantity((raw / 65_536) * voltage * current, "W");
}

/** Sections 3.17.5 and 3.17.6: the relative codings are `Datenbytes / 64000 · Bezugswert` */
function relativeTo(address: number, unit: string, label: string) {
  return (raw: number, context: { holding: ReadonlyMap<number, number> }) => {
    const scale = reference(context, address);

    if (scale === undefined) return unavailable(`Relative to ${label} (D${hex(address).slice(1)}), not read yet`);

    return quantity((raw / SET_POINT_MAX) * scale, unit);
  };
}

function unsigned(unit: string, places = 0) {
  return (raw: number) => quantity(raw, unit, places);
}

/**
 * Section 3.22. The energy counter is documented, but both fans on this bus answer 0xFFFF for it.
 * `fan-controller` stopped reading it for exactly that reason (see CLAUDE.md), and reporting it as
 * a number here would be inventing data
 */
function energyCounter(raw: number) {
  if (raw === 0xffff) {
    return unavailable("The fans on this bus answer 0xFFFF, so the counter is not populated");
  }

  return quantity(raw, "Wh", 0);
}

/**
 * Section 3.9 »Motorstatus«. The manual draws the two bytes as separate rows, MSB above LSB; read
 * as one sixteen-bit register the MSB row occupies bits 15…8
 */
const motorStatusBits = new Map([
  [12, "UzLow — Zwischenkreisunterspannung (DC link undervoltage)"],
  [7, "BLK — Motor blockiert (motor blocked)"],
  [5, "TFM — Motor überhitzt (motor overheated)"],
  [4, "FB — Fan Bad (set on every fault)"],
  [3, "SKF — Kommunikationsfehler Master/Slave Controller"],
]);

/** Section 3.10 »Warnung«: the same layout, one step before the corresponding fault */
const warningBits = new Map([
  [12, "UzHigh — Zwischenkreisspannung hoch"],
  [10, "Kabelbruch am Analogeingang für den Sollwert"],
  [9, "n_Low — Istdrehzahl unter Grenzdrehzahl Laufüberwachung"],
  [6, "UzLow — Zwischenkreisspannung niedrig"],
  [5, "TEI_high — Temperatur Elektronikinnenraum hoch"],
  [4, "TM_high — Temperatur Motor hoch"],
]);

/** Section 3.19 */
const effectiveDirection = new Map([
  [0, "Positiv — Regeldifferenz = Sollwert − Istwert (heating, with a temperature sensor)"],
  [1, "Negativ — Regeldifferenz = Istwert − Sollwert (cooling)"],
]);

/** Section 3.21 */
const setPointSource = new Map([
  [0, "Analogeingang Ain1"],
  [1, "RS485 / Vorgabesollwert (D001)"],
  [255, "Sollwert Notlauf"],
]);

/** Section 2.34.1. The manual: "Das MSB ist ohne Bedeutung!" */
export const baudRates = new Map([
  [0x00, "1200 Bit/s"],
  [0x01, "2400 Bit/s"],
  [0x02, "4800 Bit/s"],
  [0x03, "9600 Bit/s"],
  [0x04, "19200 Bit/s (recommended)"],
  [0x05, "38400 Bit/s"],
  [0x06, "57600 Bit/s"],
  [0x07, "115200 Bit/s"],
]);

/**
 * Section 2.34.2. 8E1 is both the recommended value and what `fan-controller` uses. The manual
 * notes that 8N1 puts the device outside the Modbus specification, which requires an 11-bit frame
 */
export const parityConfigurations = new Map([
  [0x00, "8E1 — 8 data, even, 1 stop (recommended)"],
  [0x01, "8O1 — 8 data, odd, 1 stop"],
  [0x02, "8N2 — 8 data, none, 2 stop"],
  [0x03, "8N1 — 8 data, none, 1 stop (outside the Modbus specification)"],
]);

/** Section 3.2, the values relevant to this project */
const identification = new Map([
  [0x0001, "ebm-papst Baureihe 84 / 112 / 150, spec 1.02"],
  [0x0002, "ebm-papst Baureihe 84 / 112 / 150, spec 2.01 … 3.01"],
  [0x0006, "ebm-papst Baureihe 84 / 112 / 150, spec 3.02"],
  [0x0007, "ebm-papst Baureihe 84 / 112 / 150, spec 4.00"],
  [0x0008, "ebm-papst Baureihe 84 / 112 / 150 / 200, spec 5.00"],
  [0x000a, "ebm-papst Baureihe 84 / 112 / 150 / 200 Lite, spec 5.00"],
  [0x000b, "ebm-papst Baureihe … Lite + Aufsteckmodul, spec 5.00"],
  [0x000c, "Besondere Anwendungen Baureihe 84 / 112 / 150 / 200, spec 5.02"],
  [0x000d, "ebm-papst Baureihe 84 / 112 / 150 / 200 Lite, spec 5.01"],
  [0x000e, "ebm-papst Baureihe, spec 6.00"],
  [0x0010, "ebm-papst RadiCal im Spiralgehäuse, spec 1.00"],
]);

/* -------------------------------------------------------------------------- */
/* Input registers — section 3.1                                              */
/* -------------------------------------------------------------------------- */

const input = (
  address: number,
  name: string,
  reference_: string,
  decode: Register["decode"],
  extra: Partial<Register> = {},
): Register => ({ address, space: "input", name, reference: reference_, decode, ...extra });

export const inputRegisters: readonly Register[] = [
  input(0xd000, "Identifikation", "3.2", choice(identification), { gloss: "Which electronics and protocol this is" }),
  input(0xd001, "Max. Anzahl Bytes", "3.3", unsigned("bytes"), {
    gloss: "Longest answer the fan will send. It refuses a read of more than 37 registers or over 80 bytes",
  }),
  input(0xd002, "Software Name Buscontroller", "3.4", (raw) => ({ kind: "text", text: `0x${hex(raw)}` })),
  input(0xd003, "Software Version Buscontroller", "3.5", (raw) => ({ kind: "text", text: `0x${hex(raw)}` })),
  input(0xd004, "Software Name Kommutierungscontroller", "3.6", (raw) => ({ kind: "text", text: `0x${hex(raw)}` })),
  input(0xd005, "Software Version Kommutierungscontroller", "3.7", (raw) => ({ kind: "text", text: `0x${hex(raw)}` })),

  input(0xd010, "Istdrehzahl", "3.8", speed, {
    gloss: "Actual speed. Capped at 1.02 × Maximale Drehzahl (0xFF00)",
    dependsOn: [MAXIMUM_SPEED],
  }),
  input(0xd011, "Motorstatus", "3.9", flags(motorStatusBits), { gloss: "Faults currently present" }),
  input(0xd012, "Warnung", "3.10", flags(warningBits), { gloss: "One step before the matching fault" }),
  input(0xd013, "Zwischenkreisspannung", "3.11", linkVoltage, {
    gloss: "DC link voltage",
    dependsOn: [VOLTAGE_REFERENCE],
  }),
  input(0xd014, "Zwischenkreisstrom", "3.12", linkCurrent, {
    gloss: "DC link current",
    dependsOn: [CURRENT_REFERENCE],
  }),
  input(0xd016, "Motortemperatur", "3.13", celsius),
  input(0xd017, "Elektroniktemperatur", "3.14", celsius, {
    gloss: "Measured inside the electronics housing, not in the air stream",
  }),
  input(0xd018, "Aktuelle Drehrichtung", "3.1", unsigned(""), { gloss: "Current direction of rotation" }),
  input(0xd019, "Aktueller Aussteuergrad", "3.15", modulation, {
    gloss: "Current modulation level, as a percentage of full drive",
  }),
  input(0xd01a, "Aktueller Sollwert", "3.16", unsigned(""), {
    gloss: "Coded the same way as Vorgabesollwert (D001): 0 … 64000",
  }),
  input(0xd01b, "Sensoristwert", "3.17.1", unsigned("")),
  input(0xd01c, "Zustand Enable - Eingang", "3.18", unsigned("")),
  input(0xd01e, "Aktueller Wirksinn", "3.19", choice(effectiveDirection), {
    gloss: "Which way the control error is computed",
  }),
  input(0xd021, "Aktuelle Leistung relativ", "3.20.1", relativePower, {
    dependsOn: [VOLTAGE_REFERENCE, CURRENT_REFERENCE],
  }),
  input(0xd023, "Sensoristwert 1", "3.17.2", unsigned("")),
  input(0xd027, "Aktuelle Leistung Watt", "3.20.2", unsigned("W"), {
    gloss: "Absolute, and needs no reference values — which is why fan-controller polls this one",
  }),
  input(0xd028, "Aktuelle Sollwert Quelle", "3.21", choice(setPointSource)),
  input(0xd029, "Energieverbrauchszähler", "3.22", energyCounter, { gloss: "MSB; the LSB is D02A" }),

  input(0xd02e, "Temperatur Temperatur-/Feuchtesensor 1", "3.17.3", tenthsCelsius),
  input(0xd02f, "Feuchte Temperatur-/Feuchtesensor 1", "3.17.3", relativeHumidity),
  input(0xd030, "Temperatur Temperatur-/Feuchtesensor 2", "3.17.3", tenthsCelsius),
  input(0xd031, "Feuchte Temperatur-/Feuchtesensor 2", "3.17.3", relativeHumidity),

  input(0xd032, "Drehzahl Flügelradanemometer", "3.17.4", unsigned("rpm"), { gloss: "Vane anemometer speed" }),
  input(0xd033, "Volumenstrom m³/h", "3.17.5", unsigned("m³/h")),
  input(0xd034, "Massestrom kg/h", "3.17.6", unsigned("kg/h")),
  input(0xd035, "Volumenstrom relativ codiert", "3.17.5", relativeTo(VOLUME_FLOW_REFERENCE, "m³/h", "Bezugswert Volumenstrom"), {
    dependsOn: [VOLUME_FLOW_REFERENCE],
  }),
  input(0xd036, "Massestrom relativ codiert", "3.17.6", relativeTo(MASS_FLOW_REFERENCE, "kg/h", "Bezugswert Massestrom"), {
    dependsOn: [MASS_FLOW_REFERENCE],
  }),
  input(0xd037, "Heartbeat", "3.23", unsigned("")),
  input(0xd038, "Temperatur PT1000 1", "3.17.7", pt1000),
  input(0xd039, "Temperatur PT1000 2", "3.17.7", pt1000),
];

/* -------------------------------------------------------------------------- */
/* Holding registers — section 2.1                                            */
/* -------------------------------------------------------------------------- */

const holding = (
  address: number,
  name: string,
  reference_: string,
  decode: Register["decode"],
  extra: Partial<Register> = {},
): Register => ({ address, space: "holding", name, reference: reference_, decode, ...extra });

/** Writing a plain unsigned value back unchanged */
const identity = (input_: number) => input_;

export const holdingRegisters: readonly Register[] = [
  holding(0xd001, "Vorgabesollwert", "2.3", unsigned(""), {
    gloss:
      "The set point. Only has an effect while Sollwert Quelle (D101) is RS485 (1). In speed control it is a fraction of Maximale Drehzahl; the low 4 bits are ignored",
    write: { input: { control: "number", min: 0, max: SET_POINT_MAX, step: 16 }, encode: identity },
  }),
  holding(0xd00c, "Adressierung ein/aus", "2.9", unsigned("")),
  holding(0xd00d, "Gespeicherter Sollwert", "2.10", unsigned("")),
  holding(0xd00f, "Enable RS485", "2.11", unsigned("")),

  holding(0xd100, "Ventilatoradresse", "2.12", unsigned(""), {
    gloss: "The fan's own Modbus address. fan-controller uses 0x02 and 0x03, avoiding 0x01 as a likely factory default",
    write: { input: { control: "number", min: 1, max: 247, step: 1 }, encode: identity },
  }),
  holding(0xd101, "Sollwert Quelle", "2.13", unsigned(""), {
    gloss: "Must be 1 (RS485) for Vorgabesollwert to do anything",
    write: { input: { control: "number", min: 0, max: 255, step: 1 }, encode: identity },
  }),
  holding(0xd102, "Vorzugslaufrichtung", "2.14", unsigned("")),
  holding(0xd103, "Sollwert Speichern", "2.15", unsigned(""), {
    gloss: "With this on, every write to Vorgabesollwert is copied to Gespeicherter Sollwert and survives a reset",
  }),
  holding(0xd106, "Betriebsart (Parametersatz 1)", "2.16", unsigned("")),
  holding(0xd108, "Wirksinn (Parametersatz 1)", "2.17", unsigned("")),
  holding(0xd10a, "P - Faktor (Parametersatz 1)", "2.18", unsigned("")),
  holding(0xd10c, "I - Faktor (Parametersatz 1)", "2.18", unsigned("")),
  holding(0xd10e, "Max. Aussteuergrad (Parametersatz 1)", "2.19", modulation),
  holding(0xd110, "Min. Aussteuergrad (Parametersatz 1)", "2.20", modulation),
  holding(0xd112, "Motor Stop Enable (Parametersatz 1)", "2.21", unsigned("")),

  holding(0xd116, "Start Aussteuergrad", "2.22", modulation),
  holding(0xd117, "Max. zulässiger Aussteuergrad", "2.23", modulation),
  holding(0xd118, "Min. zulässiger Aussteuergrad", "2.24", modulation),
  holding(MAXIMUM_SPEED, "Max. Drehzahl", "2.25", unsigned("rpm"), {
    gloss: "Directly in rpm, and the value every other speed is a fraction of. Must stay below D11A",
    write: { input: { control: "number", min: 0, max: 65_535, step: 1, unit: "rpm" }, encode: identity },
  }),
  holding(0xd11a, "Max. zulässige Drehzahl", "2.26", unsigned("rpm")),
  holding(0xd11f, "Hochlauframpe", "2.27", unsigned("")),
  holding(0xd120, "Auslauframpe", "2.27", unsigned("")),
  holding(0xd128, "Grenzdrehzahl", "2.28", unsigned("rpm")),
  holding(0xd135, "Max. zulässige Leistung", "2.31", unsigned("W")),
  holding(0xd145, "Grenzdrehzahl Laufüberwachung", "2.32", unsigned("rpm")),
  holding(0xd147, "Sensoristwert Quelle", "2.33", unsigned("")),

  holding(0xd149, "Übertragungsgeschwindigkeit", "2.34.1", choice(baudRates), {
    gloss: "Changing this drops the connection until the master is changed to match",
    write: { input: { control: "choice", options: baudRates }, encode: identity },
  }),
  holding(0xd14a, "Parity Konfiguration", "2.34.2", choice(parityConfigurations), {
    gloss: "Changing this drops the connection until the master is changed to match",
    write: { input: { control: "choice", options: parityConfigurations }, encode: identity },
  }),
  holding(0xd155, "Max. Leistung", "2.30", unsigned("W")),

  holding(VOLTAGE_REFERENCE, "Bezugswert Zwischenkreisspannung", "2.45", unsigned("V")),
  holding(CURRENT_REFERENCE, "Bezugswert Zwischenkreisstrom", "2.46", unsigned("A")),
  holding(0xd1a2, "Seriennummer Ventilator", "2.47.1", unsigned(""), { gloss: "Continues into D1A3" }),
  holding(0xd1a4, "Produktionsdatum Ventilator", "2.47.1", unsigned("")),
  holding(VOLUME_FLOW_REFERENCE, "Bezugswert Volumenstrom", "2.48", unsigned("m³/h")),
  holding(MASS_FLOW_REFERENCE, "Bezugswert Massestrom", "2.49", unsigned("kg/h")),
];

export const radical: Device = {
  id: "radical",
  name: "ebm-papst RadiCal im Spiralgehäuse",
  documentation: "docs/manufacturer/radical/ — MODBUS Parameter RadiCal im Spiralgehäuse V1.00",
  defaults: {
    // The manual recommends 19200 8E1, which is what fan-controller is built for
    address: 0x02,
    baudRate: 19_200,
    parity: "even",
    addressRange: [1, 247],
  },
  registers: [...inputRegisters, ...holdingRegisters],
};
