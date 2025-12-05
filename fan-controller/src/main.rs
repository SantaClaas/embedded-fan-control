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
use embassy_futures::select::{Either, Either3, select, select3};
use embassy_net::Stack;
use embassy_rp::gpio::{Input, Level, Output, Pin, Pull};
use embassy_rp::peripherals::{
    DMA_CH0, PIN_4, PIN_18, PIN_20, PIN_21, PIN_23, PIN_25, PIO0, UART0,
};
use embassy_rp::pio::{InterruptHandler as PioInterruptHandler, Pio, PioPin};
use embassy_rp::uart::BufferedInterruptHandler;
use embassy_rp::{Peripherals, bind_interrupts};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{self, Channel};
use embassy_sync::mutex::Mutex;
use embassy_sync::once_lock::OnceLock;
use embassy_sync::pubsub::PubSubChannel;
use embassy_sync::signal::Signal;
use embassy_sync::watch::{self, Watch};
use embassy_time::{Duration, Instant, Timer};
use mqtt::TryDecode;
use static_cell::StaticCell;

use {defmt_rtt as _, panic_probe as _};

use crate::fan::Fan;
use crate::fan::set_point::{ParseSetPointError, SetPoint};
use crate::mqtt::packet::ping_request::PingRequest;
use crate::mqtt::packet::publish;
use crate::task::{MqttBrokerConfiguration, Publish, set_up_network_stack};

mod async_callback;
mod configuration;
mod debounce;
mod event_bus;
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

#[embassy_executor::task]
async fn input_routine(
    pin: PIN_18,
    mut display_state: (
        watch::Receiver<'static, CriticalSectionRawMutex, SetPoint, 2>,
        watch::Receiver<'static, CriticalSectionRawMutex, SetPoint, 2>,
    ),
    fan_state: (
        &'static Signal<CriticalSectionRawMutex, SetPoint>,
        &'static Signal<CriticalSectionRawMutex, SetPoint>,
    ),
) {
    // The button just rotates through fan settings. This is because we currently only have one button
    // Will probably use something more advanced in the future
    let mut button = Debouncer::new(Input::new(pin, Pull::Up), Duration::from_millis(250));

    loop {
        // Falling edge for our button -> button down (pressing down
        // Rising edge for our button -> button up (letting go after press)
        // Act on press as there is delay between pressing and letting go and it feels snappier
        info!("[Button] Waiting for falling edge");
        button.debounce_falling_edge().await;
        info!("[Button] Falling edge detected");

        // As the button controls both fans it will force them to be synchronous
        // Take the lowest of both fan states to decide the next advancement
        let fan_1_state = display_state.0.try_get().unwrap_or(SetPoint::ZERO);
        let fan_2_state = display_state.1.try_get().unwrap_or(SetPoint::ZERO);

        let base_state = core::cmp::min(fan_1_state, fan_2_state);

        let next_set_point = if base_state == SetPoint::ZERO {
            fan::user_setting::LOW
        } else if base_state <= fan::user_setting::LOW {
            fan::user_setting::MEDIUM
        } else if base_state <= fan::user_setting::MEDIUM {
            fan::user_setting::HIGH
        } else {
            SetPoint::ZERO
        };

        fan_state.0.signal(next_set_point);
        fan_state.1.signal(next_set_point);
    }
}

type ModbusMutex = Mutex<CriticalSectionRawMutex, modbus::Client<'static, UART0, PIN_4>>;
type ModbusOnceLock = OnceLock<ModbusMutex>;

