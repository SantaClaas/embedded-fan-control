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
    /// `D011`, section 3.9
    pub(super) const MOTOR_STATUS: usize = 0x1;
    /// `D012`, section 3.10
    pub(super) const WARNING: usize = 0x2;
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

/// What the fan says is wrong with it, as the manual's own abbreviations.
///
/// Both registers are bit fields printed in section 3.9 and 3.10 as two rows of eight, most
/// significant row first. Only some bits carry a meaning; the manual prints the rest as `0`.
/// A bit set outside the documented ones is reported as raw hex rather than swallowed, because a
/// fan reporting something this does not know about is worth seeing rather than reading as healthy
fn write_flags(
    formatter: &mut core::fmt::Formatter<'_>,
    value: u16,
    flags: &[(u16, &'static str)],
    reported_elsewhere: u16,
) -> core::fmt::Result {
    // Bits reported as their own value are still documented ones, so they must not fall through to
    // the undocumented branch below and be printed as hex
    let mut documented = reported_elsewhere;
    let mut written = false;

    for (bit, name) in flags {
        documented |= 1 << bit;

        if value & (1 << bit) == 0 || reported_elsewhere & (1 << bit) != 0 {
            continue;
        }

        if written {
            formatter.write_str(", ")?;
        }
        formatter.write_str(name)?;
        written = true;
    }

    let undocumented = value & !documented;
    if undocumented != 0 {
        if written {
            formatter.write_str(", ")?;
        }
        write!(formatter, "{undocumented:#06X}")?;
        written = true;
    }

    if !written {
        formatter.write_str("OK")?;
    }

    Ok(())
}

/// `D011`, section 3.9. A set bit is a fault present on the fan right now
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotorStatus(pub u16);

impl MotorStatus {
    /// Section 3.9, most significant bit first. `FB` is set alongside whichever fault actually
    /// happened — the manual: "Fan Bad wird bei jedem Fehler gesetzt" — so it is a summary rather
    /// than a fault of its own
    const FLAGS: [(u16, &'static str); 5] = [
        (12, "UzLow"),
        (7, "BLK"),
        (5, "TFM"),
        (4, "FB"),
        (3, "SKF"),
    ];

    /// Whether the fan is reporting no fault at all. Any bit set means something, including one
    /// the manual does not document
    pub fn is_healthy(&self) -> bool {
        self.0 == 0
    }
}

impl core::fmt::Display for MotorStatus {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write_flags(formatter, self.0, &Self::FLAGS, 0)
    }
}

/// `D012`, section 3.10. The same shape as [`MotorStatus`], one step before the matching fault:
/// "der Grenzwert für die Fehlermeldung ist fast erreicht"
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Warning(pub u16);

impl Warning {
    /// Section 3.10, most significant bit first
    const FLAGS: [(u16, &'static str); 6] = [
        (12, "UzHigh"),
        (10, "Kabelbruch"),
        (9, "n_Low"),
        (6, "UzLow"),
        (5, "TEI_high"),
        (4, "TM_high"),
    ];

    /// Bit 10, `Kabelbruch am Analogeingang für den Sollwert`.
    ///
    /// Reported on its own rather than among the others because it is structurally always set
    /// here: the set point arrives over RS-485, so the analog input this watches is deliberately
    /// unwired and sits below the break threshold. Left in the warning string it would be a light
    /// that never goes out, which is the state a genuinely new warning is easiest to miss in
    const ANALOG_SET_POINT_BREAK: u16 = 1 << 10;

    /// Whether the analog set point input reads as broken. Always true on this controller, and
    /// kept anyway so that wiring something to that input later does not need new firmware to see
    pub fn is_analog_set_point_broken(&self) -> bool {
        self.0 & Self::ANALOG_SET_POINT_BREAK != 0
    }

    /// Whether the fan is reporting anything worth acting on.
    ///
    /// [`Self::ANALOG_SET_POINT_BREAK`] does not count, for the reason given there — it is a
    /// property of how this controller is wired rather than of how the fan is doing
    pub fn is_healthy(&self) -> bool {
        self.0 & !Self::ANALOG_SET_POINT_BREAK == 0
    }
}

impl core::fmt::Display for Warning {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write_flags(formatter, self.0, &Self::FLAGS, Self::ANALOG_SET_POINT_BREAK)
    }
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
    /// What the fan says is currently wrong with it, if anything
    pub motor_status: MotorStatus,
    /// What the fan says is close to going wrong
    pub warning: Warning,
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
        motor_status: MotorStatus(status[status::MOTOR_STATUS]),
        warning: Warning(status[status::WARNING]),
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

/// Watching a fan arrive at a new speed.
///
/// The fans do not step to a commanded speed, they travel to it, and how long that takes depends on
/// how far they are going — a nudge arrives in seconds where a change from one end of the range to
/// the other does not. So the poll that follows a speed change does not wait a fixed delay and hope:
/// it reads quickly and watches the flow until it stops moving.
///
/// The flow is the signal rather than the speed because it is reported in whole m³/h, so it is
/// naturally quieter than an rpm that wanders while the fan holds station — and it is the value
/// worth getting right.
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy)]
pub struct Settling {
    /// The flow the last successful reading carried, to compare the next one against
    previous: Option<u16>,
    /// How many consecutive readings have been within [`Settling::TOLERANCE`] of the one before
    steady: u8,
    /// How many of those mean the fan has arrived, derived from the polling cadence rather than
    /// fixed — see [`Settling::at_interval`]
    required: u8,
    /// How many readings this has taken, successful or not
    taken: u8,
    /// How many it may take before giving up, derived the same way
    limit: u8,
}

/// How many readings at `interval` fit into `span`, never fewer than one and never more than the
/// counters hold. A zero interval is not a cadence; treating it as one millisecond keeps this
/// total rather than making every caller handle an impossible case
fn readings_in(span_milliseconds: u32, interval_milliseconds: u32) -> u8 {
    let interval = interval_milliseconds.max(1);
    (span_milliseconds / interval).clamp(1, u8::MAX as u32) as u8
}

/// What one reading says about whether the fan has arrived
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Still on its way. Keep reading quickly
    Moving,
    /// Arrived. The reading just taken is the one worth keeping
    Settled,
    /// It has been given long enough and the flow is still not holding still. Stop reading quickly
    /// anyway — see [`Settling::MAX_READINGS`]
    GaveUp,
}

