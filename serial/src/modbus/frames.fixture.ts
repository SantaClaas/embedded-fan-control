/**
 * Frames copied out of the device manuals, CRCs and all.
 *
 * These are worth more than frames computed here would be: the manufacturer wrote the bytes *and*
 * the check bytes, so a test built on them fails if this implementation disagrees with the device
 * rather than merely with itself.
 *
 * Relay frames: LC-Modbus-1R-D7 manual, "Modbus RTU instruction introduction".
 * Temperature sensor frames: docs/temperature-sensor.md, "MODBUS command frame".
 *
 * `relayPowerUp` below is the exception: no manual prints it, it is not made of frames, and it is
 * kept in its own object so that everything in `relay` and `temperatureSensor` stays checkable.
 */

/** `"FF 05 00 00"` → bytes. Whitespace is ignored, so frames can be written as the manual prints them */
export function bytes(hex: string): Uint8Array {
  const digits = hex.replace(/\s+/g, "");
  const out = new Uint8Array(digits.length / 2);

  for (let index = 0; index < out.length; index++) {
    out[index] = Number.parseInt(digits.slice(index * 2, index * 2 + 2), 16);
  }

  return out;
}

/** Text the way a device puts it on the line, for the greetings that are not frames at all */
export function ascii(text: string): Uint8Array {
  return new TextEncoder().encode(text);
}

export const relay = {
  /** Close relay 1, manual mode */
  closeRelayRequest: bytes("FF 05 00 00 FF 00 99 E4"),
  closeRelayResponse: bytes("FF 05 00 00 FF 00 99 E4"),
  openRelayRequest: bytes("FF 05 00 00 00 00 D8 14"),

  /** Close all eight relays */
  closeAllRequest: bytes("FF 0F 00 00 00 08 01 FF 30 1D"),
  openAllRequest: bytes("FF 0F 00 00 00 08 01 00 70 5D"),
  writeCoilsResponse: bytes("FF 0F 00 00 00 08 41 D3"),

  /** Read the device address out of holding register 0 */
  readAddressRequest: bytes("00 03 00 00 00 01 85 DB"),
  readAddressResponse: bytes("00 03 02 00 FF C5 C4"),

  readRelayStateRequest: bytes("FF 01 00 00 00 08 28 12"),
  readRelayStateResponse: bytes("FF 01 01 01 A1 A0"),

  readInputStateRequest: bytes("FF 02 00 00 00 08 6C 12"),
  readInputStateResponse: bytes("FF 02 01 01 51 A0"),

  /** Set the baud rate to 19 200 */
  setBaudRequest: bytes("FF 10 03 E9 00 01 02 00 04 CA 0E"),
  setBaudResponse: bytes("FF 10 03 E9 00 01 C5 A7"),

  readBaudRequest: bytes("FF 03 03 E8 00 01 11 A4"),
  readBaudResponse: bytes("FF 03 02 00 04 90 53"),

} as const;

/**
 * The relay module's power-up greeting, and the exchange it ruined.
 *
 * Kept out of `relay` above because neither is a frame: `crc.test.ts` holds every entry in these
 * fixture objects to the manufacturer's own check bytes, and these two have no check bytes to hold
 * them to. They were captured from the hardware and are written down in docs/relay.md
 */
export const relayPowerUp = {
  /** Sent unasked on power-up. Not a frame, and not addressed to anyone */
  greeting: ascii("Thank you for using the Modbus modules of LCTECH\r\n"),

  /**
   * The first exchange after power-up, as it actually arrived: the echo of `relay.closeRelayRequest`
   * with the greeting driven through it. `FF 05 00 00 FF 00 99 E4` became `FF 05 00 00 FF 80 0A` and
   * the greeting followed. Nothing in here checks out, which is the point — the relay stayed open
   */
  spoiledEcho: bytes(
    "FF 05 00 00 FF 80 0A 54 68 61 6E 6B 20 79 6F 75 20 66 6F 72 20 75 73 69 6E 67 20 74 68 65 20" +
      "4D 6F 64 62 75 73 20 6D 6F 64 75 6C 65 73 20 6F 66 20 4C 43 54 45 43 48 0D 0A",
  ),
} as const;

export const temperatureSensor = {
  readTemperatureRequest: bytes("01 04 00 01 00 01 60 0A"),
  /** 0x0131 → 30.5 °C */
  readTemperatureResponse: bytes("01 04 02 01 31 79 74"),

  // The two humidity frames are the only ones in either manual whose printed check bytes were
  // wrong: the request was given as `C1 CA` and the response as `D1 BA`. These are the recomputed
  // values, and docs/temperature-sensor.md now carries the same correction and says why
  readHumidityRequest: bytes("01 04 00 02 00 01 90 0A"),
  /** 0x0222 → 54.6 % */
  readHumidityResponse: bytes("01 04 02 02 22 38 49"),

  /** Both values in one read, which is what the app actually issues */
  readBothRequest: bytes("01 04 00 01 00 02 20 0B"),
  readBothResponse: bytes("01 04 04 01 31 02 22 2A CE"),
} as const;
