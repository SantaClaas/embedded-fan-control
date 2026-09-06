//! What the RS-485 temperature and humidity sensors report: the air temperature and the relative
//! humidity where they hang, which is what the fans are run for in the first place.
//!
//! Both arrive as signed tenths of their unit in two input registers, which are read only. That
//! coding is the whole reason this is its own crate rather than a module of the firmware:
//! `fan-controller` only builds for `thumbv6m-none-eabi`, which has no test harness, so anything
//! left in there is compiled by nothing and rots unnoticed. See the `fan_sensor` and `set_point`
//! crates, which are here for the same reason.
//!
//! The register addresses and the coding are from `docs/temperature-sensor.md`, which is a
//! transcription of what came with the device rather than a manufacturer's document — and which
//! had two wrong check bytes in it until the tests in `serial/src/modbus` caught them. It is the
//! least trustworthy documentation of the three devices, so the tests below quote its own worked
//! examples rather than restating what this code does.

#![no_std]

use core::fmt::Write;

/// Where the run of input registers holding the two measurements starts, and how many registers it
/// spans. Both are read in one request, which is the frame the document prints as "continuously
/// read the temperature and humidity" and the only one either value has ever been read by.
///
/// Note the address is 1 based, unlike the fans' registers and unlike the relay's: the first
/// register of this device is `0x0001`, and there is no `0x0000`
pub const MEASUREMENTS_START: u16 = 0x0001;
pub const MEASUREMENTS_LENGTH: usize = 2;

/// Offsets into the block starting at [`MEASUREMENTS_START`]
mod measurement {
    /// `0x0001`
    pub(super) const TEMPERATURE: usize = 0;
    /// `0x0002`
    pub(super) const HUMIDITY: usize = 1;
}

/// What the device keeps in its holding registers: how it is addressed, how fast it talks, and the
/// two offsets it adds to what it measures.
///
/// The firmware never writes any of them — a sensor is given its address and its corrections from
/// the [serial tool](../serial) before it is wired in, the same way the fans and the relay are, and
/// the values are stored in the device rather than in this repository. They are here because the
/// addresses belong with the ones above rather than being spread over two documents
pub mod holding_register {
    /// 1 … 247, and 1 as it ships. Two sensors on one bus therefore collide until at least one of
    /// them is moved
    pub const DEVICE_ADDRESS: u16 = 0x0101;
    /// `0`: 9600, `1`: 14400, `2`: 19200. 9600 as it ships, which is what the controller's second
    /// bus runs at
    pub const BAUD_RATE: u16 = 0x0102;
    /// Signed tenths of a degree, −10.0 … +10.0, added to what the device reports
    pub const TEMPERATURE_CORRECTION: u16 = 0x0103;
    /// Signed tenths of a percent, −10.0 … +10.0, added to what the device reports
    pub const HUMIDITY_CORRECTION: u16 = 0x0104;
}

/// A measurement as the device reports it: signed tenths of whatever it is measuring.
///
/// Kept in tenths rather than divided out, because a tenth is the resolution the device has and
/// floating point on a chip without an FPU would buy nothing but rounding. It is written out with
/// its decimal point only where it leaves the firmware, which is the JSON below and the log
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tenths(pub i16);

impl Tenths {
    /// The sign written separately from the digits, because a value between −1 and 0 has none of
    /// its own: −0.5 is `0` tenths of a unit with a minus in front of it
    fn parts(&self) -> (&'static str, u16, u16) {
        let magnitude = self.0.unsigned_abs();
        let sign = if self.0.is_negative() { "-" } else { "" };
        (sign, magnitude / 10, magnitude % 10)
    }
}

impl core::fmt::Display for Tenths {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let (sign, whole, fraction) = self.parts();
        write!(formatter, "{sign}{whole}.{fraction}")
    }
}

#[cfg(feature = "defmt")]
impl defmt::Format for Tenths {
    fn format(&self, formatter: defmt::Formatter) {
        let (sign, whole, fraction) = self.parts();
        defmt::write!(formatter, "{=str}{=u16}.{=u16}", sign, whole, fraction);
    }
}

