//! The desired speed of an ebm-papst RadiCal fan.
//!
//! This lives in its own crate so it can be tested on the host. `fan-controller` only builds for
//! `thumbv6m-none-eabi`, which has no test harness, and it cannot build for the host because
//! `cortex-m` uses ARM inline assembly, so anything left in there is compiled by nothing and rots
//! unnoticed.

#![no_std]

use core::{ops::Deref, str::FromStr};

/// The highest value the fan accepts. In speed control it means the maximum revolutions per
/// minute the fan is configured for, and zero means standstill.
/// See MODBUS Parameter RadiCal im Spiralgehäuse V1.00, section 2.3
pub const MAX: u16 = 64_000;

/// Describes the desired speed of the fan from 0 to [`MAX`]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct SetPoint(u16);

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, PartialEq, Eq)]
pub struct SetPointOutOfBoundsError;

impl SetPoint {
    pub const ZERO: Self = match Self::new(0) {
        Ok(setting) => setting,
        Err(_error) => panic!("Invalid value. This should not be reachable."),
    };

    pub const fn new(set_point: u16) -> Result<Self, SetPointOutOfBoundsError> {
        if set_point > MAX {
            return Err(SetPointOutOfBoundsError);
        }

        Ok(Self(set_point))
    }

    /// This should always succeed
    pub fn to_string(self) -> heapless::String<5> {
        heapless::String::<5>::try_from(self.0)
            .expect("The maximum value for a u16 is 65535 which is a 5 digit number and should should be represented as a string with 5 characters and thus fit into a string with a capacity of 5")
    }
}

impl Deref for SetPoint {
    type Target = u16;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, PartialEq, Eq)]
pub enum ParseSetPointError {
    ParseInt,
    SettingOutOfBounds(SetPointOutOfBoundsError),
}

impl From<core::num::ParseIntError> for ParseSetPointError {
    fn from(_error: core::num::ParseIntError) -> Self {
        ParseSetPointError::ParseInt
    }
}

impl FromStr for SetPoint {
    type Err = ParseSetPointError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let set_point = s.parse()?;
        Self::new(set_point).map_err(ParseSetPointError::SettingOutOfBounds)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These are important hardcoded values I want to make sure are not changed accidentally
    #[test]
    fn setting_does_not_exceed_max_set_point() {
        assert_eq!(MAX, 64_000);
        assert_eq!(SetPoint::new(64_000), Ok(SetPoint(64_000)));
        assert_eq!(SetPoint::new(64_000 + 1), Err(SetPointOutOfBoundsError));
        assert_eq!(SetPoint::new(u16::MAX), Err(SetPointOutOfBoundsError));
    }

    #[test]
    fn fits_into_string() -> Result<(), SetPointOutOfBoundsError> {
        let set_point = SetPoint::new(12_345)?;
        assert_eq!(set_point.to_string().len(), 5);

        let set_point = SetPoint::new(MAX)?;
        assert_eq!(*set_point, 64_000);
        assert_eq!(set_point.to_string(), "64000");

        Ok(())
    }

    /// Every speed Home Assistant asks for arrives as the text of a MQTT payload and comes through
    /// here, including whatever a hand written payload contains
    #[test]
    fn parses_from_a_payload() {
        assert_eq!("0".parse(), Ok(SetPoint::ZERO));
        assert_eq!("19393".parse(), Ok(SetPoint::new(19_393).unwrap()));
        assert_eq!("64000".parse::<SetPoint>(), Ok(SetPoint::new(MAX).unwrap()));
    }

    #[test]
    fn rejects_a_payload_that_is_not_a_set_point() {
        assert_eq!(
            "64001".parse::<SetPoint>(),
            Err(ParseSetPointError::SettingOutOfBounds(
                SetPointOutOfBoundsError
            ))
        );
        // Above u16 rather than above the maximum set point, so it does not even parse as a number
        assert_eq!(
            "65536".parse::<SetPoint>(),
            Err(ParseSetPointError::ParseInt)
        );
        assert_eq!("".parse::<SetPoint>(), Err(ParseSetPointError::ParseInt));
        assert_eq!(
            "high".parse::<SetPoint>(),
            Err(ParseSetPointError::ParseInt)
        );
        assert_eq!("-1".parse::<SetPoint>(), Err(ParseSetPointError::ParseInt));
        assert_eq!("1.5".parse::<SetPoint>(), Err(ParseSetPointError::ParseInt));
    }
}
