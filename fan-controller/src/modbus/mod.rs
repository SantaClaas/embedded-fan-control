pub(crate) mod client;

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

pub(crate) trait Function {
    const CODE: u8;
}

pub(crate) struct ReadInputRegister {
    address: u16,
    number_of_registers: u16,
}

impl ReadInputRegister {
    pub(crate) fn new(address: u16, number_of_registers: u16) -> Self {
        Self {
            address,
            number_of_registers,
        }
    }
}

impl Function for ReadInputRegister {
    const CODE: u8 = function_code::READ_INPUT_REGISTER;
}

pub(crate) struct Message<F> {
    address: u8,
    function: F,
}

pub(crate) trait ToBytes<const LENGTH: usize> {
    fn to_bytes(&self) -> [u8; LENGTH];
}

impl ToBytes<8> for Message<ReadInputRegister> {
    fn to_bytes(&self) -> [u8; 8] {
        let address_bytes = self.function.address.to_be_bytes();
        let length_bytes = self.function.number_of_registers.to_be_bytes();
        let mut buffer = [
            // Device address
            self.address,
            // Modbus function code
            self.code(),
            // Starting address
            address_bytes[0],
            address_bytes[1],
            // Number of registers to read
            length_bytes[0],
            length_bytes[1],
            // CRC checksum (placeholder)
            0,
            0,
        ];

        let checksum = CRC.checksum(&buffer[..6]).to_be_bytes();
        // They come out reversed (or is us using to_be_bytes reversed?)
        buffer[6] = checksum[1];
        buffer[7] = checksum[0];
        buffer
    }
}

impl<F: Function> Message<F> {
    pub(crate) fn new(address: u8, function: F) -> Self {
        Self { address, function }
    }

    const fn code(&self) -> u8 {
        F::CODE
    }
}
