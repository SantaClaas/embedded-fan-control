#![no_std]
#![no_main]

use core::future::poll_fn;
use core::ops::DerefMut;
use core::task::Poll;
use cyw43::{Control, NetDriver};
use cyw43_pio::PioSpi;
use debounce::Debouncer;
use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::dns::{DnsQueryType, DnsSocket};
use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};
use embassy_net::{tcp, Config, IpEndpoint, Stack, StackResources};
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals::{
    DMA_CH0, PIN_18, PIN_20, PIN_21, PIN_23, PIN_25, PIN_4, PIO0, UART0,
};
use embassy_rp::pio::{InterruptHandler as PioInterruptHandler, Pio, PioPin};
use embassy_rp::uart::BufferedInterruptHandler;
use embassy_rp::{bind_interrupts, Peripherals};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{self, Channel};
use embassy_sync::mutex::Mutex;
use embassy_sync::pubsub::PubSubChannel;
use embassy_sync::watch::Watch;
use embassy_time::{Duration, Instant, TimeoutError, Timer};
use embedded_nal_async::TcpConnect;
use mqtt::TryDecode;
use rand::RngCore;
use static_cell::StaticCell;

use {defmt_rtt as _, panic_probe as _};

use self::mqtt::packet;
use crate::fan::SetPoint;
use crate::mqtt::packet::ping_request::PingRequest;
use crate::mqtt::packet::{connect, publish, subscribe};
use crate::task::{set_up_network_stack, MqttBrokerConfiguration, Publish};

mod async_callback;
mod configuration;
mod debounce;
mod fan;
mod modbus;
mod mqtt;
mod task;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => PioInterruptHandler<PIO0>;
    UART0_IRQ  => BufferedInterruptHandler<UART0>;
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

#[embassy_executor::task]
async fn network_task(stack: &'static Stack<NetDriver<'static>>) -> ! {
    stack.run().await
}

#[derive(Clone, Copy)]
enum Event {}
static EVENT: PubSubChannel<CriticalSectionRawMutex, Event, 8, 4, 4> = PubSubChannel::new();
async fn trigger_event(mutex: &Mutex<CriticalSectionRawMutex, Option<u32>>) {
    let mut round = 0;
    loop {
        Timer::after_secs(3).await;
        let mut mutex = mutex.lock().await;
        *mutex = Some(round);
        round += 1;
    }
}

async fn wait_for_event(mutex: &Mutex<CriticalSectionRawMutex, Option<u32>>) {
    loop {
        let value = poll_fn(|context| match mutex.try_lock() {
            Ok(guard) => match *guard {
                None => Poll::Pending,
                Some(value) => Poll::Ready(value),
            },
            Err(_error) => Poll::Pending,
        })
        .await;

        info!("Got value {}", value);
    }
}
async fn how_to() {
    let mutex = Mutex::<CriticalSectionRawMutex, Option<u32>>::new(None);

    let f1 = trigger_event(&mutex);
    let f2 = wait_for_event(&mutex);
    join(f1, f2).await;
}

async fn gain_control(
    spawner: Spawner,
    pwr_pin: PIN_23,
    cs_pin: PIN_25,
    pio: PIO0,
    dma: DMA_CH0,
    dio: impl PioPin,
    clk: impl PioPin,
) -> (NetDriver<'static>, Control<'static>) {
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
    let spi = PioSpi::new(&mut pio.common, pio.sm0, pio.irq0, cs, dio, clk, dma);

    static STATE: StaticCell<cyw43::State> = StaticCell::new();
    let state = STATE.init(cyw43::State::new());
    let (net_device, mut control, runner) = cyw43::new(state, pwr, spi, firmware).await;
    unwrap!(spawner.spawn(wifi_task(runner)));
    control.init(chip_local_memory).await;
    control
        .set_power_management(cyw43::PowerManagementMode::PowerSave)
        .await;
    (net_device, control)
}

