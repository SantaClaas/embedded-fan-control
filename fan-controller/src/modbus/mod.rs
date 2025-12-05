pub(crate) mod client;
pub(crate) mod device;
pub(crate) mod function;
pub(crate) mod register;

use crc::{CRC_16_MODBUS, Crc};

pub(crate) use client::Client;

/// Used to create CRC checksums when forming modbus messages
pub(super) const CRC: Crc<u16> = Crc::<u16>::new(&CRC_16_MODBUS);
