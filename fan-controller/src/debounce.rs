use defmt::debug;
use embassy_rp::gpio::{Input, Pin};
use embassy_time::{Duration, TimeoutError, with_timeout};

/// Debouncer based on [Embassy debounce example](https://github.com/embassy-rs/embassy/blob/8d8cd78f634b2f435e3a997f7f8f3ac0b8ca300c/examples/rp/src/bin/debounce.rs)
/// (Licensed MIT/Apache-2.0)
/// and extended to wait for the level to hold instead of trusting a single edge
///
pub struct Debouncer<'a, T: Pin> {
    input: Input<'a, T>,
    hold: Duration,
}

impl<'a, T: Pin> Debouncer<'a, T> {
    pub fn new(input: Input<'a, T>, hold: Duration) -> Self {
        Self { input, hold }
    }

    /// Wait until the input is pulled low and *stays* low for the hold time.
    ///
    /// A falling edge on its own does not mean the button was pressed. Contact bounce chatters
    /// the line for a few milliseconds, and interference couples into the button wiring, which is
    /// a long piece of wire held high by nothing but the RP2040's weak internal pull-up. A
    /// fluorescent lamp starting on the same circuit is enough to produce an edge nobody caused.
    /// Both are over in far less time than a finger takes to let go, so the wait starts over
    /// unless the line stays low for the whole window.
    pub async fn debounce_falling_edge(&mut self) {
        loop {
            self.input.wait_for_falling_edge().await;

            // Waiting for the *level* rather than for a rising edge, so a spike that is already
            // over by the time we get here returns right away instead of counting as a press for
            // want of an edge that has been and gone.
            if let Err(TimeoutError) = with_timeout(self.hold, self.input.wait_for_high()).await {
                debug!("[Debounce] Accepted a falling edge that held");
                break;
            }

            debug!("[Debounce] Discarded a falling edge that did not hold");
        }
    }
}
