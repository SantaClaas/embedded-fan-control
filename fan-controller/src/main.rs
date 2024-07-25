//! This example test the RP Pico W on board LED.
//!
//! It does not work with the RP Pico board. See blinky.rs.

#![no_std]
#![no_main]

mod fan;
mod modbus;
mod mqtt;

use core::cmp::min;
use core::ops::Mul;
use crc::{Crc, CRC_16_MODBUS};
use cyw43::Control;
use cyw43_pio::PioSpi;
use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::yield_now;
use embassy_rp::gpio::{Input, Level, Output, Pin, Pull};
use embassy_rp::peripherals::{
    DMA_CH0, PIN_12, PIN_13, PIN_18, PIN_23, PIN_24, PIN_25, PIN_29, PIN_4, PIO0, UART0,
};
use embassy_rp::pio::{InterruptHandler, Pio, PioPin};
use embassy_rp::uart::Uart;
use embassy_rp::{bind_interrupts, uart, Peripherals, Peripheral, pio, dma};
use embassy_rp::spi::ClkPin;
use embassy_time::{Duration, Timer};
use static_cell::StaticCell;
use {defmt_rtt as _, panic_probe as _};

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => InterruptHandler<PIO0>;
});

#[embassy_executor::task]
async fn wifi_task(
    runner: cyw43::Runner<
        'static,
        Output<'static, PIN_23>,
        PioSpi<'static, PIN_25, PIO0, 0, DMA_CH0>,
    >,
) -> ! {
    runner.run().await
}

// async fn setup_control(spawner: Spawner) {
//     let firmware = include_bytes!("../cyw43-firmware/43439A0.bin");
//     // Google AI says CLM stands for "Chip Local Memory". Feels like everyone except me knows
//     // what it is. I hate acronyms. I searched for "CLM" in the source code and on the internet and
//     // still no idea.
//     let chip_local_memory = include_bytes!("../cyw43-firmware/43439A0_clm.bin");
//
//
//     let pwr = Output::new(pin_23, Level::Low);
//     let cs = Output::new(pin_25, Level::High);
//     let mut pio = Pio::new(pio0, Irqs);
//     let spi = PioSpi::new(
//         &mut pio.common,
//         pio.sm0,
//         pio.irq0,
//         cs,
//         pin_24,
//         pin_29,
//         dma_ch0,
//     );
//
//     static STATE: StaticCell<cyw43::State> = StaticCell::new();
//     let state = STATE.init(cyw43::State::new());
//     let (_net_device, mut control, runner) = cyw43::new(state, pwr, spi, firmware).await;
//     unwrap!(spawner.spawn(wifi_task(runner)));
//
//     control.init(chip_local_memory).await;
//     control
//         .set_power_management(cyw43::PowerManagementMode::PowerSave)
//         .await;
// }