enum MqttError {
    WriteConnectError(connect::EncodeError),
    WriteError(tcp::Error),
    FlushError(tcp::Error),
    ReadError(tcp::Error),
    ReadPacketError(packet::ReadError),
    ConnectError(mqtt::ConnectErrorReasonCode),
    WriteSubscribeError(subscribe::EncodeError),
    UnexpectedPacketType(u8),
    WritePublishError(publish::EncodeError),
}
#[embassy_executor::task]
async fn mqtt_task_client(
    spawner: Spawner,
    pwr_pin: PIN_23,
    cs_pin: PIN_25,
    pio: PIO0,
    dma: DMA_CH0,
    dio: impl PioPin,
    clk: impl PioPin,
) -> () {
    let (net_device, mut control) =
        gain_control(spawner, pwr_pin, cs_pin, pio, dma, dio, clk).await;

    static STACK: StaticCell<Stack<NetDriver<'static>>> = StaticCell::new();
    static RESOURCES: StaticCell<StackResources<5>> = StaticCell::new();
    let configuration = Config::dhcpv4(Default::default());
    let mut random = RoscRng;
    let seed = random.next_u64();
    // Initialize network stack
    let stack = &*STACK.init(Stack::new(
        net_device,
        configuration,
        RESOURCES.init(StackResources::<5>::new()),
        seed,
    ));

    unwrap!(spawner.spawn(network_task(stack)));

    // Join Wi-Fi network
    loop {
        match control
            .join_wpa2(configuration::WIFI_NETWORK, configuration::WIFI_PASSWORD)
            .await
        {
            Ok(_) => break,
            Err(error) => info!("Error joining Wi-Fi network with status: {}", error.status),
        }
    }

    // Wait for DHCP
    info!("Waiting for DHCP");
    while !stack.is_config_up() {
        Timer::after_millis(100).await;
    }

    info!("DHCP is up");

    info!("Waiting for link up");
    while !stack.is_link_up() {
        Timer::after_millis(500).await;
    }

    info!("Link is up");

    info!("Waiting for stack to be up");
    stack.wait_config_up().await;
    info!("Stack is up");

    // Now we can use it
    let mut receive_buffer = [0; 1024];
    let mut send_buffer = [0; 1024];
    let mut socket = TcpSocket::new(stack, &mut receive_buffer, &mut send_buffer);

    let dns_client = DnsSocket::new(stack);

    info!("Resolving MQTT broker IP address");
    // Get home assistant MQTT broker IP address
    let address = loop {
        //TODO support IPv6
        let result = dns_client
            .query(configuration::MQTT_BROKER_ADDRESS, DnsQueryType::A)
            .await;

        let mut addresses = match result {
            Ok(addresses) => addresses,
            Err(error) => {
                info!(
                    "Error resolving Home Assistant MQTT broker IP address with {}: {:?}",
                    configuration::MQTT_BROKER_ADDRESS,
                    error,
                );
                // Exponential backoff doesn't seem necessary here
                // Maybe the current installation of Home Assistant is in the process of
                // being set up and the entry is not yet available
                Timer::after_secs(25).await;
                continue;
            }
        };

        if addresses.is_empty() {
            info!("No addresses found for Home Assistant MQTT broker");
            Timer::after_millis(500).await;
            continue;
        }

        break addresses.swap_remove(0);
    };
    info!("MQTT broker IP address resolved");

    info!("Connecting to MQTT broker through TCP");
    let endpoint = IpEndpoint::new(address, configuration::MQTT_BROKER_PORT);
    // Connect
    while let Err(error) = socket.connect(endpoint).await {
        info!(
            "Error connecting to Home Assistant MQTT broker: {:?}",
            error
        );
        Timer::after_millis(500).await;
    }

    use crate::mqtt::client::Client as MqttClient;

    let (reader, writer) = socket.split();
    if let Err(error) =
        MqttClient::<FanControlPublish, TcpWriter, tcp::Error, TcpReader, tcp::Error>::connect(
            reader, writer,
        )
        .await
    {
        info!(
            "Error connecting to Home Assistant MQTT broker: {:?}",
            error
        );
        return;
    }
    info!("MQTT connection established");

    info!("Subscribing to MQTT topics");
}