/// One poll of a sensor
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// Degrees celsius, and genuinely signed: the device is specified down to −20 °C and the
    /// document spells the coding out with a worked example below zero
    pub temperature: Tenths,
    /// Relative humidity in percent. Signed for the same reason the correction value is: an offset
    /// applied to a reading near zero can take it below it. Note the fans code the same quantity
    /// completely differently, as a fraction of `65536`
    pub humidity: Tenths,
}

/// Turns the block of input registers into the quantities it describes.
///
/// Both are two's complement, which the raw register does not say and which only the temperature's
/// worked example in the document makes explicit
pub fn decode(measurements: &[u16; MEASUREMENTS_LENGTH]) -> Reading {
    Reading {
        temperature: Tenths(measurements[measurement::TEMPERATURE] as i16),
        humidity: Tenths(measurements[measurement::HUMIDITY] as i16),
    }
}

/// Enough for both values at their longest, minus signs included. Proven by
/// `json_fits_the_worst_case`
pub const JSON_CAPACITY: usize = 64;

impl Reading {
    /// The payload Home Assistant reads, as one JSON object per sensor so that both values arrive
    /// in a single publish and each sensor picks its own out with a value template. The same shape
    /// the fans' readings are published in
    pub fn to_json(&self) -> heapless::String<JSON_CAPACITY> {
        let mut json = heapless::String::new();

        // The buffer is proven large enough by the test below, so the only way this can fail is a
        // change to the fields without a change to the capacity, which that test catches
        let result = write!(
            json,
            "{{\"temperature\":{},\"humidity\":{}}}",
            self.temperature, self.humidity
        );

        debug_assert!(result.is_ok(), "the reading did not fit JSON_CAPACITY");
        let _ = result;

        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `to_string` belongs to `alloc`, which a `no_std` crate does not have even in its tests, so
    /// a `Display` implementation is checked by writing it somewhere that exists here
    fn format(value: Tenths) -> heapless::String<16> {
        let mut string = heapless::String::new();
        write!(string, "{value}").expect("every value fits sixteen bytes");
        string
    }

    /// The document's own worked example: "Temperature value = 0x131, converted to decimal 305,
    /// actual temperature value = 305/10 = 30.5℃", and "Humidity value=0x222, converted to decimal
    /// 546, actual humidity value=546 / 10 = 54.6%"
    #[test]
    fn decodes_the_documented_example() {
        let reading = decode(&[0x0131, 0x0222]);

        assert_eq!(reading.temperature, Tenths(305));
        assert_eq!(reading.humidity, Tenths(546));
        assert_eq!(format(reading.temperature).as_str(), "30.5");
        assert_eq!(format(reading.humidity).as_str(), "54.6");
    }

    /// The document's example below zero: "temperature value=0xFF33, converted to decimal -205,
    /// actual temperature = -20.5℃"
    #[test]
    fn decodes_the_documented_example_below_zero() {
        let reading = decode(&[0xFF33, 0x0222]);

        assert_eq!(reading.temperature, Tenths(-205));
        assert_eq!(format(reading.temperature).as_str(), "-20.5");
    }

    /// A value between −1 and 0 has no sign of its own in either of its digits, so the minus has
    /// to be written before them rather than fallen out of the division
    #[test]
    fn keeps_the_sign_of_a_value_below_one() {
        assert_eq!(format(Tenths(-5)).as_str(), "-0.5");
        assert_eq!(format(Tenths(0)).as_str(), "0.0");
        assert_eq!(format(Tenths(5)).as_str(), "0.5");
    }

    #[test]
    fn serializes_to_json() {
        let reading = decode(&[0x0131, 0x0222]);

        assert_eq!(
            reading.to_json().as_str(),
            r#"{"temperature":30.5,"humidity":54.6}"#
        );
    }

    /// [`JSON_CAPACITY`] is asserted against rather than guessed at
    #[test]
    fn json_fits_the_worst_case() {
        let reading = Reading {
            temperature: Tenths(i16::MIN),
            humidity: Tenths(i16::MIN),
        };

        let json = reading.to_json();

        // Would have been silently truncated rather than panicking in a release build
        assert!(
            json.ends_with('}'),
            "truncated at {} bytes: {json}",
            json.len()
        );
        assert!(json.len() <= JSON_CAPACITY);
    }
}