impl Settling {
    /// How close two consecutive readings have to be to count as the same flow.
    ///
    /// Not equality: the flow wanders a whole unit while the fan is holding station — 78, 78, 79
    /// was measured at rest — so waiting for two identical readings can wait forever
    pub const TOLERANCE: u16 = 1;

    /// How long the flow has to hold still before the fan counts as arrived.
    ///
    /// A duration rather than a number of readings, because a number of readings is a duration in
    /// disguise and a treacherous one: at a five second cadence "three in a row" means steady for
    /// fifteen seconds, at one second it means steady for three, so shortening the cadence to see
    /// more detail would quietly make the test weaker. A fan on its final approach changes by less
    /// than the tolerance from one second to the next while it is still very much moving
    pub const STEADY_FOR_MILLISECONDS: u32 = 15_000;

    /// How long to keep reading quickly before giving up and going back to the ordinary interval.
    ///
    /// This is the important one. A fan that has stopped answering fails a poll by *timing out*,
    /// and it holds the modbus mutex while it does — so without a bound, one silent fan would time
    /// out at the settling cadence indefinitely with every speed change queued behind it. That is
    /// the failure the polling routine was written to avoid, so the fast cadence has to end whether
    /// or not anything ever settles
    pub const GIVE_UP_AFTER_MILLISECONDS: u32 = 120_000;

