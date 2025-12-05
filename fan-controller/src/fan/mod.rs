//! ebm-pabst [RadiCal centrifugal fans in scroll housings for residential ventilation](https://www.ebmpapst.com/us/en/campaigns/product-campaigns/centrifugal-fans/radical-with-scroll-housing.html)
//! specific configuration and constants

pub(crate) mod set_point;

use embassy_rp::uart::{self, DataBits, Parity, StopBits};

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
    use crate::fan::set_point::{self, SetPoint};

    /// Max speed 64000 / 3.3
    pub(crate) const LOW: SetPoint = match SetPoint::new(19_393) {
        Ok(setting) => setting,
        Err(_error) => panic!("Invalid value"),
    };
    /// Max speed 64000 / 2.4
    pub(crate) const MEDIUM: SetPoint = match SetPoint::new(26_666) {
        Ok(setting) => setting,
        Err(_error) => panic!("Invalid value"),
    };

    /// Max speed 50%
    /// Not set to full speed to not wear out the fans
    pub(crate) const HIGH: SetPoint = match SetPoint::new(set_point::MAX / 2) {
        Ok(setting) => setting,
        Err(_error) => panic!("Invalid value"),
    };
}

pub(crate) mod address {
    use crate::modbus;

    /// Starting fan with address 0x02 as 0x01 might be occupied by as a default address
    pub(crate) const FAN_1: modbus::device::Address = modbus::device::Address::new(0x02);
    pub(crate) const FAN_2: modbus::device::Address = modbus::device::Address::new(0x03);
}

pub(super) mod holding_registers {
    use crate::modbus;

    pub(crate) const REFERENCE_SET_POINT: modbus::register::Address =
        modbus::register::Address::new(0xd001_u16);
}

pub(crate) enum Fan {
    One,
    Two,
}
