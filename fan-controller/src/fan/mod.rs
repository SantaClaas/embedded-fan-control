//! ebm-pabst [RadiCal centrifugal fans in scroll housings for residential ventilation](https://www.ebmpapst.com/us/en/campaigns/product-campaigns/centrifugal-fans/radical-with-scroll-housing.html)
//! specific configuration and constants

/// Its own crate so it can be tested on the host, re-exported here because it is part of what a
/// fan is. See the crate documentation for why it cannot live in this one
pub(crate) use ::set_point;

/// Decoding what the fan reports about itself, in its own crate for the same reason as
/// [`set_point`] and re-exported here for the same one
pub(crate) use ::fan_sensor as sensor;

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

    /// A third of [`HIGH`], which is the bottom third of the range Home Assistant knows about
    pub(crate) const LOW: SetPoint = match SetPoint::new(set_point::MAX / 6) {
        Ok(setting) => setting,
        Err(_error) => panic!("Invalid value"),
    };
    /// Two thirds of [`HIGH`], see [`LOW`]
    pub(crate) const MEDIUM: SetPoint = match SetPoint::new(set_point::MAX / 3) {
        Ok(setting) => setting,
        Err(_error) => panic!("Invalid value"),
    };

    /// Max speed 50%
    /// Not set to full speed to not wear out the fans. Home Assistant is told this is the top of
    /// its range, so the three settings the button cycles through are the thirds of that range
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

pub(super) mod holding_register {
    use crate::modbus;

    pub(crate) const REFERENCE_SET_POINT: modbus::register::Address =
        modbus::register::Address::new(0xd001_u16);

    /// The speed the fan is configured for, which every speed it reports and accepts is a fraction
    /// of. Only changes when the fan is reconfigured, so it is read once rather than on every poll
    pub(crate) const MAXIMUM_SPEED: modbus::register::Address =
        modbus::register::Address::new(super::sensor::MAXIMUM_SPEED_REGISTER);
}

/// Where the fan reports what it measures about itself. Read only, and read as two runs rather
/// than register by register because a range costs the same round trip as one register.
/// The addresses and the layout of each run belong to the [`sensor`] crate, which is what decodes
/// them; these only wrap them in the address type the modbus client asks for
pub(super) mod input_register {
    use crate::modbus;

    /// The run holding the actual speed and both temperatures
    pub(crate) const STATUS: modbus::register::Address =
        modbus::register::Address::new(super::sensor::STATUS_START);

    /// The run holding the current power draw and the energy counter
    pub(crate) const ENERGY: modbus::register::Address =
        modbus::register::Address::new(super::sensor::ENERGY_START);
}

#[derive(Clone, Copy)]
pub(crate) enum Fan {
    One,
    Two,
}
