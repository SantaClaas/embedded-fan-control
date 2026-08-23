//! What an ebm-papst RadiCal fan reports about itself: how fast it is actually turning, how warm
//! it is, and what it is costing to run.
//!
//! The fan keeps these in input registers, which are read only. Their raw contents are not the
//! quantities they describe — a speed is relative to the maximum the fan is configured for, and
//! energy spans two registers — so decoding them has rules of its own. That is why this is its own
//! crate: `fan-controller` only builds for `thumbv6m-none-eabi`, which has no test harness, so
//! anything left in there is compiled by nothing and rots unnoticed. See the `set_point` crate,
//! which is here for the same reason.
//!
//! All register addresses and codings are from MODBUS Parameter RadiCal im Spiralgehäuse V1.00,
//! chapter 3.

#![no_std]

use core::fmt::Write;

/// The fan's configured maximum speed, which every speed it reports is relative to. A holding
/// register rather than an input register, and the only value here that has to be read separately.
/// See section 2.25
pub const MAXIMUM_SPEED_REGISTER: u16 = 0xD119;

/// Where the run of input registers holding the speed and the two temperatures starts, and how
/// many registers it spans. Modbus reads a range, so asking for `D010` through `D017` in one
/// request costs the same round trip as asking for any one of them. See section 3.1
pub const STATUS_START: u16 = 0xD010;
pub const STATUS_LENGTH: usize = 8;

/// Where the run holding the current power and the energy counter starts, and how many registers
/// it spans. A second request rather than one larger one, because everything between `D017` and
/// `D027` is either reserved or of no interest here
pub const ENERGY_START: u16 = 0xD027;
pub const ENERGY_LENGTH: usize = 4;

/// Offsets into the block starting at [`STATUS_START`]
mod status {
    /// `D010`, section 3.8
    pub(super) const ACTUAL_SPEED: usize = 0x0;
    /// `D016`, section 3.13
    pub(super) const MOTOR_TEMPERATURE: usize = 0x6;
    /// `D017`, section 3.14
    pub(super) const ELECTRONICS_TEMPERATURE: usize = 0x7;
}

/// Offsets into the block starting at [`ENERGY_START`]
mod energy {
    /// `D027`, section 3.20.2
    pub(super) const CURRENT_POWER: usize = 0x0;
    /// `D029`, the high half of the counter in section 3.22
    pub(super) const CONSUMPTION_HIGH: usize = 0x2;
    /// `D02A`, the low half of the counter in section 3.22
    pub(super) const CONSUMPTION_LOW: usize = 0x3;
}

/// One poll of a fan, decoded into the units the values actually describe
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Reading {
    /// Revolutions per minute, or `None` while the fan's configured maximum speed is not known.
    /// The reported speed is a fraction of that maximum, so without it the raw value cannot be
    /// turned into a rate at all
    pub speed: Option<u16>,
    /// Degrees celsius, and genuinely signed: a fan in an unheated loft reports below zero
    pub motor_temperature: i16,
    /// Degrees celsius, measured inside the electronics housing rather than in the air stream
    pub electronics_temperature: i16,
    /// Watts the fan is drawing right now
    pub power: u16,
    /// Kilowatt hours since the fan left the factory. Only ever counts up, short of a reset
    pub energy: u32,
}

/// Turns the two blocks of input registers into the quantities they describe.
///
/// `maximum_speed` is the contents of [`MAXIMUM_SPEED_REGISTER`], which only the speed needs. It
/// is separate because it is a holding register that changes only when the fan is reconfigured,
/// so it is read once rather than on every poll
pub fn decode(
    status: &[u16; STATUS_LENGTH],
    energy_block: &[u16; ENERGY_LENGTH],
    maximum_speed: Option<u16>,
) -> Reading {
    Reading {
        speed: maximum_speed.map(|maximum| speed(status[status::ACTUAL_SPEED], maximum)),
        motor_temperature: status[status::MOTOR_TEMPERATURE] as i16,
        electronics_temperature: status[status::ELECTRONICS_TEMPERATURE] as i16,
        power: energy_block[energy::CURRENT_POWER],
        energy: u32::from(energy_block[energy::CONSUMPTION_HIGH]) << 16
            | u32::from(energy_block[energy::CONSUMPTION_LOW]),
    }
}

/// The fan reports speed the same way it accepts one: as a fraction of [`set_point::MAX`], which
/// stands for the maximum speed the fan is configured for. See section 3.8.
///
/// The multiplication is done before the division so the rounding happens once, at the end, and it
/// is done in `u32` because the product does not fit in 16 bits. It cannot overflow `u32` either:
/// the fan caps what it reports at `1.02 * maximum` (`0xFF00`), and even the full `u16` range on
/// both sides stays under `u32::MAX`
fn speed(reported: u16, maximum: u16) -> u16 {
    let scaled = u32::from(reported) * u32::from(maximum) / u32::from(set_point::MAX);
    // Saturating rather than `as`, because a fan configured with a maximum near the top of `u16`
    // reports up to 1.02 times it, which no longer fits
    scaled.min(u32::from(u16::MAX)) as u16
}