/// This routine takes the latest fan state updates and updates all parts of the device that display a state.
/// This includes at the time of writing Home Assistant through MQTT and two status LEDs on the device.
/// Displays fan status with 2 LEDs:
/// Off Off -> Fans Off
/// On Off -> Fan on low setting
/// Off On -> Fan on medium setting
/// On On -> Fan on high setting
#[embassy_executor::task]
async fn display_routine(
    mut display_state: (
        watch::Receiver<'static, CriticalSectionRawMutex, SetPoint, 2>,
        watch::Receiver<'static, CriticalSectionRawMutex, SetPoint, 2>,
    ),
    led_state: &'static Signal<CriticalSectionRawMutex, LedState>,
    mqtt_out: channel::Sender<'static, CriticalSectionRawMutex, OutgoingPublish, CHANNEL_SIZE>,
) {
    // The current fan state that was last recorded
    let mut current_display_state: (Option<SetPoint>, Option<SetPoint>) = (None, None);
    // The incoming fan state update. Reset after every write to the displays. Can not use current fan state for this because that would lead to sending the same state twice.
    let mut display_update_state: (Option<SetPoint>, Option<SetPoint>) = (None, None);
    // The debounce allows us to skip the loop iterations where we have received one signal update but not yet the other. We usually expect them to arrive shortly after each other.
    // Debounce can only activate after the first signal update otherwise we would always unnecessarily stop waiting even though there has been no debounce
    let mut is_debounce_active = false;
    const IDENTIFIER: &str = "[Display]";
    loop {
        info!(
            "{} Waiting for fan state update. Is debounce active: {}",
            IDENTIFIER, is_debounce_active
        );
        // Debounce and take the latest signal of both.
        // Basically wait until there is no futher update and then set the lights to the latest state
        enum Update {
            WithDebounce(Either3<SetPoint, SetPoint, ()>),
            WithoutDebounce(Either<SetPoint, SetPoint>),
        }

        let update = if is_debounce_active {
            Update::WithDebounce(
                select3(
                    // Hope these are cancellation safe
                    display_state.0.changed(),
                    display_state.1.changed(),
                    Timer::after_millis(250),
                )
                .await,
            )
        } else {
            // If debounce is inactive we want to wait until one of the states updates and not until the timer completes
            Update::WithoutDebounce(
                select(display_state.0.changed(), display_state.1.changed()).await,
            )
        };

        match update {
            // Pattern matching in Rust is awesome!
            Update::WithDebounce(Either3::First(fan_1_state))
            | Update::WithoutDebounce(Either::First(fan_1_state)) => {
                // Count "same state update" as "no update"
                if current_display_state
                    .0
                    .is_some_and(|state| state == fan_1_state)
                {
                    continue;
                }

                // Store latest state to be picked up after debounce is over
                display_update_state.0.replace(fan_1_state);
                is_debounce_active = true;
                continue;
            }
            Update::WithDebounce(Either3::Second(fan_2_state))
            | Update::WithoutDebounce(Either::Second(fan_2_state)) => {
                // Count "same state update" as "no update"
                if current_display_state
                    .1
                    .is_some_and(|state| state == fan_2_state)
                {
                    continue;
                }

                display_update_state.1.replace(fan_2_state);
                is_debounce_active = true;
                continue;
            }
            // If there was no update after 250ms, we assume all updates have been sent, turn off the debounce and continue with setting the displays
            Update::WithDebounce(Either3::Third(())) => {
                // After this loop iteration, we start waiting possibly for infinity again until a state change occurs
                is_debounce_active = false;
            }
        }

        info!("[Display] Update after debounce");

        if let Some(update) = display_update_state.0 {
            // Turn on the fan on home assistant if it was off before
            // We already checked above if the new state is not the same as the current state
            if let Some(command) = current_display_state
                .0
                .and_then(|current| SetStateCommandValue::from_change(current, update))
            {
                // Update setting before is on state for smoother transition in homeassistant UI
                let publish = OutgoingPublish::UpdateState {
                    fan: Fan::One,
                    payload: command,
                };

                //TODO handle back pressure when channel is full. Try to send until new message comes in
                if let Err(channel::TrySendError::Full(_publish)) = mqtt_out.try_send(publish) {
                    error!("[Display] MQTT out channel is full",);
                    continue;
                }
            }

            // Update MQTT
            let publish = OutgoingPublish::UpdateSpeed {
                fan: Fan::One,
                payload: update.into(),
            };

            //TODO handle back pressure when channel is full. Try to send until new message comes in
            if let Err(channel::TrySendError::Full(_publish)) = mqtt_out.try_send(publish) {
                error!("[Display] MQTT out channel is full",);
                continue;
            }

            // Persist new state
            current_display_state.0.replace(update);
            // Reset
            display_update_state.0 = None;
        }

        if let Some(update) = display_update_state.1 {
            // Turn on the fan on home assistant if it was off before
            // We already checked above if the new state is not the same as the current state
            if let Some(command) = current_display_state
                .1
                .and_then(|current| SetStateCommandValue::from_change(current, update))
            {
                let publish = OutgoingPublish::UpdateState {
                    fan: Fan::Two,
                    payload: command,
                };

                //TODO handle back pressure when channel is full. Try to send until new message comes in
                if let Err(channel::TrySendError::Full(_publish)) = mqtt_out.try_send(publish) {
                    error!("[Display] MQTT out channel is full",);
                    continue;
                }
            }

            // Update MQTT
            let publish = OutgoingPublish::UpdateSpeed {
                fan: Fan::Two,
                payload: update.into(),
            };

            //TODO handle back pressure when channel is full. Try to send until new message comes in
            if let Err(channel::TrySendError::Full(_publish)) = mqtt_out.try_send(publish) {
                error!("[Display] MQTT out channel is full",);
                continue;
            }

            // Persist new state
            current_display_state.1.replace(update);
            // Reset
            display_update_state.1 = None;
        }

        // LEDs shows state of both fans so we need the current and not just the updated state
        // Current display state needs to be updated before this
        let new_led_state = match current_display_state {
            (Some(state_1), Some(state_2)) if state_1 == state_2 => {
                // Both states are the same so we can use one of them to compare
                if state_1 == SetPoint::ZERO {
                    LedState::Synchronized { led_1: Level::Low, led_2: Level::Low }
                } else if state_1 <= fan::user_setting::LOW {
                    LedState::Synchronized { led_1: Level::High, led_2: Level::Low }
                } else if state_1 <= fan::user_setting::MEDIUM {
                    LedState::Synchronized { led_1: Level::Low, led_2: Level::High }
                } else {
                    LedState::Synchronized { led_1: Level::High, led_2: Level::High }
                }
            }
            // Out of sync. Blink each LED individually to indicate their state.
            (Some(state_1), Some(state_2)) /* if state_1 != state_2 */ => {
                LedState::Unsynchronized { led_1: state_1.into(), led_2: state_2.into() }
            }
            // This is technically incorrect as not having a state for one of the fans does not mean it is off
            (Some(state_1), None) => LedState::Unsynchronized { led_1: state_1.into(), led_2: Blink::Off },
            (None, Some(state_2)) => LedState::Unsynchronized { led_1: Blink::Off, led_2: state_2.into() },
            // This could be made more elegant by making it check at compile time
            (None, None) => defmt::unreachable!("Reached invalid state of executing an LED display update when there is no current state or state update"),
        };

        led_state.signal(new_led_state);
    }
}

