//! The two RS-485 temperature and humidity sensors, which measure the air rather than a machine.
//!
//! They sit on the relay's bus rather than on the fans'. That is not a preference: the RP2040 has
//! two UARTs and both are already spoken for, and of the two the sensors can only join the
//! relay's, because they answer 9600 8N1 — the relay's framing exactly, and the opposite of the
//! fans' 8E1. So the second bus, which used to carry one device owned outright by one task, is now
//! shared the way the fans' bus is.
//!
//! What they report and how it is coded lives in the [`sensor`] crate, for the same reason the
//! fans' does. `docs/temperature-sensor.md` is the only documentation these devices have.

/// Decoding what a sensor measures, in its own crate so it can be tested on the host, and
/// re-exported here because it is part of what a sensor is. See the crate documentation for why it
/// cannot live in this one
pub(crate) use ::temperature_sensor as sensor;

/// The bit rate the sensors ship with, and the one they are left at. Settable to 14400 or 19200,
/// but there is nothing to gain: two registers every half minute is not a lot of bus time, and the
/// relay they share the line with is fixed at 9600 unless it is reconfigured too
pub(crate) const BAUD_RATE: u32 = 9_600;

/// Both devices on a bus have to agree about the line or one of them is simply unreachable, and
/// the firmware opens it once, from the relay's configuration. Checking that here means a change
/// to either bit rate fails the build rather than producing a controller whose sensors time out
const _: () = core::assert!(
    BAUD_RATE == crate::relay::BAUD_RATE,
    "the sensors and the relay share a UART, so they have to be configured for the same bit rate"
);

pub(crate) mod address {
    use crate::modbus;

    /// The sensors ship at `0x01` and have to be re-addressed before they are wired in, because
    /// two of them at one address collide and answer over each other. These continue the fans'
    /// numbering rather than starting over at `0x02`: the two buses could not collide even if they
    /// shared a number, but one address for one device reads back more easily in a log, and
    /// `0x01` is skipped here for the same reason the fans skip it — it is a likely factory
    /// default for whatever is added next
    pub(crate) const SENSOR_1: modbus::device::Address = modbus::device::Address::new(0x04);
    pub(crate) const SENSOR_2: modbus::device::Address = modbus::device::Address::new(0x05);
}

/// Where a sensor reports what it measures. Read only, like the fans' input registers, and read as
/// one run because a range costs the same round trip as a single register.
/// The address and the layout of the run belong to the [`sensor`] crate, which is what decodes it;
/// this only wraps it in the address type the modbus client asks for
pub(super) mod input_register {
    use crate::modbus;

    /// The run holding the temperature and the humidity
    pub(crate) const MEASUREMENTS: modbus::register::Address =
        modbus::register::Address::new(super::sensor::MEASUREMENTS_START);
}

/// Which of the two sensors a reading came from. They are identical devices at different addresses
/// and in different places, so nothing but the address and the topic tells them apart
#[derive(Clone, Copy)]
pub(crate) enum TemperatureSensor {
    One,
    Two,
}
