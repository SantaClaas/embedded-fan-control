//! ebm-pabst [RadiCal centrifugal fans in scroll housings for residential ventilation](https://www.ebmpapst.com/us/en/campaigns/product-campaigns/centrifugal-fans/radical-with-scroll-housing.html)
//! specific configuration and constants

use crate::{configuration, modbus};
use cortex_m::prelude::_embedded_hal_serial_Write;
use defmt::{error, info, Format};
use embassy_rp::dma::Channel;
use embassy_rp::gpio::{Level, Output, Pin};
use embassy_rp::interrupt::typelevel::Binding;
use embassy_rp::uart::{Async, DataBits, InterruptHandler, Parity, RxPin, StopBits, TxPin, Uart};
use embassy_rp::{uart, Peripheral};
use embassy_time::{block_for, with_timeout, Duration, TimeoutError, Timer};

pub(crate) const BAUD_RATE: u32 = 19_200;
pub(crate) fn get_configuration() -> uart::Config {
    // I wish I could make this constant time but default isn't, there is no new and struct is non-exhaustive 😅
    let mut configuration: uart::Config = uart::Config::default();
    configuration.baudrate = BAUD_RATE;
    configuration.data_bits = DataBits::DataBits8;
    configuration.parity = Parity::ParityEven;
    configuration.stop_bits = StopBits::STOP1;
    // Setting inverts should be a no-op as they should be false by default
    configuration
}

pub(crate) const MAX_SET_POINT: u16 = 64_000;

#[derive(Debug, Format, Clone, Copy, PartialEq)]
pub(crate) struct Setting(pub(crate) u16);

#[derive(Debug, Format)]
pub(crate) struct SetPointOutOfBoundsError;

impl Setting {
    pub(crate) const ZERO: Self = Self(0);
    pub(crate) const fn new(set_point: u16) -> Result<Self, SetPointOutOfBoundsError> {
        if set_point > MAX_SET_POINT {
            return Err(SetPointOutOfBoundsError);
        }

        Ok(Self(set_point))
    }

    const fn get(&self) -> u16 {
        self.0
    }
}

#[derive(Default, Format, Debug)]
pub(crate) enum State {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

pub(crate) mod address {
    pub(crate) const FAN_1: u8 = 0x02;
    pub(crate) const FAN_2: u8 = 0x03;
}

pub(super) mod holding_registers {
    pub(crate) const REFERENCE_SET_POINT: [u8; 2] = 0xd001_u16.to_be_bytes();
}

mod input_registers {
    pub(super) const TEMPERATURE_SENSOR_1: [u8; 2] = 0xd02e_u16.to_be_bytes();
    pub(super) const HUMIDITY_SENSOR_1: [u8; 2] = 0xd02f_u16.to_be_bytes();
    pub(super) const TEMPERATURE_SENSOR_2: [u8; 2] = 0xd030_u16.to_be_bytes();
    pub(super) const HUMIDITY_SENSOR_2: [u8; 2] = 0xd031_u16.to_be_bytes();
}

pub(crate) enum Fan {
    One,
    Two,
}

/// Modbus messages are sent through UART to MAX845 to control fans.
/// The pin is used to enable the DE pin to switch between reading and writing
pub(crate) struct Client<'a, UART: uart::Instance, PIN: Pin> {
    uart: Uart<'a, UART, Async>,
    driver_enable: Output<'a, PIN>,
}

impl<'a, UART: uart::Instance, PIN: Pin> Client<'a, UART, PIN> {
    pub(crate) fn new(
        uart: impl Peripheral<P = UART> + 'a,
        tx: impl Peripheral<P = impl TxPin<UART>> + 'a,
        rx: impl Peripheral<P = impl RxPin<UART>> + 'a,
        irq: impl Binding<UART::Interrupt, InterruptHandler<UART>>,
        tx_dma: impl Peripheral<P = impl Channel> + 'a,
        rx_dma: impl Peripheral<P = impl Channel> + 'a,
        driver_enable: impl Peripheral<P = PIN> + 'a,
    ) -> Self {
        let uart = Uart::new(uart, tx, rx, irq, tx_dma, rx_dma, get_configuration());
        let driver_enable = Output::new(driver_enable, Level::Low);

        Self {
            uart,
            driver_enable,
        }
    }

