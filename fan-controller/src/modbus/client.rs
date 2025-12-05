use defmt::{error, info};
use embassy_rp::{
    Peripheral,
    gpio::{Level, Output, Pin},
    interrupt::typelevel::Binding,
    uart::{self, BufferedInterruptHandler, BufferedUart, RxPin, TxPin},
};
use embassy_time::{Duration, TimeoutError, block_for, with_timeout};
use embedded_io_async::{Read, Write};

use crate::{configuration, modbus::function::WriteHoldingRegister};

pub(crate) enum Error {
    Timeout(TimeoutError),
    Uart,
}

impl From<uart::Error> for Error {
    fn from(_value: uart::Error) -> Self {
        Self::Uart
    }
}

impl From<TimeoutError> for Error {
    fn from(error: TimeoutError) -> Self {
        Self::Timeout(error)
    }
}

const BLOCK_FOR: Duration = Duration::from_micros(5_000);

/// Modbus messages are sent through UART to MAX845 to control fans.
/// The pin is used to enable the DE pin to switch between reading and writing
pub(crate) struct Client<'a, UART: uart::Instance, PIN: Pin> {
    uart: BufferedUart<'a, UART>,
    driver_enable: Output<'a, PIN>,
}

impl<'a, UART: uart::Instance, PIN: Pin> Client<'a, UART, PIN> {
    pub(crate) fn new(
        uart: impl Peripheral<P = UART> + 'a,
        tx: impl Peripheral<P = impl TxPin<UART>> + 'a,
        rx: impl Peripheral<P = impl RxPin<UART>> + 'a,
        irq: impl Binding<UART::Interrupt, BufferedInterruptHandler<UART>>,
        driver_enable: impl Peripheral<P = PIN> + 'a,
        tx_buffer: &'a mut [u8],
        rx_buffer: &'a mut [u8],
        configuration: uart::Config,
    ) -> Self {
        let uart = BufferedUart::new(uart, irq, tx, rx, tx_buffer, rx_buffer, configuration);
        let driver_enable = Output::new(driver_enable, Level::Low);

        Self {
            uart,
            driver_enable,
        }
    }

    pub(crate) async fn send_3(&mut self, message: &WriteHoldingRegister) -> Result<(), Error> {
        // For debugging
        let fan_identifier = match *message.device_address() {
            2 => "[Fan 1]",
            3 => "[Fan 2]",
            _other => "Unknown (oops)",
        };

        // Write then read
        // Set pin setting DE (driver enable) to on (high) on the MAX845 to send data
        self.driver_enable.set_high();

        let bytes = message.as_ref();
        info!("{} Sending message to fan: {:?}", fan_identifier, bytes);
        // As ref because &[u8; 8] is not the same as &[u8]
        let result = with_timeout(configuration::FAN_TIMEOUT, self.uart.write_all(bytes)).await?;

        info!("{} UART write result: {:?}", fan_identifier, result);

        // Before closing we need to flush the buffer to ensure that all data is written
        // This requires blocking or we get a WouldBlock error. I don't understand why (TODO)
        let result = self.uart.blocking_flush();
        if let Err(_error) = result {
            error!("{} UART flush error", fan_identifier);
        }

        // In addition to flushing we need to wait for some time before turning off data in on the
        // MAX845 because we might be too fast and cut off the last byte or more. (This happened)
        // I saw someone using 120 microseconds (https://youtu.be/i46jdhvRej4?t=886).
        // This number is based on trial and error. Don't feel bad to change it if it doesn't work.
        // Also timings in microseconds are not accurate.
        // I assume this should be below the modbus message delay
        // Timer::after(Duration::from_micros(1_000)).await;
        // Using await timer breaks this too. Probably because it yields to the scheduler
        block_for(BLOCK_FOR);

        // Close sending data to enable receiving data
        self.driver_enable.set_low();

        // Read
        // Read response from fan. The response can vary in length
        let mut response_buffer: [u8; 8] = [0; 8];
        info!("{} Waiting for response from fan", fan_identifier);
        let bytes_read = with_timeout(
            configuration::FAN_TIMEOUT,
            //TODO test this does not wait for bytes to fill the buffer
            // leading to a timeout because the response is only 7 bytes but the buffer is 8 and it waits for the last byte to arrive
            self.uart.read(&mut response_buffer),
        )
        .await??;

        info!(
            "{} Response from fan: {:?} {:?}",
            fan_identifier, bytes_read, response_buffer
        );

        //TODO validate response from fan
        // Read the correct number of bytes
        Ok(())
    }
}
