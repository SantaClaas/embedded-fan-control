//! Why the RP2040 last came up.
//!
//! A fan controller that reboots on its own looks identical from the outside to one that was
//! power cycled, and the two want completely different fixes. The chip records the answer across
//! the reset, so read it before anything else has a chance to overwrite it.

use embassy_rp::pac;

#[derive(Clone, Copy, defmt::Format)]
pub enum ResetCause {
    /// The supply dropped below the brown-out threshold, or power was applied. A controller that
    /// reports this without anyone touching the plug browned out.
    PowerOnOrBrownOut,
    /// The RUN pin was pulled low, which is the reset button and the probe's reset line.
    RunPin,
    /// The watchdog was not fed in time.
    WatchdogTimeout,
    /// Something asked for the reset deliberately — a debugger, or `probe-rs run`.
    Forced,
    /// The power-on state machine restarted without any of the above, which is what a debugger
    /// attaching usually looks like.
    DebuggerRestart,
    /// None of the bits were set. Documented as the plain hardware reset case.
    Unknown,
}

impl ResetCause {
    /// The string published to MQTT. Kept short and stable so it can be grepped and compared
    /// across boots.
    pub fn as_str(self) -> &'static str {
        match self {
            ResetCause::PowerOnOrBrownOut => "power-on-or-brown-out",
            ResetCause::RunPin => "run-pin",
            ResetCause::WatchdogTimeout => "watchdog-timeout",
            ResetCause::Forced => "forced",
            ResetCause::DebuggerRestart => "debugger-restart",
            ResetCause::Unknown => "unknown",
        }
    }
}

/// Read why the chip last reset.
///
/// The watchdog reason is checked first: a watchdog reset runs through the same power-on state
/// machine, so `CHIP_RESET` alone would report it as a restart and hide the real cause.
pub fn read() -> ResetCause {
    let watchdog_reason = pac::WATCHDOG.reason().read();
    if watchdog_reason.timer() {
        return ResetCause::WatchdogTimeout;
    }
    if watchdog_reason.force() {
        return ResetCause::Forced;
    }

    let chip_reset = pac::VREG_AND_CHIP_RESET.chip_reset().read();
    if chip_reset.had_por() {
        ResetCause::PowerOnOrBrownOut
    } else if chip_reset.had_run() {
        ResetCause::RunPin
    } else if chip_reset.had_psm_restart() {
        ResetCause::DebuggerRestart
    } else {
        ResetCause::Unknown
    }
}