/// This task handles inputs from physical buttons to change the fan speed
#[embassy_executor::task]
async fn input(pin_18: PIN_18) {
    // The button just rotates through fan settings. This is because we currently only have one button
    // Will probably use something more advanced in the future
    let mut button = Debouncer::new(Input::new(pin_18, Pull::Up), Duration::from_millis(250));

    let mut fan_state = fan::State::default();
    let sender = FAN_CONTROLLER.fan_states.0.sender();

    loop {
        // Falling edge for our button -> button down (pressing down
        // Rising edge for our button -> button up (letting go after press)
        // Act on press as there is delay between pressing and letting go and it feels snappier
        button.debounce_falling_edge().await;
        info!("Button pressed");
        // Record time of button press for naive debounce
        let start = Instant::now();

        // Advance to next fan state
        fan_state = fan_state.next();

        let state = match fan_state {
            fan::State::Off => {
                let setting = FAN_CONTROLLER
                    .fan_states
                    .0
                    .try_get()
                    .map(|state| state.setting)
                    .unwrap_or(fan::SetPoint::ZERO);
                FanState {
                    setting,
                    is_on: false,
                }
            }
            fan::State::Low => FanState {
                setting: fan::user_setting::LOW,
                is_on: true,
            },
            fan::State::Medium => FanState {
                setting: fan::user_setting::MEDIUM,
                is_on: true,
            },
            fan::State::High => FanState {
                setting: fan::user_setting::HIGH,
                is_on: true,
            },
        };

        // Optimistically update setting
        sender.send(state);
    }
}

/// Update fans whenenver the fan setting or on state changes
#[embassy_executor::task]
async fn update_fans() {
    let Some(mut receiver) = FAN_CONTROLLER.fan_states.0.receiver() else {
        // Not using asserts because they are hard to debug on embedded where it crashed
        error!("No receiver for fan is on state. This should never happen.");
        return;
    };

    // Only comparing on state causes button triggers to be ignored
    let mut previous = FAN_CONTROLLER.fan_states.0.try_get().unwrap_or(FanState {
        is_on: false,
        setting: fan::SetPoint::ZERO,
    });

    loop {
        // This is expected to always provide the latest value.
        // Even if it had multiple updates while this loop was throttled
        let state = receiver.changed_and(|new| *new != previous).await;

        // Update previous before continue
        previous = state.clone();

        info!("Updating fans");
        let mut fans = FANS.lock().await;
        let Some(fans) = fans.deref_mut() else {
            warn!("No fan client found");
            continue;
        };

        let setting = if state.is_on {
            state.setting
        } else {
            // Turn off fans
            fan::SetPoint::ZERO
        };

        match fans.set_set_point(&setting).await {
            Ok(_) => {}
            Err(fan::Error::Timeout(TimeoutError)) => {
                error!("Timeout setting fan speed");
                continue;
            }
            Err(fan::Error::Uart(error)) => {
                error!("Uart error setting fan speed: {:?}", error);
                continue;
            }
        }

        // Throttle updates send to the fans
        Timer::after_millis(500).await;
    }
}

type Fans = Mutex<CriticalSectionRawMutex, Option<fan::Client<'static, UART0, PIN_4>>>;
/// Use this to make calls to the fans through modbus
static FANS: Fans = Mutex::new(None);

/// Fan state can have a setting while being off although and we emulate that behavior because
/// fan devices actually don't have that behavior
#[derive(PartialEq, Clone)]
struct FanState {
    is_on: bool,
    setting: fan::SetPoint,
}

