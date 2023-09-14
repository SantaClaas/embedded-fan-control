function swapEndianness16(number: number) {
  // Source: https://stackoverflow.com/questions/5320439/how-do-i-swap-endian-ness-byte-order-of-a-variable-in-javascript
  return ((number & 0xff) << 8) | ((number >> 8) & 0xff);
}

/**
 * Creates the cyclic redundancy check for the given data
 * @param data
 * @returns big endian crc
 */
export function crc16(data: Uint8Array): number {
  let crc = 0xffff;
  let polynomial = 0xa001;

  for (let i = 0; i < data.length; i++) {
    crc ^= data[i];

    for (let j = 0; j < 8; j++) {
      if ((crc & 1) === 1) {
        crc >>= 1;
        crc ^= polynomial;
      } else {
        crc >>= 1;
      }
    }
  }

  return swapEndianness16(crc);
}