/// This is only a virtual off and on state for home assistant.
/// The fans actually don't power off or on. We just set the speed to 0 when off or some other value when on.
enum SetStateCommandValue {
    On,
    Off,
}

impl SetStateCommandValue {
    fn from_change(old_speed: SetPoint, new_speed: SetPoint) -> Option<Self> {
        if old_speed == SetPoint::ZERO && new_speed != SetPoint::ZERO {
            Some(SetStateCommandValue::On)
        } else if old_speed != SetPoint::ZERO && new_speed == SetPoint::ZERO {
            Some(SetStateCommandValue::Off)
        } else {
            None
        }
    }
}

enum FanCommand {
    SetSpeed { set_point: SetPoint },
    SetState(SetStateCommandValue),
}

enum IncomingPublish {
    FanCommand {
        /// The fan the publish is addressed to
        target: Fan,
        command: FanCommand,
    },
}

enum FromPublishError {
    // Invalid fan command
    InvalidStringPayload(core::str::Utf8Error),
    ParseSetPoint(ParseSetPointError),
    InvalidSetStateCommandPayload,

    UnknownTopic,
}

impl TryFrom<publish::Publish<'_>> for IncomingPublish {
    type Error = FromPublishError;

    fn try_from(publish: publish::Publish<'_>) -> Result<Self, Self::Error> {
        match publish.topic_name {
            topic::fan_controller::fan_1::state::COMMAND => match publish.payload {
                b"ON" => Ok(Self::FanCommand {
                    target: Fan::One,
                    command: FanCommand::SetState(SetStateCommandValue::On),
                }),
                b"OFF" => Ok(Self::FanCommand {
                    target: Fan::One,
                    command: FanCommand::SetState(SetStateCommandValue::Off),
                }),
                other => Err(FromPublishError::InvalidSetStateCommandPayload),
            },
            topic::fan_controller::fan_1::percentage::COMMAND => {
                let payload = core::str::from_utf8(publish.payload)
                    .map_err(FromPublishError::InvalidStringPayload)?;

                let set_point: SetPoint =
                    payload.parse().map_err(FromPublishError::ParseSetPoint)?;

                Ok(IncomingPublish::FanCommand {
                    target: Fan::One,
                    command: FanCommand::SetSpeed { set_point },
                })
            }
            topic::fan_controller::fan_2::state::COMMAND => match publish.payload {
                b"ON" => Ok(Self::FanCommand {
                    target: Fan::Two,
                    command: FanCommand::SetState(SetStateCommandValue::On),
                }),
                b"OFF" => Ok(Self::FanCommand {
                    target: Fan::Two,
                    command: FanCommand::SetState(SetStateCommandValue::Off),
                }),
                other => Err(FromPublishError::InvalidSetStateCommandPayload),
            },
            topic::fan_controller::fan_2::percentage::COMMAND => {
                let payload = core::str::from_utf8(publish.payload)
                    .map_err(FromPublishError::InvalidStringPayload)?;

                let set_point: SetPoint =
                    payload.parse().map_err(FromPublishError::ParseSetPoint)?;

                Ok(IncomingPublish::FanCommand {
                    target: Fan::Two,
                    command: FanCommand::SetSpeed { set_point },
                })
            }
            other => {
                warn!(
                    "Unexpected topic: {} with payload: {}",
                    other, publish.payload
                );
                Err(FromPublishError::UnknownTopic)
            }
        }
    }
}

