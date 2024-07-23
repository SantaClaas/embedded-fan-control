use crc::{Crc, CRC_16_MODBUS};

pub(super) mod function_code {
    pub const READ_HOLDING_REGISTER: u8 = 0x03;
    pub const READ_INPUT_REGISTER: u8 = 0x04;
    pub const WRITE_SINGLE_REGISTER: u8 = 0x06;
}


/// Used to create CRC checksums when forming modbus messages
pub(super) const CRC: Crc<u16> = Crc::<u16>::new(&CRC_16_MODBUS);