    /// Builds a watcher for the cadence it is going to be polled at.
    ///
    /// Both counts come from the durations above, so changing the cadence changes how finely the
    /// flow is sampled and *not* what counts as settled. At five seconds this works out as the
    /// three readings and twenty-four cap it was originally written with
    pub fn at_interval(interval_milliseconds: u32) -> Self {
        Self {
            previous: None,
            steady: 0,
            required: readings_in(Self::STEADY_FOR_MILLISECONDS, interval_milliseconds),
            taken: 0,
            limit: readings_in(Self::GIVE_UP_AFTER_MILLISECONDS, interval_milliseconds),
        }
    }

    /// How many readings this has taken, for reporting when it gives up
    pub fn readings_taken(&self) -> u8 {
        self.taken
    }

    /// Takes one reading's flow, or `None` when that poll failed, and says whether to keep going.
    ///
    /// A failed poll says nothing about the flow, so it cannot count towards being steady — but it
    /// does count towards [`Self::MAX_READINGS`], because the cap is there to bound exactly that
    pub fn observe(&mut self, volume_flow: Option<u16>) -> Progress {
        self.taken = self.taken.saturating_add(1);

        match volume_flow {
            Some(flow) => {
                match self.previous {
                    // Nothing to compare the first reading against
                    Some(previous) if flow.abs_diff(previous) <= Self::TOLERANCE => {
                        self.steady = self.steady.saturating_add(1);
                    }
                    Some(_) => self.steady = 0,
                    None => {}
                }

                self.previous = Some(flow);
            }
            // The last known flow is kept to compare the next successful reading against, but the
            // run of steady ones is broken: what happened in between is unknown
            None => self.steady = 0,
        }

        if self.steady >= self.required {
            Progress::Settled
        } else if self.taken >= self.limit {
            Progress::GaveUp
        } else {
            Progress::Moving
        }
    }
}