struct UpdateSpeedPayload(heapless::String<5>);

impl From<SetPoint> for UpdateSpeedPayload {
    fn from(set_point: SetPoint) -> Self {
        let buffer = set_point.to_string();
        Self(buffer)
    }
}

enum OutgoingPublish {
    UpdateSpeed {
        fan: Fan,
        payload: UpdateSpeedPayload,
    },
    UpdateState {
        fan: Fan,
        payload: SetStateCommandValue,
    },
}

impl Publish for OutgoingPublish {
    fn topic(&self) -> &str {
        match self {
            OutgoingPublish::UpdateSpeed {
                fan: Fan::One,
                payload: _,
            } => topic::fan_controller::fan_1::percentage::STATE,
            OutgoingPublish::UpdateSpeed {
                fan: Fan::Two,
                payload: _,
            } => topic::fan_controller::fan_2::percentage::STATE,
            OutgoingPublish::UpdateState {
                fan: Fan::One,
                payload: _,
            } => topic::fan_controller::fan_1::state::STATE,
            OutgoingPublish::UpdateState {
                fan: Fan::Two,
                payload: _,
            } => topic::fan_controller::fan_2::state::STATE,
        }
    }

    fn payload(&self) -> &[u8] {
        match self {
            OutgoingPublish::UpdateSpeed { fan: _, payload } => {
                // set_point.0.to_be_bytes()
                payload.0.as_bytes()
            }
            OutgoingPublish::UpdateState { fan: _, payload } => match payload {
                SetStateCommandValue::On => b"ON",
                SetStateCommandValue::Off => b"OFF",
            },
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
    sender_in: channel::Sender<
        'static,
        CriticalSectionRawMutex,
        Result<IncomingPublish, FromPublishError>,
        CHANNEL_SIZE,
    >,
    receiver_out: channel::Receiver<
        'static,
        CriticalSectionRawMutex,
        OutgoingPublish,
        CHANNEL_SIZE,
    >,
) {
    // Setting up the network in the task to not block from controlling the device without server connection
    let stack = set_up_network_stack(spawner, pwr_pin, cs_pin, pio, dma, dio, clk).await;

    crate::task::mqtt_with_connect(stack, sender_in, receiver_out, &configuration::MQTT_BROKER)
        .await;
}

/// Handles all the incoming MQTT messages and decides what to do with them in the context of the fan controller
#[embassy_executor::task]
async fn mqtt_brain_routine(
    receiver_in: channel::Receiver<
        'static,
        CriticalSectionRawMutex,
        Result<IncomingPublish, FromPublishError>,
        CHANNEL_SIZE,
    >,
    fan_one_state: &'static Signal<CriticalSectionRawMutex, SetPoint>,
    fan_two_state: &'static Signal<CriticalSectionRawMutex, SetPoint>,
) {
    // Remembering the last fan state for when the home assistant turns the device off and then on again
    //TODO use state loaded from fan
    let mut last_fan_state = (SetPoint::ZERO, SetPoint::ZERO);
    loop {
        info!("[MQTT Brain] Waiting for new publish");
        let message = receiver_in.receive().await;
        info!("[MQTT Brain] Received publish");

        let publish = match message {
            Err(error) => {
                match error {
                    FromPublishError::InvalidStringPayload(utf8_error) => {
                        error!("Invalid UTF-8 payload");
                    }
                    FromPublishError::ParseSetPoint(parse_set_point_error) => {
                        error!("Invalid set point payload");
                    }
                    FromPublishError::UnknownTopic => error!("Unknown topic. Look for ealier logs"),
                    FromPublishError::InvalidSetStateCommandPayload => {
                        error!("Invalid set state command payload")
                    }
                }
                continue;
            }
            Ok(payload) => payload,
        };

        info!("Received valid payload!");

        //TODO make this configurable through a switch
        let is_synchronization_on = true;

        match publish {
            IncomingPublish::FanCommand {
                target,
                command: FanCommand::SetSpeed { set_point },
            } => match target {
                Fan::One => {
                    last_fan_state.0 = set_point;
                    fan_one_state.signal(set_point);
                    if is_synchronization_on {
                        fan_two_state.signal(set_point);
                    }
                }
                Fan::Two => {
                    last_fan_state.1 = set_point;
                    fan_two_state.signal(set_point);
                    if is_synchronization_on {
                        fan_one_state.signal(set_point);
                    }
                }
            },
            IncomingPublish::FanCommand {
                target,
                command: FanCommand::SetState(new_state),
            } => match target {
                Fan::One => match new_state {
                    SetStateCommandValue::On => fan_one_state.signal(last_fan_state.0),
                    SetStateCommandValue::Off => fan_one_state.signal(SetPoint::ZERO),
                },
                Fan::Two => match new_state {
                    SetStateCommandValue::On => fan_two_state.signal(last_fan_state.1),
                    SetStateCommandValue::Off => fan_two_state.signal(SetPoint::ZERO),
                },
            },
        }
    }
}

/// Receives the fan state updates and sends them to modbus as modbus messages
/// After a successful response, this sends an update to the fan display logic unit
#[embassy_executor::task(pool_size = 2)]
async fn fan_control_routine(
    fan_address: modbus::device::Address,
    current_fan_speed: &'static Signal<CriticalSectionRawMutex, SetPoint>,
    other_fan_speed: &'static Signal<CriticalSectionRawMutex, SetPoint>,
    modbus: &'static ModbusOnceLock,
    display_state: watch::Sender<'static, CriticalSectionRawMutex, SetPoint, 2>,
) {
    let fan_identifier = match *fan_address {
        2 => "[Fan 1]",
        3 => "[Fan 2]",
        other => "Unknown (oops)",
    };

    info!("{} Waiting for MODBUS initialization", fan_identifier);
    let modbus_mutex = modbus.get().await;
    info!("{} MODBUS initialized", fan_identifier);

    //TODO load initial fan speed through modbus from fan and make current_speed non optional
    let mut current_set_point: Option<SetPoint> = None;
    'signal_loop: loop {
        info!("{} Waiting for fan state update", fan_identifier);
        let mut set_point = current_fan_speed.wait().await;
        if current_set_point.is_some_and(|speed| speed == speed) {
            //TODO consider to update fan display state nontheless
            info!(
                "{} Fan state update received but has same state",
                fan_identifier
            );
            continue;
        }

        info!("{} Received fan state", fan_identifier);

        // Instruct modbus to send update
        info!("{} Attempting to acquire lock (again?)", fan_identifier);
        let mut modbus = modbus_mutex.lock().await;
        info!("{} Acquired lock on modbus (again?)", fan_identifier);
        // Check we have the latest state in case it was updated while waiting for the lock
        set_point = current_fan_speed.try_take().unwrap_or(set_point);

        let function = modbus::function::WriteHoldingRegister::new(
            fan_address,
            fan::holding_registers::REFERENCE_SET_POINT,
            *set_point,
        );

        info!("{} Sending fan state update through modbus", fan_identifier);
        const MAX_ATTEMPTS: u8 = 3;
        let mut attempt = 1;
        while let Err(error) = modbus.send_3(&function).await
            && attempt <= MAX_ATTEMPTS
        {
            // Release lock so other tasks get a chance to access modbus for sending messages to devices
            drop(modbus);

            error!(
                "{} Failed to send fan state update with attempt {}",
                fan_identifier, attempt
            );
            attempt += 1;

            if current_fan_speed.signaled() {
                continue 'signal_loop;
            }

            // Exponential backoff
            // Safe power of 2 because maximum value is 3 (900ms max)
            Timer::after_millis(u64::from(attempt).pow(2) * 100).await;
            info!("{} Waiting for lock on modbus for retry", fan_identifier);
            modbus = modbus_mutex.lock().await;
            info!("{} Acquired lock on modbus for retry", fan_identifier);
        }

        if attempt > MAX_ATTEMPTS {
            error!(
                "{} Failed to send fan state update after {} attempts",
                fan_identifier, MAX_ATTEMPTS
            );

            //TODO don't try to update other fan speed if we have a setting to allow fans to run out of sync
            // Set other fan to current fan speed to avoid them getting out of sync and creating over or underpressure in the house
            //TODO fix endless loop when both fan speed setting fails and they keep retrying and sending each other instructions to set back to previous speed.
            //TODO Could additionally provide a retry strategy to the signal that is set to once to avoid endless loop
            //TODO or provide a counter to detect the endless loop

            // There is no Option::copied or Option::cloned for some reason in core
            current_set_point.inspect(|speed| other_fan_speed.signal(speed.clone()));
            continue;
        }

        info!(
            "{} Fan state updated after {} attempts",
            fan_identifier, attempt
        );

        // On success send update to fan display logic unit
        display_state.send_if_modified(|current| {
            if current.is_none_or(|current| current != set_point) {
                current.replace(set_point);
                return true;
            }

            false
        });

        info!("{} Updated display state", fan_identifier);
        current_set_point = Some(set_point);
    }
}

#[derive(Debug, Clone, Copy)]
enum Blink {
    Off,
    Once,
    Twice,
    Thrice,
}

impl From<SetPoint> for Blink {
    fn from(set_point: SetPoint) -> Self {
        if set_point == SetPoint::ZERO {
            Blink::Off
        } else if set_point <= fan::user_setting::LOW {
            Blink::Once
        } else if set_point <= fan::user_setting::MEDIUM {
            Blink::Twice
        } else {
            Blink::Thrice
        }
    }
}

enum LedState {
    Synchronized { led_1: Level, led_2: Level },
    Unsynchronized { led_1: Blink, led_2: Blink },
}

async fn blink<'d, T: Pin>(led: &mut Output<'d, T>, blink: Blink) {
    let pause = || Timer::after_millis(500);
    match blink {
        Blink::Off => {
            if led.is_set_high() {
                led.set_low();
            }
        }
        Blink::Once => {
            led.set_high();
            pause().await;
            led.set_low();
        }
        Blink::Twice => {
            led.set_high();
            pause().await;
            led.set_low();
            pause().await;
            led.set_high();
            pause().await;
            led.set_low();
        }
        Blink::Thrice => {
            led.set_high();
            pause().await;
            led.set_low();
            pause().await;
            led.set_high();
            pause().await;
            led.set_low();
            pause().await;
            led.set_high();
            pause().await;
            led.set_low();
        }
    }
}

