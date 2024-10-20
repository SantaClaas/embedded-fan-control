//! ebm-pabst [RadiCal centrifugal fans in scroll housings for residential ventilation](https://www.ebmpapst.com/us/en/campaigns/product-campaigns/centrifugal-fans/radical-with-scroll-housing.html)
//! specific configuration and constants

use crate::modbus;
use cortex_m::prelude::_embedded_hal_serial_Write;
use defmt::{error, info, Format};
use embassy_rp::dma::Channel;
use embassy_rp::gpio::{Level, Output, Pin};
use embassy_rp::interrupt::typelevel::Binding;
use embassy_rp::uart::{Async, DataBits, InterruptHandler, Parity, RxPin, StopBits, TxPin, Uart};
use embassy_rp::{uart, Peripheral};
use embassy_time::{Duration, Timer};

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

pub(crate) struct FanClient<'a, UART: uart::Instance, PIN: Pin> {
    uart: Uart<'a, UART, Async>,
    driver_enable: Output<'a, PIN>,
}

impl<'a, UART: uart::Instance, PIN: Pin> FanClient<'a, UART, PIN> {
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

    pub(crate) async fn set_set_point(&mut self, set_point: u16) {
        if set_point > MAX_SET_POINT {
            return;
        }

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
            set_point as u8,
            // CRC is set later
            0,
            0,
        ];

        let checksum = modbus::CRC.checksum(&message[..6]).to_be_bytes();

        // They come out reversed (or is us using to_be_bytes reversed?)
        message[6] = checksum[1];
        message[7] = checksum[0];
        info!("Sending message to fan 1: {:?}", message);

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

        // Read response from fan 1
        let mut response_buffer: [u8; 8] = [0; 8];
        info!("Waiting for response from fan 1");
        let response = self.uart.read(&mut response_buffer).await;
        info!("response from fan 1: {:?} {:?}", response, response_buffer);
        //TODO validate response from fan 1

        /// Messsage delay between modbus messages in microseconds
        const MESSAGE_DELAY: u64 = modbus::get_message_delay(BAUD_RATE);
        Timer::after_micros(MESSAGE_DELAY).await;

        // Form message to fan 2
        // Update the fan address and therefore the CRC
        // Keep speed as both fans should be running at the same speed
        message[0] = address::FAN_2;
        let checksum = modbus::CRC.checksum(&message[..6]).to_be_bytes();
        message[6] = checksum[1];
        message[7] = checksum[0];
        info!("sending message to fan 2: {:?}", message);

        // Set pin setting DE (driver enable) to on (high) on the MAX845 to send data
        self.driver_enable.set_high();
        let result = self.uart.write(&message).await;
        info!("uart result: {:?}", result);

        // Before closing we need to flush the buffer to ensure that all data is written
        let result = self.uart.blocking_flush();
        if let Err(error) = result {
            info!("uart flush error");
        }

        // In addition to flushing we need to wait for some time before turning off data in on the
        // MAX845 because we might be too fast and cut off the last byte or more. (This happened)
        // I saw someone using 120 microseconds (https://youtu.be/i46jdhvRej4?t=886). This number
        // is based on trial and error. Don't feel bad to change it if it doesn't work.
        Timer::after(Duration::from_micros(190)).await;

        // Close sending data to enable receiving data
        self.driver_enable.set_low();
        //TODO validate response from fan 2

        // Read response from fan 2
        let response = self.uart.read(&mut response_buffer).await;
        info!("response from fan 2: {:?} {:?}", response, response_buffer);
    }
}