/// Enough for every field at its longest, including the minus signs and a `null` speed. Proven by
/// `json_fits_the_worst_case`
pub const JSON_CAPACITY: usize = 384;

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
            // The flag names are the manual's own abbreviations, which contain nothing JSON has to
            // escape
            write!(
                json,
                ",\"motor_status\":\"{}\",\"warning\":\"{}\",\"analog_set_point\":\"{}\"",
                self.motor_status,
                self.warning,
                if self.warning.is_analog_set_point_broken() {
                    "Kabelbruch"
                } else {
                    "OK"
                }
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
    fn format_flags(flags: impl core::fmt::Display) -> heapless::String<64> {
        let mut buffer = heapless::String::new();
        write!(buffer, "{flags}").unwrap();
        buffer
    }

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

    /// The manual prints section 3.9 as two rows of eight, most significant row first, so the
    /// flags are checked against whole 16 bit words rather than against the bit numbers this
    /// transcribed them into — a transcription error would survive the latter
    #[test]
    fn motor_status_bits_are_where_the_manual_puts_them() {
        assert_eq!(format_flags(MotorStatus(0b0001_0000_0000_0000)), "UzLow");
        assert_eq!(format_flags(MotorStatus(0b0000_0000_1000_0000)), "BLK");
        assert_eq!(format_flags(MotorStatus(0b0000_0000_0010_0000)), "TFM");
        assert_eq!(format_flags(MotorStatus(0b0000_0000_0001_0000)), "FB");
        assert_eq!(format_flags(MotorStatus(0b0000_0000_0000_1000)), "SKF");
    }

    /// Section 3.10, checked the same way. `Kabelbruch` is bit 10 and is checked separately,
    /// because it is reported as its own value rather than in this string
    #[test]
    fn warning_bits_are_where_the_manual_puts_them() {
        assert_eq!(format_flags(Warning(0b0001_0000_0000_0000)), "UzHigh");
        assert!(Warning(0b0000_0100_0000_0000).is_analog_set_point_broken());
        assert_eq!(format_flags(Warning(0b0000_0010_0000_0000)), "n_Low");
        assert_eq!(format_flags(Warning(0b0000_0000_0100_0000)), "UzLow");
        assert_eq!(format_flags(Warning(0b0000_0000_0010_0000)), "TEI_high");
        assert_eq!(format_flags(Warning(0b0000_0000_0001_0000)), "TM_high");
    }

    /// The analog set point input is unwired on this controller, so both fans set that bit on
    /// every poll. It has to stay out of the warning string and out of `is_healthy`, or the
    /// warning entity is a light that never goes out — but it is still documented, so it must not
    /// come out as hex either
    #[test]
    fn the_analog_set_point_break_is_reported_on_its_own() {
        let only_the_break = Warning(0b0000_0100_0000_0000);

        assert!(only_the_break.is_analog_set_point_broken());
        assert!(only_the_break.is_healthy());
        assert_eq!(format_flags(only_the_break), "OK");
    }

    /// A real warning alongside it still has to show, and show alone
    #[test]
    fn a_real_warning_shows_past_the_analog_set_point_break() {
        let both = Warning(0b0000_0100_0001_0000);

        assert!(both.is_analog_set_point_broken());
        assert!(!both.is_healthy());
        assert_eq!(format_flags(both), "TM_high");
    }

    /// A healthy fan is the common case and has to read as such rather than as an empty string
    #[test]
    fn nothing_set_reads_as_healthy() {
        assert!(MotorStatus(0).is_healthy());
        assert!(Warning(0).is_healthy());
        assert_eq!(format_flags(MotorStatus(0)), "OK");
        assert_eq!(format_flags(Warning(0)), "OK");
    }

    /// `FB` accompanies whichever fault actually happened, so the common real reading is two bits,
    /// listed most significant first the way the manual prints them
    #[test]
    fn several_faults_are_listed_together() {
        assert_eq!(format_flags(MotorStatus(0b0000_0000_1001_0000)), "BLK, FB");
        assert_eq!(
            format_flags(MotorStatus(0b0001_0000_0011_1000)),
            "UzLow, TFM, FB, SKF"
        );
    }

    /// The manual prints the remaining bits as `0`, so a fan setting one is saying something this
    /// does not understand. Reading that as healthy would be the worst of the available answers
    #[test]
    fn an_undocumented_bit_is_reported_rather_than_swallowed() {
        assert!(!MotorStatus(0b0000_0000_0000_0001).is_healthy());
        assert_eq!(format_flags(MotorStatus(0b0000_0000_0000_0001)), "0x0001");
        assert_eq!(format_flags(MotorStatus(0b0000_0000_1000_0001)), "BLK, 0x0001");
    }

    /// Both registers are read as part of the status run rather than asked for separately
    #[test]
    fn the_status_and_warning_come_out_of_the_status_run() {
        let mut status = [0; STATUS_LENGTH];
        status[status::MOTOR_STATUS] = 0b0000_0000_1001_0000;
        status[status::WARNING] = 0b0000_0000_0001_0000;

        let reading = decode(&status, &[0; POWER_AND_AIR_LENGTH], None);

        assert_eq!(reading.motor_status, MotorStatus(0b0000_0000_1001_0000));
        assert_eq!(reading.warning, Warning(0b0000_0000_0001_0000));
    }

    /// Feeds a run of flows and returns what each one said, so a whole ramp reads as one line
    /// Five seconds is the cadence these were first written against, and the one the derived
    /// counts are checked to reproduce
    const FIVE_SECONDS: u32 = 5_000;

    fn observe_all(
        interval_milliseconds: u32,
        flows: impl IntoIterator<Item = Option<u16>>,
    ) -> heapless::Vec<Progress, 160> {
        let mut settling = Settling::at_interval(interval_milliseconds);
        flows
            .into_iter()
            .map(|flow| settling.observe(flow))
            .collect()
    }

    /// The shape actually measured on 2026-09-06: commanded 78 m³/h, still reading 82 and 84
    /// sixteen seconds in, settled by forty-six. Three steady readings end it, and it takes four
    /// readings to get three comparisons
    #[test]
    fn a_ramp_settles_once_the_flow_stops_moving() {
        let progress = observe_all(FIVE_SECONDS, [180, 120, 95, 84, 80, 78, 78, 79, 78].map(Some));

        assert_eq!(
            progress.as_slice(),
            [
                // Nothing to compare the first against
                Progress::Moving,
                // Every step down is wider than the tolerance, including the last one: 80 to 78 is
                // two, so arriving at the target is not by itself evidence of having stopped
                Progress::Moving,
                Progress::Moving,
                Progress::Moving,
                Progress::Moving,
                Progress::Moving,
                // 78, 79, 78: three consecutive comparisons within the tolerance
                Progress::Moving,
                Progress::Moving,
                Progress::Settled,
            ]
        );
    }

    /// Nine readings at the firmware's settling cadence is roughly the forty seconds that ramp took
    /// on the bench, which is the sanity check that the tolerance and the required run are not so
    /// strict that the cap is reached first
    #[test]
    fn a_measured_ramp_settles_well_inside_the_cap() {
        let progress = observe_all(FIVE_SECONDS, [180, 120, 95, 84, 80, 78, 78, 79, 78].map(Some));

        assert_eq!(*progress.last().unwrap(), Progress::Settled);
        assert!(progress.len() < usize::from(Settling::at_interval(FIVE_SECONDS).limit));
    }

    /// The flow wanders a unit while the fan holds station, so equality would never arrive. That
    /// is the whole reason there is a tolerance
    #[test]
    fn a_wandering_flow_still_counts_as_settled() {
        let progress = observe_all(FIVE_SECONDS, [78, 79, 78, 79].map(Some));

        assert_eq!(*progress.last().unwrap(), Progress::Settled);
    }

    /// A fan that never holds still must not hold the fast cadence forever
    #[test]
    fn a_flow_that_never_settles_gives_up() {
        // Alternating far enough apart that no two consecutive readings are ever within tolerance
        let cap = Settling::at_interval(FIVE_SECONDS).limit;
        let flows = (0..cap).map(|reading| Some(u16::from(reading % 2) * 50));
        let progress = observe_all(FIVE_SECONDS, flows);

        assert_eq!(progress.len(), usize::from(cap));
        assert!(
            progress[..progress.len() - 1]
                .iter()
                .all(|step| *step == Progress::Moving)
        );
        assert_eq!(*progress.last().unwrap(), Progress::GaveUp);
    }

    /// The cap exists for the silent fan above all: a failed poll is a timeout holding the modbus
    /// mutex, so failures have to count towards it even though they say nothing about the flow
    #[test]
    fn failed_polls_count_towards_giving_up() {
        let cap = Settling::at_interval(FIVE_SECONDS).limit;
        let progress = observe_all(FIVE_SECONDS, core::iter::repeat_n(None, cap.into()));

        assert_eq!(*progress.last().unwrap(), Progress::GaveUp);
    }

    /// A failed poll in the middle breaks the run rather than being skipped over: what the flow did
    /// while nothing was heard is unknown, so the readings either side of it are not consecutive
    #[test]
    fn a_failed_poll_breaks_the_run_of_steady_readings() {
        let progress = observe_all(
            FIVE_SECONDS,
            [Some(78), Some(78), Some(78), None, Some(78), Some(78)],
        );

        assert_eq!(
            progress.as_slice(),
            [
                Progress::Moving,
                Progress::Moving,
                // Two steady comparisons, one short
                Progress::Moving,
                Progress::Moving,
                Progress::Moving,
                Progress::Moving,
            ]
        );
    }

    /// The counts are derived so that what counts as settled is a duration, not a sample count.
    /// Five seconds has to reproduce the three-in-a-row and twenty-four cap this started with
    #[test]
    fn the_counts_come_from_the_cadence() {
        let five = Settling::at_interval(FIVE_SECONDS);
        assert_eq!(five.required, 3);
        assert_eq!(five.limit, 24);

        // Twice as often, so twice as many readings for the same fifteen and hundred-and-twenty
        // seconds — 15 / 2 truncates to seven, which is seven intervals of steadiness, not six
        let two = Settling::at_interval(2_000);
        assert_eq!(two.required, 7);
        assert_eq!(two.limit, 60);

        let one = Settling::at_interval(1_000);
        assert_eq!(one.required, 15);
        assert_eq!(one.limit, 120);
    }

    /// The reason the counts are derived at all.
    ///
    /// This is a fan on its final approach sampled every second: each reading is within the
    /// tolerance of the one before, because a whole m³/h takes several seconds to lose, and yet
    /// the fan is still moving. With a fixed three-in-a-row this would call it settled on the
    /// third reading — the mistake that shortening the cadence would otherwise have introduced
    #[test]
    fn a_slow_approach_does_not_read_as_settled_just_because_it_is_sampled_quickly() {
        let creeping = [95, 95, 94, 94, 94, 93, 93].map(Some);

        let progress = observe_all(1_000, creeping);

        assert!(
            progress.iter().all(|step| *step == Progress::Moving),
            "seven seconds of a slow approach must not count as fifteen seconds of holding still"
        );
    }

    /// The same flows at the cadence they were sized for do settle, so the rule above is a
    /// duration rather than simply a stricter count
    #[test]
    fn the_same_approach_settles_when_it_really_has_held_still_for_long_enough() {
        // Fifteen seconds of it at one second apiece, after the first reading to compare against
        let holding = core::iter::repeat_n(Some(93), 16);

        let progress = observe_all(1_000, holding);

        assert_eq!(*progress.last().unwrap(), Progress::Settled);
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
                motor_status: MotorStatus(0),
                warning: Warning(0),
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
            motor_status: MotorStatus(0),
            warning: Warning(0),
            motor_temperature: 42,
            electronics_temperature: 38,
            power: 25,
            air_temperature: Tenths(215),
            air_humidity: Tenths(473),
            volume_flow: 120,
        };

        assert_eq!(
            reading.to_json().as_str(),
            r#"{"speed":1500,"motor_temperature":42,"electronics_temperature":38,"power":25,"motor_status":"OK","warning":"OK","analog_set_point":"OK","air_temperature":21.5,"air_humidity":47.3,"volume_flow":120}"#
        );
    }

    /// A speed that is not known yet must not hold back the values that are
    #[test]
    fn serializes_an_unknown_speed_as_null() {
        let reading = Reading {
            speed: None,
            motor_status: MotorStatus(1 << 7 | 1 << 4),
            warning: Warning(1 << 10 | 1 << 4),
            motor_temperature: -5,
            electronics_temperature: 38,
            power: 25,
            air_temperature: Tenths(-3),
            air_humidity: Tenths(802),
            volume_flow: 0,
        };

        assert_eq!(
            reading.to_json().as_str(),
            r#"{"speed":null,"motor_temperature":-5,"electronics_temperature":38,"power":25,"motor_status":"BLK, FB","warning":"TM_high","analog_set_point":"Kabelbruch","air_temperature":-0.3,"air_humidity":80.2,"volume_flow":0}"#
        );
    }

    /// [`JSON_CAPACITY`] is asserted against rather than guessed at. `null` is shorter than the
    /// longest speed, so the widest object is the one with every number at its longest
    #[test]
    fn json_fits_the_worst_case() {
        let reading = Reading {
            speed: Some(u16::MAX),
            motor_status: MotorStatus(u16::MAX),
            warning: Warning(u16::MAX),
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