/// This task controls the LEDs based on the current state of the fan.
/// It acts a bit like the MQTT task that is used to display the state of the fans in Home Assistant.
#[embassy_executor::task]
async fn led_routine(
    pin_20: PIN_20,
    pin_21: PIN_21,
    led_state: &'static Signal<CriticalSectionRawMutex, LedState>,
) {
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

    let mut current_state = None;
    loop {
        if let Some(new_state) = led_state.try_take() {
            current_state = Some(new_state);
        }

        match current_state {
            // Using a quick switch between states as something moving faster gives the illusion of loading faster
            None => {
                const PAUSE_TIME: u64 = 250;
                Timer::after_millis(PAUSE_TIME).await;
                led_2.set_low();
                led_1.set_high();
                Timer::after_millis(PAUSE_TIME).await;
                led_1.set_low();
                led_2.set_high();
                // Continue loop and repeat if state did not change
            }
            Some(LedState::Synchronized {
                led_1: ref led_1_level,
                led_2: ref led_2_level,
            }) => {
                led_1.set_level(*led_1_level);
                led_2.set_level(*led_2_level);
            }
            Some(LedState::Unsynchronized {
                led_1: ref led_1_state,
                led_2: ref led_2_state,
            }) => {
                /*
                 * Note: blink counts can be the same but that does not mean the fan state is the same.
                 * The blink counts just represent if the fan is running within a certain range (low, medium, high).
                 * Within these ranges, the fan state can be different.
                 */

                // Switching between blinking the state of one LED/fan and only then the other is intended
                // to make it clearer that they don't run the same
                blink(&mut led_1, *led_1_state).await;
                Timer::after_secs(5).await;
                blink(&mut led_2, *led_2_state).await;
                Timer::after_secs(5).await;
            }
        }
    }
}
const CHANNEL_SIZE: usize = 8;

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

    let client = modbus::client::Client::new(
        uart0,
        pin_12,
        pin_13,
        Irqs,
        dma_ch1,
        dma_ch2,
        pin_4,
        tx_buffer,
        rx_buffer,
        fan::get_configuration(),
    );

    static FANS: ModbusOnceLock = ModbusOnceLock::new();
    // Just initialize it
    _ = FANS.get_or_init(|| client.into());

    /// Channel for messages incoming from the MQTT broker to this fan controller
    static IN: Channel<
        CriticalSectionRawMutex,
        Result<IncomingPublish, FromPublishError>,
        CHANNEL_SIZE,
    > = Channel::new();
    let sender_in = IN.sender();

    /// Channel for messages outgoing from this fan controller to the MQTT broker
    static OUT: Channel<CriticalSectionRawMutex, OutgoingPublish, CHANNEL_SIZE> = Channel::new();
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

    static LED_STATE: Signal<CriticalSectionRawMutex, LedState> = Signal::new();
    unwrap!(spawner.spawn(led_routine(pin_20, pin_21, &LED_STATE)));

    // The display state is updated after the fan state has been successfully applied
    // and is used to update any component that displays the fan state like the LEDs or Home Assistant through MQTT
    static FAN_ONE_DISPLAY_STATE: Watch<CriticalSectionRawMutex, SetPoint, 2> = Watch::new();
    static FAN_TWO_DISPLAY_STATE: Watch<CriticalSectionRawMutex, SetPoint, 2> = Watch::new();

    let display_receivers = (
        FAN_ONE_DISPLAY_STATE
            .receiver()
            .expect("Expected receiver to be configured to allow 2 receivers"),
        FAN_TWO_DISPLAY_STATE
            .receiver()
            .expect("Expected receiver to be configured to allow 2 receivers"),
    );
    let sender_out = OUT.sender();
    unwrap!(spawner.spawn(display_routine(display_receivers, &LED_STATE, sender_out)));

    static FAN_ONE_STATE: Signal<CriticalSectionRawMutex, SetPoint> = Signal::new();
    static FAN_TWO_STATE: Signal<CriticalSectionRawMutex, SetPoint> = Signal::new();

    let button_receivers = (
        FAN_ONE_DISPLAY_STATE
            .receiver()
            .expect("Expected receiver to be configured to allow 2 receivers"),
        FAN_TWO_DISPLAY_STATE
            .receiver()
            .expect("Expected receiver to be configured to allow 2 receivers"),
    );
    unwrap!(spawner.spawn(input_routine(
        pin_18,
        button_receivers,
        (&FAN_ONE_STATE, &FAN_TWO_STATE)
    )));

    let receiver_in = IN.receiver();
    unwrap!(spawner.spawn(mqtt_brain_routine(
        receiver_in,
        &FAN_ONE_STATE,
        &FAN_TWO_STATE
    )));

    let display_fan_one_sender = FAN_ONE_DISPLAY_STATE.sender();
    let display_fan_two_sender = FAN_TWO_DISPLAY_STATE.sender();

    unwrap!(spawner.spawn(fan_control_routine(
        fan::address::FAN_1,
        &FAN_ONE_STATE,
        &FAN_TWO_STATE,
        &FANS,
        display_fan_one_sender,
    )));
    unwrap!(spawner.spawn(fan_control_routine(
        fan::address::FAN_2,
        &FAN_TWO_STATE,
        &FAN_ONE_STATE,
        &FANS,
        display_fan_two_sender,
    )));
}
