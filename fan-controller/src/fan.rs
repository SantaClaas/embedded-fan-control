//! ebm-pabst [RadiCal centrifugal fans in scroll housings for residential ventilation](https://www.ebmpapst.com/us/en/campaigns/product-campaigns/centrifugal-fans/radical-with-scroll-housing.html)
//! specific configuration and constants

use crc::{Crc, CRC_16_MODBUS};
use defmt::Format;
use embassy_rp::uart;
use embassy_rp::uart::{DataBits, Parity, StopBits};

pub(crate) const BAUD_RATE: u32 = 19_200;
pub(crate) fn get_configuration() -> uart::Config {
    // I wish I could make this constant time but default isn't, there is no new and struct is non-exhaustive 😅
    let mut configuration: uart::Config = uart::Config::default();
    configuration.baudrate = BAUD_RATE;
    configuration.data_bits = DataBits::DataBits8;
    configuration.parity = Parity::ParityEven;
    configuration.stop_bits = StopBits::STOP1;
    // Setting inverts should be a no-op as they should be false by default
    configuration
}

pub(crate) const MAX_SET_POINT: u16 = 64_000;

#[derive(Default, Format, Debug)]
pub(crate) enum State {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

pub(crate) mod address {
    pub(crate) const FAN_1: u8 = 0x02;
    pub(crate) const FAN_2: u8 = 0x03;
}

pub(super) mod holding_registers {
    pub(crate) const REFERENCE_SET_POINT: [u8; 2] = 0xd001_u16.to_be_bytes();
}
