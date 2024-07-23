//! This example test the RP Pico W on board LED.
//!
//! It does not work with the RP Pico board. See blinky.rs.

#![no_std]
#![no_main]

mod fan;

use core::cmp::min;
use core::ops::Mul;
use cyw43_pio::PioSpi;
use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::yield_now;
use embassy_rp::gpio::{Level, Output};
use embassy_rp::peripherals::{DMA_CH0, PIN_23, PIN_25, PIO0};
use embassy_rp::pio::{InterruptHandler, Pio};
use embassy_rp::uart::Uart;
use embassy_rp::{bind_interrupts, uart};
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

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let peripherals = embassy_rp::init(Default::default());
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

    let pwr = Output::new(peripherals.PIN_23, Level::Low);
    let cs = Output::new(peripherals.PIN_25, Level::High);
    let mut pio = Pio::new(peripherals.PIO0, Irqs);
    let spi = PioSpi::new(
        &mut pio.common,
        pio.sm0,
        pio.irq0,
        cs,
        peripherals.PIN_24,
        peripherals.PIN_29,
        peripherals.DMA_CH0,
    );

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (_net_device, mut control, runner) = cyw43::new(state, pwr, spi, firmware).await;
    unwrap!(spawner.spawn(wifi_task(runner)));

    control.init(chip_local_memory).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;

    // UART things
    // PIN_4 seems to refer to GP4 on the Pico W pinout
    let mut driver_enable = Output::new(peripherals.PIN_4, Level::Low);
    //TODO read up on interrupts
    let mut uart = Uart::new_blocking(
        peripherals.UART0,
        peripherals.PIN_12,
        peripherals.PIN_13,
        fan::get_configuration(),
    );

    let on_duration = Duration::from_secs(1);
    let off_duration = Duration::from_secs(1);

    loop {
        info!("led on!");
        control.gpio_set(0, true).await;
        Timer::after(on_duration).await;

        // Set pin setting DI (data in) to on (high) on the MAX845 to send data
        driver_enable.set_high();
        let message = [
            0b100010u8, 0b100010, 0b101010, 0b101010, 0b101010, 0b101010, 0b101010, 0b101010,
        ];
        let result = uart.blocking_write(&message);

        info!("uart result: {:?}", result);

        // Before closing we need to flush the buffer to ensure that all data is written
        // And we need to wait for some time before turning off data in on the MAX845 because we
        // might be too fast and cut off the last byte or more. (This happened)
        let result = uart.blocking_flush();
        // I saw someone using 120 microseconds.
        // Found out just 0 microseconds might be enough. This probably has to do with async. Should
        // try a blocking non-async version for delay
        // Timer::after(Duration::from_micros(0)).await;
        // This works the same
        yield_now().await;

        // Close sending data to enable receiving data
        driver_enable.set_low();
        info!("uart flush result: {:?}", result);

        info!("led off!");
        control.gpio_set(0, false).await;
        Timer::after(off_duration).await;
    }
}
