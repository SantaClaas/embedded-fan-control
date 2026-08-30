/**
 * The CRC-16 every Modbus RTU frame ends with.
 *
 * It is the reflected algorithm: polynomial 0xA001 (0x8005 bit-reversed), seeded with 0xFFFF, no
 * final inversion. On the wire the two bytes go out **low byte first**, which is the opposite
 * order from every other 16-bit field in the protocol — see `append` and `read`.
 */

/**
 * The table costs 512 bytes and turns the per-byte inner loop into one lookup. Built once at
 * module load rather than written out as a literal, so there is nothing to mistype
 */
const TABLE = /* @__PURE__ */ (() => {
  const table = new Uint16Array(256);

  for (let byte = 0; byte < 256; byte++) {
    let crc = byte;

    for (let bit = 0; bit < 8; bit++) {
      crc = crc & 1 ? (crc >> 1) ^ 0xa001 : crc >> 1;
    }

    table[byte] = crc;
  }

  return table;
})();

/** The CRC of `bytes`, or of the slice `[start, end)` of it */
export function crc16(bytes: Uint8Array, start = 0, end = bytes.length): number {
  let crc = 0xffff;

  for (let index = start; index < end; index++) {
    // The loop bound is the array's own length, so the index is always in range. `!` rather than a
    // branch that can never be taken
    const byte = bytes[index]!;
    crc = (crc >> 8) ^ TABLE[(crc ^ byte) & 0xff]!;
  }

  return crc;
}

/**
 * Whether a whole frame — address, PDU and the two CRC bytes — checks out.
 *
 * A frame shorter than four bytes cannot be one: an address, a function code and a CRC is already
 * four, and there is no function code without at least one more byte after it
 */
export function isValid(frame: Uint8Array): boolean {
  if (frame.length < 4) return false;

  const expected = crc16(frame, 0, frame.length - 2);
  return expected === read(frame, frame.length - 2);
}

/** Reads a CRC from `frame` at `offset`, low byte first */
export function read(frame: Uint8Array, offset: number): number {
  return frame[offset]! | (frame[offset + 1]! << 8);
}

/**
 * Writes the CRC of everything before `offset` into `frame` at `offset`, low byte first.
 *
 * Takes the frame with its last two bytes already reserved rather than returning a new array, so
 * that building a frame is one allocation
 */
export function append(frame: Uint8Array): void {
  const offset = frame.length - 2;
  const crc = crc16(frame, 0, offset);

  frame[offset] = crc & 0xff;
  frame[offset + 1] = crc >> 8;
}
