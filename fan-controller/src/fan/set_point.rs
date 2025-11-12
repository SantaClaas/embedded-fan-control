use core::str::FromStr;

use defmt::Format;

pub(crate) const MAX: u16 = 64_000;

/// Describes the desired speed of the fan from 0 to [`MAX_SET_POINT`]
#[derive(Debug, Format, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct SetPoint(pub(crate) u16);

#[derive(Debug, Format)]
pub(crate) struct SetPointOutOfBoundsError;

impl SetPoint {
    pub(crate) const ZERO: Self = match Self::new(0) {
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
