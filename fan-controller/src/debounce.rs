use embassy_rp::gpio::{Input, Level, Pin};
use embassy_time::{with_timeout, Duration, TimeoutError, Timer};

/// Debouncer based on [Embassy debounce example](https://github.com/embassy-rs/embassy/blob/8d8cd78f634b2f435e3a997f7f8f3ac0b8ca300c/examples/rp/src/bin/debounce.rs)
/// (Licensed MIT/Apache-2.0)
/// and extended with debounce for falling edge
///
pub struct Debouncer<'a, T: Pin> {
    input: Input<'a, T>,
    debounce: Duration,
}

impl<'a, T: Pin> Debouncer<'a, T> {
    pub fn new(input: Input<'a, T>, debounce: Duration) -> Self {
        Self { input, debounce }
    }

    pub async fn debounce(&mut self) -> Level {
        loop {
            // Up
            // 2nd round stil down
            let l1 = self.input.get_level();

            self.input.wait_for_any_edge().await;

            Timer::after(self.debounce).await;

            let l2 = self.input.get_level();
            if l1 != l2 {
                break l2;
            }
        }
    }

    pub async fn debounce_falling_edge(&mut self) {
        self.input.wait_for_falling_edge().await;
        loop {
            // Only after no falling edge was detected for the debounce time, return to the caller the latest result/level
            // which is the falling edge in this case which is why nothing/unit/"()" is returned
            // Restart the wait if there is a new falling edge
            if let Err(TimeoutError) =
                with_timeout(self.debounce, self.input.wait_for_falling_edge()).await
            {
                break;
            }
        }
    }
}
