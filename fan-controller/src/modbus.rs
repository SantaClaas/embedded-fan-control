use crc::{Crc, CRC_16_MODBUS};
pub(super) mod function_code {
    pub const READ_HOLDING_REGISTER: u8 = 0x03;
    pub const READ_INPUT_REGISTER: u8 = 0x04;
    pub const WRITE_SINGLE_REGISTER: u8 = 0x06;
}

/// Used to create CRC checksums when forming modbus messages
pub(super) const CRC: Crc<u16> = Crc::<u16>::new(&CRC_16_MODBUS);

/// Calculate the time in microseconds that needs to be waited between sending modbus messages
/// Modbus delay between messages is required to be 3.5 character times (bytes).
/// The function will return more than the required time if the calculation (division) does not
/// result in a whole number as it is better to wait longer than too short.
pub(super) const fn get_message_delay(baud_rate: u32) -> u64 {
    /// Modbus delay between messages in bits
    /// The modbus delay between messages is 3.5 bytes or 28 bits
    const DELAY_BITS: u64 = 28 + 4;

    // To send one bit it takes 1/19200 seconds.
    // To send one bit it takes 1 * 1,000,000 / 19200 microseconds.
    // To send 3.5 bytes it takes 1,000,000 * 28 / 19200 microseconds.
    // Putting the division last to avoid add on effect of inaccurate floating point division
    const MICROSECONDS_FOR_BITS: u64 = 1_000_000 * DELAY_BITS;

    // Using floating point division to be closer to the logical mathematical result
    // (e.g. 3 / 2 = 1.5 instead of 1 with integer division)

    // Round up as it is better to wait longer than too short
    MICROSECONDS_FOR_BITS.div_ceil(baud_rate as u64)
}

