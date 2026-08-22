use core::{ops::Deref, str::FromStr};

use defmt::Format;

pub(crate) const MAX: u16 = 64_000;

/// Describes the desired speed of the fan from 0 to [`MAX_SET_POINT`]
#[derive(Debug, Format, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SetPoint(u16);

#[derive(Debug, Format, PartialEq, Eq)]
pub(crate) struct SetPointOutOfBoundsError;

impl SetPoint {
    pub(crate) const ZERO: Self = match Self::new(0) {
        Ok(setting) => setting,
        Err(_error) => panic!("Invalid value. This should not be reachable."),
    };

    pub(crate) const fn new(set_point: u16) -> Result<Self, SetPointOutOfBoundsError> {
        if set_point > MAX {
            return Err(SetPointOutOfBoundsError);
        }

        Ok(Self(set_point))
    }

    /// This should always succeed
    pub(crate) fn to_string(&self) -> heapless::String<5> {
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

pub(crate) enum ParseSetPointError {
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
    //! These tests cannot run in this crate: it only builds for `thumbv6m-none-eabi`, which has no
    //! test harness, and it cannot build for the host because `cortex-m` uses ARM inline assembly.
    //TODO move `SetPoint` into a host-testable crate so these actually run
    extern crate std;

    use super::*;

    /// These are important hardcoded values I want to make sure are not changed accidentally
    #[test]
    fn setting_does_not_exceed_max_set_point() {
        core::assert_eq!(MAX, 64_000);
        core::assert_eq!(SetPoint::new(64_000), Ok(SetPoint(64_000)));
        core::assert_eq!(SetPoint::new(64_000 + 1), Err(SetPointOutOfBoundsError));
        core::assert_eq!(SetPoint::new(u16::MAX), Err(SetPointOutOfBoundsError));
    }

    #[test]
    fn fits_into_string() -> Result<(), SetPointOutOfBoundsError> {
        let set_point = SetPoint::new(12_345)?;
        core::assert_eq!(set_point.to_string().len(), 5);

        let set_point = SetPoint::new(MAX)?;
        core::assert_eq!(*set_point, 64_000);
        core::assert_eq!(set_point.to_string(), "64000");

        Ok(())
    }
}
