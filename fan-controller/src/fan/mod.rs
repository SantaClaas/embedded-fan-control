//! ebm-pabst [RadiCal centrifugal fans in scroll housings for residential ventilation](https://www.ebmpapst.com/us/en/campaigns/product-campaigns/centrifugal-fans/radical-with-scroll-housing.html)
//! specific configuration and constants

pub(crate) mod set_point;

use core::str::FromStr;

use crate::{configuration, modbus};
use defmt::{error, info, Format};
use embassy_rp::dma::Channel;
use embassy_rp::gpio::{Level, Output, Pin};
use embassy_rp::interrupt::typelevel::Binding;
use embassy_rp::uart::{
    BufferedInterruptHandler, BufferedUart, DataBits, Parity, RxPin, StopBits, TxPin,
};
use embassy_rp::{uart, Peripheral};
use embassy_time::{block_for, with_timeout, Duration, TimeoutError, Timer};
use embedded_io_async::{Read, Write};

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

/// Settings specific to our use case for these fans. They are custom tuned to the house.
/// For example, we don't run the fans at full speed to reduce wear on them
pub(crate) mod user_setting {
    use crate::fan::{
        self,
        set_point::{self, SetPoint},
    };

    /// Max speed 64000 / 3.3
    pub(crate) const LOW: SetPoint = match SetPoint::new(19_393) {
        Ok(setting) => setting,
        Err(error) => panic!("Invalid value"),
    };
    /// Max speed 64000 / 2.4
    pub(crate) const MEDIUM: SetPoint = match SetPoint::new(26_666) {
        Ok(setting) => setting,
        Err(error) => panic!("Invalid value"),
    };

    /// Max speed 50%
    /// Not set to full speed to not wear out the fans
    pub(crate) const HIGH: SetPoint = match SetPoint::new(set_point::MAX / 2) {
        Ok(setting) => setting,
        Err(error) => panic!("Invalid value"),
    };
}

#[derive(Default, Format, Debug)]
pub(crate) enum State {
    #[default]
    Off,
    Low,
    Medium,
    High,
}
impl State {
    pub(crate) const fn next(&self) -> Self {
        match self {
            Self::Off => Self::Low,
            Self::Low => Self::Medium,
            Self::Medium => Self::High,
            Self::High => Self::Off,
        }
    }
}

pub(crate) mod address {
    pub(crate) const FAN_1: u8 = 0x02;
    pub(crate) const FAN_2: u8 = 0x03;
}

pub(super) mod holding_registers {
    pub(crate) const REFERENCE_SET_POINT: [u8; 2] = 0xd001_u16.to_be_bytes();
}

pub(crate) mod input_registers {
    pub(crate) const TEMPERATURE_SENSOR_1: [u8; 2] = 0xd02e_u16.to_be_bytes();
    pub(crate) const HUMIDITY_SENSOR_1: [u8; 2] = 0xd02f_u16.to_be_bytes();
    pub(crate) const TEMPERATURE_SENSOR_2: [u8; 2] = 0xd030_u16.to_be_bytes();
    pub(crate) const HUMIDITY_SENSOR_2: [u8; 2] = 0xd031_u16.to_be_bytes();
}

pub(crate) enum Fan {
    One,
    Two,
}

pub(crate) struct FanResponse<const N: usize> {
    data: [u8; N],
    length: usize,
}

impl<const N: usize> FanResponse<N> {
    pub(crate) fn new(data: [u8; N], length: usize) -> Self {
        Self { data, length }
    }

    pub(crate) fn as_slice(&self) -> &[u8] {
        &self.data[..self.length]
    }
}