struct FanController {
    /// Is on and the setting update independent of each other on homeassistant.
    /// Update this state even though it might not yet be set on the fan devices.
    /// Use optimistic updates.
    /// Senders:
    /// - MQTT (server to client)
    /// - Button
    /// Receivers:
    /// - Fan
    /// - MQTT (client to server)
    fan_states: (
        Watch<CriticalSectionRawMutex, FanState, 3>,
        Watch<CriticalSectionRawMutex, FanState, 3>,
    ),
}

impl FanController {
    const fn new() -> Self {
        Self {
            fan_states: (Watch::new(), Watch::new()),
        }
    }
}

static FAN_CONTROLLER: FanController = FanController::new();

/// Displays fan status with 2 LEDs:
/// Off Off -> Fans Off
/// On Off -> Fan on low setting
/// Off On -> Fan on medium setting
/// On On -> Fan on high setting
#[embassy_executor::task]
async fn led_routine(pin_21: PIN_21, pin_20: PIN_20) {
    // Setup LEDs
    let mut led_1 = Output::new(pin_21, Level::Low);
    let mut led_2 = Output::new(pin_20, Level::Low);

    // Flash LEDs for a second to check if they are working
    // This needs to handle all LEDs so they flash at the same time. Because an Output can't be turned back into its pin to be passed around.
    led_1.set_high();
    led_2.set_high();
    Timer::after_secs(1).await;
    led_1.set_low();
    led_2.set_low();

    let Some(mut receiver) = FAN_CONTROLLER.fan_states.0.receiver() else {
        // Not using asserts because they are hard to debug on embedded where it crashed
        error!("No receiver for fan is on state. This should never happen.");
        return;
    };

    // Set initial state
    let mut current_state = FAN_CONTROLLER.fan_states.0.try_get().unwrap_or(FanState {
        is_on: false,
        setting: fan::SetPoint::ZERO,
    });

    loop {
        let led_state = match current_state {
            FanState {
                is_on: false,
                setting: _,
            } => (Level::Low, Level::Low),
            FanState {
                is_on: true,
                setting,
            } => {
                if setting <= fan::user_setting::LOW {
                    (Level::High, Level::Low)
                } else if setting <= fan::user_setting::MEDIUM {
                    (Level::Low, Level::High)
                } else {
                    (Level::High, Level::High)
                }
            }
        };

        info!(
            "Setting status LEDs to {}, {}",
            led_state.0 == Level::High,
            led_state.1 == Level::High
        );
        led_1.set_level(led_state.0);
        led_2.set_level(led_state.1);

        // Wait for state update
        current_state = receiver.changed().await;
    }
}

enum FanControlPublish {
    SetFanSpeed { set_point: SetPoint },
}

impl From<publish::Publish<'_>> for FanControlPublish {
    fn from(publish: publish::Publish<'_>) -> Self {
        FanControlPublish::SetFanSpeed {
            set_point: SetPoint::new(12),
        }
    }
}

impl Publish for FanControlPublish {
    fn topic(&self) -> &str {
        "temporary"
    }

    fn payload(&self) -> &[u8] {
        b"25.5"
    }
}

/// Handles all the incoming MQTT messages and decides what to do with them in the context of the fan controller
#[embassy_executor::task]
async fn mqtt_brain_routine(
    receiver_in: channel::Receiver<'static, CriticalSectionRawMutex, FanControlPublish, 3>,
) {
    loop {
        let message = receiver_in.receive().await;
        // This is where we handle the incoming message and notify the other components of this fan controller
        match message {
            FanControlPublish => {
                info!("Received temporary publish!")
            }
        }
    }
}