    async fn send(&mut self, message: &[u8; 8]) -> Result<(), TimeoutError> {
        // Write then read
        // Set pin setting DE (driver enable) to on (high) on the MAX845 to send data
        self.driver_enable.set_high();

        // As ref because &[u8; 8] is not the same as &[u8]
        let result = with_timeout(
            configuration::FAN_TIMEOUT,
            self.uart.write(message.as_ref()),
        )
        .await?;
        // let result = self.uart.blocking_write(message.as_ref());
        info!("uart write result: {:?}", result);

        // Before closing we need to flush the buffer to ensure that all data is written
        // This requires blocking or we get a WouldBlock error. I don't understand why (TODO)
        let result = self.uart.blocking_flush();
        if let Err(error) = result {
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
        block_for(Duration::from_micros(1_000));

        // Close sending data to enable receiving data
        self.driver_enable.set_low();

        // Read
        // Read response from fan
        let mut response_buffer: [u8; 8] = [0; 8];
        info!("Waiting for response from fan");
        let response = with_timeout(
            configuration::FAN_TIMEOUT,
            self.uart.read(&mut response_buffer),
        )
        .await?;
        // let response = self.uart.blocking_read(&mut response_buffer);

        info!("response from fan: {:?} {:?}", response, response_buffer);
        //TODO validate response from fan
        Ok(())
    }

    /// The mutable reference to self here is important as there can only be one writer to the (mod)bus at a time
    pub(crate) async fn set_set_point(
        &mut self,
        Setting(set_point): &Setting,
    ) -> Result<(), TimeoutError> {
        // Send update through UART to MAX845 to modbus fans
        // Form message to fan 1
        let mut message: [u8; 8] = [
            // Device address fan 1
            address::FAN_1,
            // Modbus function code
            modbus::function_code::WRITE_SINGLE_REGISTER,
            // Holding register address
            holding_registers::REFERENCE_SET_POINT[0],
            holding_registers::REFERENCE_SET_POINT[1],
            // Value to set
            (set_point >> 8) as u8,
            *set_point as u8,
            // CRC is set later
            0,
            0,
        ];

        let checksum = modbus::CRC.checksum(&message[..6]).to_be_bytes();

        // They come out reversed (or is us using to_be_bytes reversed?)
        message[6] = checksum[1];
        message[7] = checksum[0];
        info!("Sending message to fan 1: {:?}", message);

        self.send(&message).await?;

        /// Messsage delay between modbus messages in microseconds
        const MESSAGE_DELAY: u64 = modbus::get_message_delay(BAUD_RATE);
        info!("Message delay {}", MESSAGE_DELAY);
        Timer::after_micros(MESSAGE_DELAY).await;

        // Form message to fan 2
        // Update the fan address and therefore the CRC
        // Keep speed as both fans should be running at the same speed
        message[0] = address::FAN_2;
        let checksum = modbus::CRC.checksum(&message[..6]).to_be_bytes();
        message[6] = checksum[1];
        message[7] = checksum[0];

        info!("sending message to fan 2: {:?}", message);
        self.send(&message).await?;
        Ok(())
    }

    pub(crate) async fn get_temperature(&mut self, fan: Fan) -> Result<u16, TimeoutError> {
        let mut message: [u8; 8] = [
            // Device address
            match fan {
                Fan::One => address::FAN_1,
                Fan::Two => address::FAN_2,
            },
            // Modbus function code
            modbus::function_code::READ_INPUT_REGISTER,
            // Input register address
            input_registers::TEMPERATURE_SENSOR_1[0],
            input_registers::TEMPERATURE_SENSOR_1[1],
            // Number of registers to read
            0,
            1,
            // CRC is set later
            0,
            0,
        ];

        let checksum = modbus::CRC.checksum(&message[..6]).to_be_bytes();

        // They come out reversed (or is us using to_be_bytes reversed?)
        message[6] = checksum[1];
        message[7] = checksum[0];
        info!("Sending read temperature message {:?}", message);

        // Set pin setting DE (driver enable) to on (high) on the MAX845 to send data
        self.driver_enable.set_high();
        let result = self.uart.write(&message).await;
        info!("uart write result: {:?}", result);
        // Before closing we need to flush the buffer to ensure that all data is written
        // This requires blocking or we get a WouldBlock error. I don't understand why (TODO)
        let result = self.uart.blocking_flush();
        if let Err(error) = result {
            error!("uart flush error");
        }

        // Wait to avoid cutting off last byte when turning off driver enable
        Timer::after(Duration::from_micros(190)).await;

        // Close sending data to enable receiving data
        self.driver_enable.set_low();

        // Read response
        let mut response_buffer: [u8; 8] = [0; 8];
        info!("Waiting for response from fan 1");
        let response = with_timeout(
            configuration::FAN_TIMEOUT,
            self.uart.read(&mut response_buffer),
        )
        .await?;

        info!("response from fan 1: {:?} {:?}", response, response_buffer);
        let length = response_buffer[2];
        let temperature = u16::from_be_bytes([response_buffer[3], response_buffer[4]]);
        info!("Temperature (divide by 10): {}", temperature);

        Ok(temperature)
    }
}
