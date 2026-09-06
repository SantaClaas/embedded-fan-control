//! What an ebm-papst RadiCal fan reports about itself: how fast it is actually turning, how warm
//! it is, what it is costing to run, and — from the sensor wired to it — the air it is moving.
//!
//! The fan keeps these in input registers, which are read only. Their raw contents are not the
//! quantities they describe — a speed is relative to the maximum the fan is configured for, a
//! temperature is signed, a humidity is a fraction of `65536` — so decoding them has rules of its
//! own. That is why this is its own crate: `fan-controller` only builds for `thumbv6m-none-eabi`,
//! which has no test harness, so anything left in there is compiled by nothing and rots unnoticed.
//! See the `set_point` crate, which is here for the same reason.
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

/// Where the second run starts, and how many registers it spans: what the fan costs to run
/// (`D027`) through what it is moving (`D033`). Everything between `D017` and `D027` is either
/// reserved or of no interest here, which is why this is a second request rather than one larger
/// one — but everything from `D027` to `D033` is one round trip either way, so it is asked for as
/// a single range even though only three of the thirteen registers past the power are read.
///
/// `D02B` to `D02D` are not in the manual's register table at all, not even as reserved. Asking
/// across them is safe for the same reason the status run is: it spans `D015`, which the table
/// does not list either, and the fans have answered that range for as long as this has run
pub const POWER_AND_AIR_START: u16 = 0xD027;
pub const POWER_AND_AIR_LENGTH: usize = 13;

/// Offsets into the block starting at [`STATUS_START`]
mod status {
    /// `D010`, section 3.8
    pub(super) const ACTUAL_SPEED: usize = 0x0;
    /// `D016`, section 3.13
    pub(super) const MOTOR_TEMPERATURE: usize = 0x6;
    /// `D017`, section 3.14
    pub(super) const ELECTRONICS_TEMPERATURE: usize = 0x7;
}

/// Offsets into the block starting at [`POWER_AND_AIR_START`]
mod power_and_air {
    /// `D027`, section 3.20.2
    pub(super) const CURRENT_POWER: usize = 0x0;
    /// `D02E`, section 3.17.3. Sensor 1 rather than sensor 2, because sensor 1 is the one the fan
    /// itself uses for its mass flow calculation
    pub(super) const AIR_TEMPERATURE: usize = 0x7;
    /// `D02F`, section 3.17.3
    pub(super) const AIR_HUMIDITY: usize = 0x8;
    /// `D033`, section 3.17.5
    pub(super) const VOLUME_FLOW: usize = 0xC;
}

/// A quantity the fan reports in tenths of its unit, carried as those tenths rather than as a
/// float. The RP2040 has no floating point unit, and a tenth is exactly what the sensor resolves,
/// so nothing is gained by converting and a rounding step is avoided
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Tenths(pub i16);

impl core::fmt::Display for Tenths {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        // A tenth below zero has a whole part of zero, so `-0.5` loses its sign if the division is
        // left to carry it. Writing the sign separately and dividing the magnitude keeps it
        let sign = if self.0.is_negative() { "-" } else { "" };
        let magnitude = self.0.unsigned_abs();
        write!(formatter, "{sign}{}.{}", magnitude / 10, magnitude % 10)
    }
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
    /// Degrees celsius of the air itself, from the temperature/humidity sensor wired to the fan,
    /// and to a tenth rather than whole degrees like the fan's own two
    pub air_temperature: Tenths,
    /// Relative humidity of the air in percent, from the same sensor
    pub air_humidity: Tenths,
    /// Cubic metres an hour the fan is moving
    pub volume_flow: u16,
}