async fn gain_control(
    spawner: Spawner,
    pwr_pin: PIN_23,
    cs_pin: PIN_25,
    pio: PIO0,//impl Peripheral<P=impl pio::Instance>,
    dma: DMA_CH0,//impl Peripheral<P=impl dma::Channel>,
    dio: impl PioPin,
    clk: impl PioPin,
) -> Control<'static> {
    let firmware = include_bytes!("../cyw43-firmware/43439A0.bin");
    // Google AI says CLM stands for "Chip Local Memory". Feels like everyone except me knows
    // what it is. I hate acronyms. I searched for "CLM" in the source code and on the internet and
    // still no idea.
    let chip_local_memory = include_bytes!("../cyw43-firmware/43439A0_clm.bin");

    // To make flashing faster for development, you may want to flash the firmwares independently
    // at hardcoded addresses, instead of baking them into the program with `include_bytes!`:
    //     probe-rs download 43439A0.bin --binary-format bin --chip RP2040 --base-address 0x10100000
    //     probe-rs download 43439A0_clm.bin --binary-format bin --chip RP2040 --base-address 0x10140000
    //let fw = unsafe { core::slice::from_raw_parts(0x10100000 as *const u8, 230321) };
    //let clm = unsafe { core::slice::from_raw_parts(0x10140000 as *const u8, 4752) };
    let pwr = Output::new(pwr_pin, Level::Low);
    let cs = Output::new(cs_pin, Level::High);
    let mut pio = Pio::new(pio, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        pio.irq0,
        cs,
        dio,
        clk,
        dma,
    );

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (_net_device, mut control, runner) = cyw43::new(state, pwr, spi, firmware).await;
    unwrap!(spawner.spawn(wifi_task(runner)));
    control.init(chip_local_memory).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;
    control
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let Peripherals {
        PIN_23: pin_23,
        PIN_25: pin_25,
        PIO0: pio0,
        DMA_CH0: dma_ch0,
        PIN_24: pin_24,
        PIN_29: pin_29,
        PIN_4: pin_4,
        UART0: uart0,
        PIN_12: pin_12,
        PIN_13: pin_13,
        PIN_18: pin_18,
        ..
    } = embassy_rp::init(Default::default());

    let mut control = gain_control(
        spawner,
        pin_23,
        pin_25,
        pio0,
        dma_ch0,
        pin_24,
        pin_29,
    )
        .await;

    // UART things
    // PIN_4 seems to refer to GP4 on the Pico W pinout
    let mut driver_enable = Output::new(pin_4, Level::Low);
    //TODO read up on interrupts
    let mut uart = Uart::new_blocking(uart0, pin_12, pin_13, fan::get_configuration());

    let on_duration = Duration::from_secs(1);
    let off_duration = Duration::from_secs(1);

    // Send signal test button
    let mut button = Input::new(pin_18, Pull::Up);
    // Could make this a state machine with phantom data
    let mut fan_state = fan::State::default();

    loop {
        // Falling edge for our button -> button down (pressing down
        // Rising edge for our button -> button up (letting go after press)
        // Act on press as there is delay between pressing and letting go and it feels snappier
        button.wait_for_falling_edge().await;
        // Advance to next fan state
        fan_state = match fan_state {
            fan::State::Off => fan::State::Low,
            fan::State::Low => fan::State::Medium,
            fan::State::Medium => fan::State::High,
            fan::State::High => fan::State::Off,
        };

        info!("fan state: {:?}", fan_state);

        // Setting values low on purpose for testing
        let set_point = match fan_state {
            fan::State::Off => 0,
            // 10%
            fan::State::Low => fan::MAX_SET_POINT / 10,
            // 25%
            fan::State::Medium => fan::MAX_SET_POINT / 4,
            // 50%
            fan::State::High => fan::MAX_SET_POINT / 2,
        }
            .to_be_bytes();

        control.gpio_set(0, true).await;
        // Form message to fan 1
        let mut message: [u8; 8] = [
            // Device address fan 1
            fan::address::FAN_1,
            // Modbus function code
            modbus::function_code::WRITE_SINGLE_REGISTER,
            // Holding register address
            fan::holding_registers::REFERENCE_SET_POINT[0],
            fan::holding_registers::REFERENCE_SET_POINT[1],
            // Value to set
            set_point[0],
            set_point[1],
            // CRC set later
            0,
            0,
        ];

        let checksum = modbus::CRC.checksum(&message[..6]).to_be_bytes();

        // They come out reversed (or is us using to_be_bytes reversed?)
        message[6] = checksum[1];
        message[7] = checksum[0];

        info!("Sending message to fan 1: {:?}", message);

        // Set pin setting DE (driver enable) to on (high) on the MAX845 to send data
        driver_enable.set_high();
        let result = uart.blocking_write(&message);
        info!("uart result: {:?}", result);

        // Before closing we need to flush the buffer to ensure that all data is written
        let result = uart.blocking_flush();

        // Wait to not cut off MAX845 last byte
        Timer::after(Duration::from_micros(190)).await;

        // Close sending data to enable receiving data
        driver_enable.set_low();

        // Read response from fan 1
        let mut response_buffer: [u8; 8] = [0; 8];
        let response = uart.blocking_read(&mut response_buffer);
        info!("response from fan 1: {:?} {:?}", response, response_buffer);
        //TODO validate response from fan 1

        /// Messsage delay between modbus messages in microseconds
        const MESSAGE_DELAY: u64 = modbus::get_message_delay(fan::BAUD_RATE);
        Timer::after_micros(MESSAGE_DELAY).await;

        // Form message to fan 2
        // Update the fan address and therefore the CRC
        // Keep speed as both fans should be running at the same speed
        message[0] = fan::address::FAN_2;
        let checksum = modbus::CRC.checksum(&message[..6]).to_be_bytes();
        message[6] = checksum[1];
        message[7] = checksum[0];
        info!("sending message to fan 2: {:?}", message);

        // Set pin setting DE (driver enable) to on (high) on the MAX845 to send data
        driver_enable.set_high();

        let result = uart.blocking_write(&message);
        info!("uart result: {:?}", result);

        // Before closing we need to flush the buffer to ensure that all data is written
        let result = uart.blocking_flush();
        info!("uart flush result: {:?}", result);

        // In addition to flushing we need to wait for some time before turning off data in on the
        // MAX845 because we might be too fast and cut off the last byte or more. (This happened)
        // I saw someone using 120 microseconds (https://youtu.be/i46jdhvRej4?t=886). This number
        // is based on trial and error. Don't feel bad to change it if it doesn't work.
        Timer::after(Duration::from_micros(190)).await;

        // Close sending data to enable receiving data
        driver_enable.set_low();
        //TODO validate response from fan 2

        // Read response from fan 2
        let response = uart.blocking_read(&mut response_buffer);
        info!("response from fan 2: {:?} {:?}", response, response_buffer);

        control.gpio_set(0, false).await;
    }
}