/// Sets up and manages the MQTT connection like keeping it alive
#[embassy_executor::task]
async fn mqtt_routine(
    spawner: Spawner,
    pwr_pin: PIN_23,
    cs_pin: PIN_25,
    pio: PIO0,
    dma: DMA_CH0,
    dio: impl PioPin,
    clk: impl PioPin,
    sender_in: channel::Sender<'static, CriticalSectionRawMutex, FanControlPublish, 3>,
    receiver_out: channel::Receiver<'static, CriticalSectionRawMutex, FanControlPublish, 3>,
) {
    // Setting up the network in the task to not block from controlling the device without server connection
    let stack = set_up_network_stack(spawner, pwr_pin, cs_pin, pio, dma, dio, clk).await;

    crate::task::mqtt_with_connect(stack, sender_in, receiver_out, &configuration::MQTT_BROKER)
        .await;
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let Peripherals {
        PIN_23: pin_23,
        PIN_25: pin_25,
        PIO0: pio0,
        DMA_CH0: dma_ch0,
        DMA_CH1: dma_ch1,
        DMA_CH2: dma_ch2,
        PIN_24: pin_24,
        PIN_29: pin_29,
        // Driver enable/disable pin to switch between sending and receiving data on UART/Modbus
        PIN_4: pin_4,
        UART0: uart0,
        // Transmitter pin UART + Modbus
        PIN_12: pin_12,
        // Receiver pin UART + Modbus
        PIN_13: pin_13,
        // Button pin
        PIN_18: pin_18,
        // Status LEDs
        PIN_20: pin_20,
        PIN_21: pin_21,
        ..
    } = embassy_rp::init(Default::default());

    // UART

    /// Transmit buffer for UART
    static TX_BUFFER: StaticCell<[u8; 16]> = StaticCell::new();
    let tx_buffer = &mut TX_BUFFER.init([0; 16])[..];
    /// Receive buffer for UART
    static RX_BUFFER: StaticCell<[u8; 16]> = StaticCell::new();
    let rx_buffer = &mut RX_BUFFER.init([0; 16])[..];

    let client = fan::Client::new(
        uart0, pin_12, pin_13, Irqs, dma_ch1, dma_ch2, pin_4, tx_buffer, rx_buffer,
    );
    //TODO load fan setting from fan
    // Inner scope to drop the guard after assigning
    {
        *(FANS.lock().await) = Some(client);
    }

    // Button input task waits for button presses and send according signals to the modbus task
    unwrap!(spawner.spawn(input(pin_18)));
    unwrap!(spawner.spawn(update_fans()));

    /// Channel for messages incoming from the MQTT broker to this fan controller
    static IN: Channel<CriticalSectionRawMutex, FanControlPublish, 3> = Channel::new();
    let sender_in = IN.sender();
    let receiver_in = IN.receiver();

    /// Channel for messages outgoing from this fan controller to the MQTT broker
    static OUT: Channel<CriticalSectionRawMutex, FanControlPublish, 3> = Channel::new();
    let receiver_out = OUT.receiver();

    // The MQTT task waits for publishes from MQTT and sends them to the modbus task.
    // It also sends updates from the modbus task that happen through button inputs to MQTT
    unwrap!(spawner.spawn(mqtt_routine(
        spawner,
        pin_23,
        pin_25,
        pio0,
        dma_ch0,
        pin_24,
        pin_29,
        sender_in,
        receiver_out
    )));
    unwrap!(spawner.spawn(led_routine(pin_21, pin_20)));
    unwrap!(spawner.spawn(mqtt_brain_routine(receiver_in)));
}

#[cfg(test)]
mod tests {
    // The tests don't run on the embedded target, so we need to import the std crate

    extern crate std;
    use crate::fan::{SetPoint, SetPointOutOfBoundsError};

    use super::*;

    /// These are important hardcoded values I want to make sure are not changed accidentally
    #[test]
    fn setting_does_not_exceed_max_set_point() {
        core::assert_eq!(fan::MAX_SET_POINT, 64_000);
        core::assert_eq!(SetPoint::new(64_000), Ok(SetPoint(64_000)));
        core::assert_eq!(SetPoint::new(64_000 + 1), Err(SetPointOutOfBoundsError));
        core::assert_eq!(SetPoint::new(u16::MAX), Err(SetPointOutOfBoundsError));
    }
}