/// Turns the two blocks of input registers into the quantities they describe.
///
/// `maximum_speed` is the contents of [`MAXIMUM_SPEED_REGISTER`], which only the speed needs. It
/// is separate because it is a holding register that changes only when the fan is reconfigured,
/// so it is read once rather than on every poll
pub fn decode(
    status: &[u16; STATUS_LENGTH],
    power_and_air_block: &[u16; POWER_AND_AIR_LENGTH],
    maximum_speed: Option<u16>,
) -> Reading {
    Reading {
        speed: maximum_speed.map(|maximum| speed(status[status::ACTUAL_SPEED], maximum)),
        motor_temperature: status[status::MOTOR_TEMPERATURE] as i16,
        electronics_temperature: status[status::ELECTRONICS_TEMPERATURE] as i16,
        power: power_and_air_block[power_and_air::CURRENT_POWER],
        air_temperature: Tenths(power_and_air_block[power_and_air::AIR_TEMPERATURE] as i16),
        air_humidity: relative_humidity(power_and_air_block[power_and_air::AIR_HUMIDITY]),
        volume_flow: power_and_air_block[power_and_air::VOLUME_FLOW],
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

/// Section 3.17.3: `φ [%] = Datenbytes / 65536 · 100 %`. Scaled to tenths of a percent, which is
/// as far as the sensor resolves, and computed in `u32` because the numerator does not fit in 16
/// bits. The largest raw value the register can hold gives `999`, so the result always fits `i16`
fn relative_humidity(raw: u16) -> Tenths {
    Tenths((u32::from(raw) * 1_000 / 65_536) as i16)
}

/// Enough for every field at its longest, including the minus signs and a `null` speed. Proven by
/// `json_fits_the_worst_case`
pub const JSON_CAPACITY: usize = 192;

impl Reading {
    /// The payload Home Assistant reads, as one JSON object per fan so that every value arrives in
    /// a single publish and each sensor picks its own out with a value template.
    ///
    /// An unknown speed is written as `null`, which Home Assistant renders as unknown. That is
    /// the honest answer while the maximum speed has not been read, and it keeps the values that
    /// are known from being held back with it
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
                ",\"motor_temperature\":{},\"electronics_temperature\":{},\"power\":{}",
                self.motor_temperature, self.electronics_temperature, self.power
            )
        })
        .and_then(|()| {
            write!(
                json,
                ",\"air_temperature\":{},\"air_humidity\":{},\"volume_flow\":{}}}",
                self.air_temperature, self.air_humidity, self.volume_flow
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

    /// The crate is `no_std`, so `ToString` is not available. Formatting into a small heapless
    /// buffer exercises the same `Display` impl the JSON payload goes through
    fn format(tenths: Tenths) -> heapless::String<16> {
        let mut buffer = heapless::String::new();
        write!(buffer, "{tenths}").unwrap();
        buffer
    }

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

    /// Section 3.17.3 gives the full scale as `65536`, not as `65535`, so the top of the register
    /// is just short of a hundred percent rather than exactly it
    #[test]
    fn humidity_is_a_fraction_of_the_full_scale() {
        assert_eq!(relative_humidity(0), Tenths(0));
        assert_eq!(relative_humidity(32_768), Tenths(500));
        assert_eq!(relative_humidity(u16::MAX), Tenths(999));
    }

    /// The sign belongs to the whole quantity, not to the whole part, which is zero for the first
    /// degree below freezing
    #[test]
    fn tenths_keep_the_sign_of_a_value_under_one() {
        assert_eq!(format(Tenths(-5)), "-0.5");
        assert_eq!(format(Tenths(5)), "0.5");
        assert_eq!(format(Tenths(0)), "0.0");
        assert_eq!(format(Tenths(-215)), "-21.5");
        assert_eq!(format(Tenths(215)), "21.5");
    }

    /// `i16::MIN` has no positive counterpart, so negating it before dividing would overflow
    #[test]
    fn tenths_format_the_bottom_of_the_range() {
        assert_eq!(format(Tenths(i16::MIN)), "-3276.8");
    }

    /// Both temperatures are signed, which the raw register does not say
    #[test]
    fn temperatures_below_zero_stay_below_zero() {
        let status = [0, 0, 0, 0, 0, 0, 0xFFFB, 0x0015];
        let reading = decode(&status, &[0; POWER_AND_AIR_LENGTH], None);

        assert_eq!(reading.motor_temperature, -5);
        assert_eq!(reading.electronics_temperature, 21);
    }

    /// The air temperature is signed too, and in tenths rather than whole degrees
    #[test]
    fn the_air_temperature_is_signed_tenths() {
        let mut block = [0; POWER_AND_AIR_LENGTH];
        // −2.7 °C
        block[power_and_air::AIR_TEMPERATURE] = 0xFFE5;

        let reading = decode(&[0; STATUS_LENGTH], &block, None);

        assert_eq!(reading.air_temperature, Tenths(-27));
    }

    #[test]
    fn decodes_a_whole_poll() {
        // Speed at half of the range, motor at 42 °C, electronics at 38 °C
        let status = [set_point::MAX / 2, 0, 0, 0, 0, 0, 0x002A, 0x0026];
        let mut power_and_air_block = [0; POWER_AND_AIR_LENGTH];
        // 25 W
        power_and_air_block[power_and_air::CURRENT_POWER] = 25;
        // 21.5 °C
        power_and_air_block[power_and_air::AIR_TEMPERATURE] = 215;
        // Half of the full scale, which is just under 50 %
        power_and_air_block[power_and_air::AIR_HUMIDITY] = 32_768;
        // 120 m³/h
        power_and_air_block[power_and_air::VOLUME_FLOW] = 120;

        let reading = decode(&status, &power_and_air_block, Some(3_000));

        assert_eq!(
            reading,
            Reading {
                speed: Some(1_500),
                motor_temperature: 42,
                electronics_temperature: 38,
                power: 25,
                air_temperature: Tenths(215),
                air_humidity: Tenths(500),
                volume_flow: 120,
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
            air_temperature: Tenths(215),
            air_humidity: Tenths(473),
            volume_flow: 120,
        };

        assert_eq!(
            reading.to_json().as_str(),
            r#"{"speed":1500,"motor_temperature":42,"electronics_temperature":38,"power":25,"air_temperature":21.5,"air_humidity":47.3,"volume_flow":120}"#
        );
    }

    /// A speed that is not known yet must not hold back the values that are
    #[test]
    fn serializes_an_unknown_speed_as_null() {
        let reading = Reading {
            speed: None,
            motor_temperature: -5,
            electronics_temperature: 38,
            power: 25,
            air_temperature: Tenths(-3),
            air_humidity: Tenths(802),
            volume_flow: 0,
        };

        assert_eq!(
            reading.to_json().as_str(),
            r#"{"speed":null,"motor_temperature":-5,"electronics_temperature":38,"power":25,"air_temperature":-0.3,"air_humidity":80.2,"volume_flow":0}"#
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
            air_temperature: Tenths(i16::MIN),
            air_humidity: Tenths(i16::MIN),
            volume_flow: u16::MAX,
        };

        let json = reading.to_json();

        // Would have been silently truncated rather than panicking in a release build
        assert!(json.ends_with('}'), "truncated at {} bytes: {json}", json.len());
        assert!(json.len() <= JSON_CAPACITY);
    }
}
