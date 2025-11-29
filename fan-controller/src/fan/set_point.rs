use core::{ops::Deref, str::FromStr};

use defmt::Format;

pub(crate) const MAX: u16 = 64_000;

/// Describes the desired speed of the fan from 0 to [`MAX_SET_POINT`]
#[derive(Debug, Format, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SetPoint(u16);

#[derive(Debug, Format)]
pub(crate) struct SetPointOutOfBoundsError;

impl SetPoint {
    pub(crate) const ZERO: Self = match Self::new(0) {
        Ok(setting) => setting,
        Err(error) => panic!("Invalid value. This should not be reachable."),
    };

    pub(crate) const MAX: Self = match Self::new(MAX) {
        Ok(setting) => setting,
        Err(error) => panic!("Invalid value. This should not be reachable."),
    };

    pub(crate) const MIN: Self = match Self::new(0) {
        Ok(setting) => setting,
        Err(error) => panic!("Invalid value. This should not be reachable."),
    };

    pub(crate) const fn new(set_point: u16) -> Result<Self, SetPointOutOfBoundsError> {
        if set_point > MAX {
            return Err(SetPointOutOfBoundsError);
        }

        Ok(Self(set_point))
    }

    const fn get(&self) -> u16 {
        self.0
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
    ParseInt(core::num::ParseIntError),
    SettingOutOfBounds(SetPointOutOfBoundsError),
}

impl FromStr for SetPoint {
    type Err = ParseSetPointError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let set_point = s.parse().map_err(ParseSetPointError::ParseInt)?;
        Self::new(set_point).map_err(ParseSetPointError::SettingOutOfBounds)
    }
}

#[cfg(test)]
mod tests {
    //! The tests don't run on the embedded target, so we need to import the std crate
    //TODO needs to be fixed to make run
    extern crate std;
    use crate::fan::{SetPoint, SetPointOutOfBoundsError};

    use super::*;

    /// These are important hardcoded values I want to make sure are not changed accidentally
    #[test]
    fn setting_does_not_exceed_max_set_point() {
        core::assert_eq!(fan::MAX_SET_POINT, 64_000);
        core::assert_eq!(SetPoint::new(64_000), Ok(SetPoint(64_000)));
        core::assert_eq!(SetPoint::new(64_000 + 1), Err(SetPointOutOfBoundsError));
        core::assert_eq!(SetPoint::new(u16::MAX), Err(SetPointOutOfBoundsError));
    }

    //TODO have not checked if test compiles
    #[test]
    fn fits_into_string() -> Result<(), SetPointOutOfBoundsError> {
        let set_point = SetPoint::new(12345)?;
        let string = set_point.to_string();
        core::assert_eq!(string.len(), 5);

        let set_point = SetPoint::MAX;
        core::assert_eq!(*set_point, 64_000);
        core::assert_eq!(set_point.to_string(), "64000");

        Ok(())
    }
}
