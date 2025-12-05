use core::ops::Deref;

use defmt::{error, info};
use embassy_rp::{
    Peripheral, dma,
    gpio::{Level, Output, Pin},
    interrupt::typelevel::Binding,
    uart::{self, BufferedInterruptHandler, BufferedUart, RxPin, TxPin},
};
use embassy_time::{Duration, TimeoutError, Timer, block_for, with_timeout};
use embedded_io_async::{Read, Write};

use crate::{
    configuration,
    fan::{BAUD_RATE, FanResponse, address, holding_registers, set_point::SetPoint},
    modbus::{self, function::WriteHoldingRegister},
};

pub(crate) enum Error {
    Timeout(TimeoutError),
    Uart(uart::Error),
}

impl From<TimeoutError> for Error {
    fn from(error: TimeoutError) -> Self {
        Self::Timeout(error)
    }
}

impl From<uart::Error> for Error {
    fn from(error: uart::Error) -> Self {
        Self::Uart(error)
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
        tx_dma: impl Peripheral<P = impl dma::Channel> + 'a,
        rx_dma: impl Peripheral<P = impl dma::Channel> + 'a,
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

    pub(crate) async fn send_2<const REQUEST: usize, const RESPONSE: usize>(
        &mut self,
        message: impl modbus::ToBytes<REQUEST>,
    ) -> Result<FanResponse<RESPONSE>, Error> {
        // Write then read
        // Set pin setting DE (driver enable) to on (high) on the MAX845 to send data
        self.driver_enable.set_high();

        let bytes = message.to_bytes();
        info!("Sending message to fan: {:?}", bytes);
        // As ref because &[u8; 8] is not the same as &[u8]
        let result = with_timeout(configuration::FAN_TIMEOUT, self.uart.write_all(&bytes)).await?;

        info!("uart write result: {:?}", result);

        // Before closing we need to flush the buffer to ensure that all data is written
        // This requires blocking or we get a WouldBlock error. I don't understand why (TODO)
        let result = self.uart.blocking_flush();
        if let Err(_) = result {
            error!("uart flush error");
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
        let mut response_buffer: [u8; RESPONSE] = [0; RESPONSE];
        info!("Waiting for response from fan");
        let bytes_read = with_timeout(
            configuration::FAN_TIMEOUT,
            //TODO test this does not wait for bytes to fill the buffer
            // leading to a timeout because the response is only 7 bytes but the buffer is 8 and it waits for the last byte to arrive
            self.uart.read(&mut response_buffer),
        )
        .await??;

        info!("response from fan: {:?} {:?}", bytes_read, response_buffer);
        let response = FanResponse::new(response_buffer, bytes_read);

        //TODO validate response from fan
        // Read the correct number of bytes
        Ok(response)
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

    async fn send<const N: usize>(
        &mut self,
        message: impl AsRef<[u8]>,
    ) -> Result<FanResponse<N>, Error> {
        // Write then read
        // Set pin setting DE (driver enable) to on (high) on the MAX845 to send data
        self.driver_enable.set_high();

        info!("Sending message to fan: {:?}", message.as_ref());
        // As ref because &[u8; 8] is not the same as &[u8]
        let result = with_timeout(
            configuration::FAN_TIMEOUT,
            self.uart.write_all(message.as_ref()),
        )
        .await?;

        info!("uart write result: {:?}", result);

        // Before closing we need to flush the buffer to ensure that all data is written
        // This requires blocking or we get a WouldBlock error. I don't understand why (TODO)
        let result = self.uart.blocking_flush();
        if let Err(_error) = result {
            error!("uart flush error");
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
        let mut response_buffer: [u8; N] = [0; N];
        info!("Waiting for response from fan");
        let bytes_read = with_timeout(
            configuration::FAN_TIMEOUT,
            //TODO test this does not wait for bytes to fill the buffer
            // leading to a timeout because the response is only 7 bytes but the buffer is 8 and it waits for the last byte to arrive
            self.uart.read(&mut response_buffer),
        )
        .await??;

        info!("response from fan: {:?} {:?}", bytes_read, response_buffer);
        let response = FanResponse::new(response_buffer, bytes_read);

        //TODO validate response from fan
        // Read the correct number of bytes
        Ok(response)
    }

    //TODO decouple
    /// The mutable reference to self here is important as there can only be one writer to the (mod)bus at a time
    #[deprecated(note = "Decoupled fan from modbus")]
    pub(crate) async fn set_set_point(&mut self, set_point: &SetPoint) -> Result<(), Error> {
        // Send update through UART to MAX845 to modbus fans
        // Form message to fan 1
        let register_address = (*holding_registers::REFERENCE_SET_POINT).to_be_bytes();
        let value: u16 = *set_point.deref();
        let mut message: [u8; 8] = [
            // Device address fan 1
            *address::FAN_1,
            // Modbus function code
            modbus::function::code::WRITE_SINGLE_REGISTER,
            // Holding register address
            register_address[0],
            register_address[1],
            // Value to set
            (value >> 8) as u8,
            value as u8,
            // CRC is set later
            0,
            0,
        ];

        let checksum = modbus::CRC.checksum(&message[..6]).to_be_bytes();

        // They come out reversed (or is us using to_be_bytes reversed?)
        message[6] = checksum[1];
        message[7] = checksum[0];
        info!("Sending message to fan 1: {:?}", message);

        let _ = self.send::<8>(&message).await?;

        /// Messsage delay between modbus messages in microseconds
        const MESSAGE_DELAY: u64 = modbus::get_message_delay(BAUD_RATE);
        info!("Message delay {}", MESSAGE_DELAY);
        // We can yield the future here because the wait time between messages is a minimum and can be longer
        Timer::after_micros(MESSAGE_DELAY).await;

        // Form message to fan 2
        // Update the fan address and therefore the CRC
        // Keep speed as both fans should be running at the same speed
        message[0] = *address::FAN_2;
        let checksum = modbus::CRC.checksum(&message[..6]).to_be_bytes();
        message[6] = checksum[1];
        message[7] = checksum[0];

        info!("sending message to fan 2: {:?}", message);
        let _ = self.send::<8>(&message).await?;
        Ok(())
    }
}