/// Enough for every field at its longest, including the minus signs and a `null` speed. Proven by
/// `json_fits_the_worst_case`
pub const JSON_CAPACITY: usize = 128;

impl Reading {
    /// The payload Home Assistant reads, as one JSON object per fan so that all five values arrive
    /// in a single publish and each sensor picks its own out with a value template.
    ///
    /// An unknown speed is written as `null`, which Home Assistant renders as unknown. That is
    /// the honest answer while the maximum speed has not been read, and it keeps the four values
    /// that are known from being held back with it
    pub fn to_json(&self) -> heapless::String<JSON_CAPACITY> {
        let mut json = heapless::String::new();

        // Every write is into a buffer proven large enough by the test below, so the only way this
        // can fail is a change to the fields without a change to the capacity, which that test
        // catches
        let result = match self.speed {
            Some(speed) => write!(json, "{{\"speed\":{speed}"),
            None => write!(json, "{{\"speed\":null"),
        }
        .and_then(|()| {
            write!(
                json,
                ",\"motor_temperature\":{},\"electronics_temperature\":{},\"power\":{},\"energy\":{}}}",
                self.motor_temperature, self.electronics_temperature, self.power, self.energy
            )
        });

        debug_assert!(result.is_ok(), "the reading did not fit JSON_CAPACITY");
        let _ = result;

        json
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Half of the configured maximum in, half of it out
    #[test]
    fn speed_is_a_fraction_of_the_configured_maximum() {
        assert_eq!(speed(set_point::MAX / 2, 3_000), 1_500);
        assert_eq!(speed(set_point::MAX, 3_000), 3_000);
        assert_eq!(speed(0, 3_000), 0);
    }

    /// The fan caps what it reports at 1.02 times the maximum rather than letting it run over
    #[test]
    fn speed_handles_the_capped_reading() {
        assert_eq!(speed(0xFF00, 3_000), 3_060);
    }

    /// The product of the two overflows 16 bits long before either side does
    #[test]
    fn speed_does_not_overflow_on_a_large_maximum() {
        assert_eq!(speed(set_point::MAX, u16::MAX), u16::MAX);
        assert_eq!(speed(0xFF00, u16::MAX), u16::MAX);
    }

    /// Both temperatures are signed, which the raw register does not say
    #[test]
    fn temperatures_below_zero_stay_below_zero() {
        let status = [0, 0, 0, 0, 0, 0, 0xFFFB, 0x0015];
        let reading = decode(&status, &[0; ENERGY_LENGTH], None);

        assert_eq!(reading.motor_temperature, -5);
        assert_eq!(reading.electronics_temperature, 21);
    }

    /// The counter spans two registers, high half first
    #[test]
    fn energy_spans_both_registers() {
        let energy_block = [0, 0, 0x0001, 0x0002];
        let reading = decode(&[0; STATUS_LENGTH], &energy_block, None);

        assert_eq!(reading.energy, 65_538);
    }

    #[test]
    fn decodes_a_whole_poll() {
        // Speed at half of the range, motor at 42 °C, electronics at 38 °C
        let status = [set_point::MAX / 2, 0, 0, 0, 0, 0, 0x002A, 0x0026];
        // 25 W, and 1234 kWh since the factory
        let energy_block = [25, 0, 0, 1_234];

        let reading = decode(&status, &energy_block, Some(3_000));

        assert_eq!(
            reading,
            Reading {
                speed: Some(1_500),
                motor_temperature: 42,
                electronics_temperature: 38,
                power: 25,
                energy: 1_234,
            }
        );
    }

    #[test]
    fn serializes_to_json() {
        let reading = Reading {
            speed: Some(1_500),
            motor_temperature: 42,
            electronics_temperature: 38,
            power: 25,
            energy: 1_234,
        };

        assert_eq!(
            reading.to_json().as_str(),
            r#"{"speed":1500,"motor_temperature":42,"electronics_temperature":38,"power":25,"energy":1234}"#
        );
    }

    /// A speed that is not known yet must not hold back the four values that are
    #[test]
    fn serializes_an_unknown_speed_as_null() {
        let reading = Reading {
            speed: None,
            motor_temperature: -5,
            electronics_temperature: 38,
            power: 25,
            energy: 1_234,
        };

        assert_eq!(
            reading.to_json().as_str(),
            r#"{"speed":null,"motor_temperature":-5,"electronics_temperature":38,"power":25,"energy":1234}"#
        );
    }

    /// [`JSON_CAPACITY`] is asserted against rather than guessed at. `null` is shorter than the
    /// longest speed, so the widest object is the one with every number at its longest
    #[test]
    fn json_fits_the_worst_case() {
        let reading = Reading {
            speed: Some(u16::MAX),
            motor_temperature: i16::MIN,
            electronics_temperature: i16::MIN,
            power: u16::MAX,
            energy: u32::MAX,
        };

        let json = reading.to_json();

        // Would have been silently truncated rather than panicking in a release build
        assert!(json.ends_with('}'), "truncated at {} bytes: {json}", json.len());
        assert!(json.len() <= JSON_CAPACITY);
    }
}
