#![no_std]
#![no_main]

use core::str;

use cyw43::{Control, NetDriver};
use cyw43_pio::PioSpi;
use debounce::Debouncer;
use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::select::{Either, Either3, select, select3};
use embassy_net::Stack;
use embassy_rp::gpio::{Input, Level, Output, Pin, Pull};
use embassy_rp::peripherals::{
    DMA_CH0, PIN_4, PIN_7, PIN_18, PIN_20, PIN_21, PIN_23, PIN_25, PIO0, UART0, UART1,
};
use embassy_rp::pio::{InterruptHandler as PioInterruptHandler, Pio, PioPin};
use embassy_rp::uart::BufferedInterruptHandler;
use embassy_rp::{Peripherals, bind_interrupts};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{self, Channel};
use embassy_sync::mutex::Mutex;
use embassy_sync::once_lock::OnceLock;
use embassy_sync::signal::Signal;
use embassy_sync::watch::{self, Watch};
use embassy_time::{Duration, Timer};
use mqtt::TryDecode;
use static_cell::StaticCell;

use {defmt_rtt as _, panic_probe as _};

use crate::fan::Fan;
use crate::fan::set_point::{ParseSetPointError, SetPoint};
use crate::mqtt::packet::ping_request::PingRequest;
use crate::mqtt::packet::publish;
use crate::task::{MqttBrokerConfiguration, Publish, set_up_network_stack};

mod configuration;
mod debounce;
mod fan;
mod modbus;
mod mqtt;
mod relay;
mod reset_cause;
mod task;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => PioInterruptHandler<PIO0>;
    UART0_IRQ  => BufferedInterruptHandler<UART0>;
    // The relay module's bus. Its own UART rather than an address on the fans': it answers 8N1 and
    // only 8N1, and the fans run 8E1
    UART1_IRQ  => BufferedInterruptHandler<UART1>;
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
    mut display_state: (DisplayStateReceiver, DisplayStateReceiver),
    fan_state: (&'static SetPointSignal, &'static SetPointSignal),
) {
    // The button just rotates through fan settings. This is because we currently only have one button
    // Will probably use something more advanced in the future
    // 50 ms is longer than the line bounces and longer than anything interference couples into
    // the wiring, and shorter than the shortest deliberate tap
    let mut button = Debouncer::new(Input::new(pin, Pull::Up), Duration::from_millis(50));

    loop {
        // Falling edge for our button -> button down (pressing down
        // Rising edge for our button -> button up (letting go after press)
        // Act on press as there is delay between pressing and letting go and it feels snappier
        info!("[Button] Waiting for press");
        button.debounce_falling_edge().await;
        info!("[Button] Press detected");

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

        fan_state
            .0
            .signal(RequestedSetPoint::FromUser(next_set_point));
        fan_state
            .1
            .signal(RequestedSetPoint::FromUser(next_set_point));
    }
}

type ModbusMutex = Mutex<CriticalSectionRawMutex, modbus::Client<'static, UART0, PIN_4>>;
type ModbusOnceLock = OnceLock<ModbusMutex>;

/// The relay module's bus, which is owned outright rather than shared.
///
/// The fans' client is behind a mutex and a once lock because four tasks reach for it — two that
/// set a speed and two that poll sensors — and behind the once lock so those tasks can await its
/// initialization rather than race it. Nothing of the sort applies here: one task talks to this
/// bus, so it holds the client itself and there is no lock for anything to wait on
type RelayClient = modbus::Client<'static, UART1, PIN_7>;

/// The state Home Assistant last asked the contact to be in. Only the latest matters, the same way
/// only the latest set point does, so it is a signal rather than a channel
type RelayStateSignal = Signal<CriticalSectionRawMutex, bool>;

/// For the log, matching the identifiers the modbus client prints
const RELAY_IDENTIFIER: &str = "[Relay]";

/// A set point signalled to a [`fan_control_routine`], together with where it came from. The
/// origin is what decides whether a fan that cannot be reached drags the other one with it
#[derive(Clone, Copy, Format)]
enum RequestedSetPoint {
    /// Home Assistant or the button asked for this speed
    FromUser(SetPoint),
    /// The other fan is pushing this one back to the speed they were last in sync at, because its
    /// own write failed
    FromOtherFan(SetPoint),
}

impl RequestedSetPoint {
    fn set_point(self) -> SetPoint {
        match self {
            Self::FromUser(set_point) | Self::FromOtherFan(set_point) => set_point,
        }
    }

    /// Whether failing to apply this should push the other fan back, to keep the two from drifting
    /// apart and putting the house under or over pressure.
    ///
    /// Only a request from a user does. A correction that fails means the bus is down rather than
    /// the two fans disagreeing, and answering it with another correction is exactly what made the
    /// fans signal each other back and forth without ever stopping
    fn should_correct_other_fan(self) -> bool {
        matches!(self, Self::FromUser(_))
    }
}

type SetPointSignal = Signal<CriticalSectionRawMutex, RequestedSetPoint>;

/// How many routines watch a fan's confirmed set point: the displays, the button, the MQTT brain
/// that restores the last running speed when Home Assistant turns the fans back on, and the sensor
/// polling that takes a fresh reading when the speed changes rather than waiting out its interval
const DISPLAY_STATE_RECEIVERS: usize = 4;
type DisplayStateWatch = Watch<CriticalSectionRawMutex, SetPoint, DISPLAY_STATE_RECEIVERS>;
type DisplayStateSender =
    watch::Sender<'static, CriticalSectionRawMutex, SetPoint, DISPLAY_STATE_RECEIVERS>;
type DisplayStateReceiver =
    watch::Receiver<'static, CriticalSectionRawMutex, SetPoint, DISPLAY_STATE_RECEIVERS>;

/// This routine takes the latest fan state updates and updates all parts of the device that display a state.
/// This includes at the time of writing Home Assistant through MQTT and two status LEDs on the device.
/// Displays fan status with 2 LEDs:
/// Off Off -> Fans Off
/// On Off -> Fan on low setting
/// Off On -> Fan on medium setting
/// On On -> Fan on high setting
#[embassy_executor::task]
async fn display_routine(
    mut display_state: (DisplayStateReceiver, DisplayStateReceiver),
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
            // Tell home assistant the fan turned on or off. The first update after boot is not a
            // change but still has to be reported, because home assistant has no idea yet
            // We already checked above if the new state is not the same as the current state
            if let Some(command) = current_display_state.0.map_or_else(
                || SetStateCommandValue::from_first(update),
                |current| SetStateCommandValue::from_change(current, update),
            ) {
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
            // Tell home assistant the fan turned on or off. The first update after boot is not a
            // change but still has to be reported, because home assistant has no idea yet
            // We already checked above if the new state is not the same as the current state
            if let Some(command) = current_display_state.1.map_or_else(
                || SetStateCommandValue::from_first(update),
                |current| SetStateCommandValue::from_change(current, update),
            ) {
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

impl From<bool> for SetStateCommandValue {
    fn from(is_on: bool) -> Self {
        if is_on {
            SetStateCommandValue::On
        } else {
            SetStateCommandValue::Off
        }
    }
}

impl From<SetPoint> for SetStateCommandValue {
    fn from(speed: SetPoint) -> Self {
        if speed == SetPoint::ZERO {
            SetStateCommandValue::Off
        } else {
            SetStateCommandValue::On
        }
    }
}

impl SetStateCommandValue {
    /// The state to report for a set point when there is no previous one to compare against, which
    /// is the first update after boot: not a change, but the state the fans were already in
    fn from_first(speed: SetPoint) -> Option<Self> {
        Some(Self::from(speed))
    }

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
    /// Close or open the relay module's contact
    RelayCommand(SetStateCommandValue),
}

enum FromPublishError {
    // Invalid fan command
    InvalidStringPayload,
    ParseSetPoint(ParseSetPointError),
    InvalidSetStateCommandPayload,

    UnknownTopic,
}

impl From<str::Utf8Error> for FromPublishError {
    fn from(_: str::Utf8Error) -> Self {
        FromPublishError::InvalidStringPayload
    }
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
                _other => Err(FromPublishError::InvalidSetStateCommandPayload),
            },
            topic::fan_controller::fan_1::percentage::COMMAND => {
                let payload = core::str::from_utf8(publish.payload)?;

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
                _other => Err(FromPublishError::InvalidSetStateCommandPayload),
            },
            topic::fan_controller::fan_2::percentage::COMMAND => {
                let payload = core::str::from_utf8(publish.payload)?;

                let set_point: SetPoint =
                    payload.parse().map_err(FromPublishError::ParseSetPoint)?;

                Ok(IncomingPublish::FanCommand {
                    target: Fan::Two,
                    command: FanCommand::SetSpeed { set_point },
                })
            }
            topic::fan_controller::relay::state::COMMAND => match publish.payload {
                b"ON" => Ok(Self::RelayCommand(SetStateCommandValue::On)),
                b"OFF" => Ok(Self::RelayCommand(SetStateCommandValue::Off)),
                _other => Err(FromPublishError::InvalidSetStateCommandPayload),
            },
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

/// The Home Assistant discovery payload, generated by `build.rs` from the `topic` constants and
/// baked into the binary. Published verbatim on boot
const DISCOVERY_PAYLOAD: &[u8] = env!("FAN_CONTROLLER_DISCOVERY_PAYLOAD").as_bytes();

/// What a publish packet puts in front of its payload: the fixed header, the remaining length as a
/// variable byte integer at its longest, the topic name and the two bytes of its length, and the
/// property length. The discovery topic is the longest the controller publishes to, so this is the
/// largest publish header it encodes
const PUBLISH_HEADER: usize = 1 + 4 + 2 + topic::fan_controller::DISCOVERY.len() + 1;

/// Only the header goes through the send buffer — `mqtt::task::send` writes a publish's payload to
/// the socket straight from where it already lives, which for the discovery payload is flash. So
/// the payload can grow with every component added without the buffer having to grow with it, and
/// what is left to check is that the header still fits.
///
/// A packet that does not fit is refused by the encoder and only logged, which would leave a
/// device that runs perfectly well and is never discovered by Home Assistant. Failing the build
/// instead is the cheaper way to find out
// `core::assert` because `defmt::*` is glob imported and its `assert` is not const
const _: () = core::assert!(
    PUBLISH_HEADER <= mqtt::task::SEND_BUFFER_SIZE,
    "the Home Assistant discovery publish header no longer fits the MQTT send buffer"
);

enum OutgoingPublish {
    Discovery,
    /// Why the controller last reset. Published once per boot, retained, because the resets worth
    /// diagnosing happen when nobody is watching
    ResetCause(reset_cause::ResetCause),
    UpdateSpeed {
        fan: Fan,
        payload: UpdateSpeedPayload,
    },
    UpdateState {
        fan: Fan,
        payload: SetStateCommandValue,
    },
    /// All five values a fan reports about itself, as the one JSON object every one of its sensors
    /// reads. Owned rather than borrowed because it is built during a poll and outlives it in the
    /// channel
    UpdateSensors {
        fan: Fan,
        payload: heapless::String<{ fan::sensor::JSON_CAPACITY }>,
    },
    /// Where the relay's contact is, published only once the module has confirmed the write. The
    /// same rule the fans follow: what is reported is what the device did, not what it was asked
    UpdateRelayState(SetStateCommandValue),
}

impl Publish for OutgoingPublish {
    fn topic(&self) -> &str {
        match self {
            OutgoingPublish::Discovery => topic::fan_controller::DISCOVERY,
            OutgoingPublish::ResetCause(_) => topic::fan_controller::RESET_CAUSE,
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
            OutgoingPublish::UpdateSensors { fan: Fan::One, .. } => {
                topic::fan_controller::fan_1::sensor::STATE
            }
            OutgoingPublish::UpdateSensors { fan: Fan::Two, .. } => {
                topic::fan_controller::fan_2::sensor::STATE
            }
            OutgoingPublish::UpdateRelayState(_) => topic::fan_controller::relay::state::STATE,
        }
    }

    fn payload(&self) -> &[u8] {
        match self {
            OutgoingPublish::Discovery => DISCOVERY_PAYLOAD,
            OutgoingPublish::ResetCause(cause) => cause.as_str().as_bytes(),
            OutgoingPublish::UpdateSpeed { fan: _, payload } => {
                // set_point.0.to_be_bytes()
                payload.0.as_bytes()
            }
            OutgoingPublish::UpdateState { fan: _, payload } => match payload {
                SetStateCommandValue::On => b"ON",
                SetStateCommandValue::Off => b"OFF",
            },
            OutgoingPublish::UpdateSensors { fan: _, payload } => payload.as_bytes(),
            OutgoingPublish::UpdateRelayState(payload) => match payload {
                SetStateCommandValue::On => b"ON",
                SetStateCommandValue::Off => b"OFF",
            },
        }
    }

    fn is_retained(&self) -> bool {
        matches!(self, OutgoingPublish::ResetCause(_))
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
    fan_one_state: &'static SetPointSignal,
    fan_two_state: &'static SetPointSignal,
    relay_state: &'static RelayStateSignal,
    mut display_state: (DisplayStateReceiver, DisplayStateReceiver),
) {
    // Remembering the last speed the fans were running at for when Home Assistant turns the device
    // off and then on again. It comes from the confirmed state rather than from the commands that
    // arrive here, so it starts at the speed read back from the fans on boot and also picks up the
    // speeds set with the button. Zero is not remembered: it is the state that "on" restores from
    let mut last_fan_state = (SetPoint::ZERO, SetPoint::ZERO);
    loop {
        info!("[MQTT Brain] Waiting for new publish");
        let message = match select3(
            receiver_in.receive(),
            display_state.0.changed(),
            display_state.1.changed(),
        )
        .await
        {
            Either3::First(message) => message,
            Either3::Second(set_point) => {
                if set_point != SetPoint::ZERO {
                    last_fan_state.0 = set_point;
                }
                continue;
            }
            Either3::Third(set_point) => {
                if set_point != SetPoint::ZERO {
                    last_fan_state.1 = set_point;
                }
                continue;
            }
        };
        info!("[MQTT Brain] Received publish");

        let publish = match message {
            Err(error) => {
                match error {
                    FromPublishError::InvalidStringPayload => {
                        error!("Invalid UTF-8 payload");
                    }
                    FromPublishError::ParseSetPoint(_parse_set_point_error) => {
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
                    fan_one_state.signal(RequestedSetPoint::FromUser(set_point));
                    if is_synchronization_on {
                        fan_two_state.signal(RequestedSetPoint::FromUser(set_point));
                    }
                }
                Fan::Two => {
                    fan_two_state.signal(RequestedSetPoint::FromUser(set_point));
                    if is_synchronization_on {
                        fan_one_state.signal(RequestedSetPoint::FromUser(set_point));
                    }
                }
            },
            IncomingPublish::RelayCommand(new_state) => {
                relay_state.signal(matches!(new_state, SetStateCommandValue::On))
            }
            IncomingPublish::FanCommand {
                target,
                command: FanCommand::SetState(new_state),
            } => match target {
                Fan::One => match new_state {
                    SetStateCommandValue::On => {
                        fan_one_state.signal(RequestedSetPoint::FromUser(last_fan_state.0))
                    }
                    SetStateCommandValue::Off => {
                        fan_one_state.signal(RequestedSetPoint::FromUser(SetPoint::ZERO))
                    }
                },
                Fan::Two => match new_state {
                    SetStateCommandValue::On => {
                        fan_two_state.signal(RequestedSetPoint::FromUser(last_fan_state.1))
                    }
                    SetStateCommandValue::Off => {
                        fan_two_state.signal(RequestedSetPoint::FromUser(SetPoint::ZERO))
                    }
                },
            },
        }
    }
}

/// How often a modbus transaction is attempted before the fan counts as unreachable
const MAX_ATTEMPTS: u8 = 3;

/// Waits before the next attempt of a modbus transaction: the attempt number squared, times
/// 100 ms. [`MAX_ATTEMPTS`] keeps the attempt number small enough for this to stay well inside a
/// `u64` and inside a second or two
async fn back_off(attempt: u8) {
    Timer::after_millis(u64::from(attempt).pow(2) * 100).await;
}

/// Reads the set point a fan is currently running at.
///
/// Returns `None` when the fan stays silent, answers with a value outside the set point range, or
/// a set point is requested before the read succeeds. That leaves the state unknown, which
/// everything displaying or restoring a fan state already treats as its own case, so guessing here
/// would be worse than admitting it.
async fn read_set_point(
    modbus_mutex: &'static ModbusMutex,
    fan_address: modbus::device::Address,
    requested_set_point: &'static SetPointSignal,
    fan_identifier: &str,
) -> Option<SetPoint> {
    let function = modbus::function::ReadHoldingRegister::new(
        fan_address,
        fan::holding_register::REFERENCE_SET_POINT,
    );

    for attempt in 1..=MAX_ATTEMPTS {
        info!(
            "{} Reading the current set point from the fan",
            fan_identifier
        );
        let result = modbus_mutex
            .lock()
            .await
            .read_holding_register(&function)
            .await;

        match result {
            // The fan ignores the four least significant bits of a set point, so the value that
            // comes back can be slightly below the one that was written. That is close enough for
            // everything this state is used for and rounding it back up would invent precision
            Ok(value) => match SetPoint::new(value) {
                Ok(set_point) => {
                    info!(
                        "{} Fan is running at set point {}",
                        fan_identifier, *set_point
                    );
                    return Some(set_point);
                }
                Err(_error) => {
                    error!(
                        "{} Fan reports set point {} which is above the maximum of {}",
                        fan_identifier,
                        value,
                        fan::set_point::MAX
                    );
                    return None;
                }
            },
            Err(error) => error!(
                "{} Failed to read the current set point on attempt {}: {:?}",
                fan_identifier, attempt, error
            ),
        }

        // A fan that does not answer takes a timeout per attempt, which is long enough that a
        // command can arrive in the meantime. That command is a state worth more than the one
        // being read, so stop asking and let it be applied
        if requested_set_point.signaled() {
            info!(
                "{} A set point was requested while reading the current one. Applying that instead",
                fan_identifier
            );
            return None;
        }

        if attempt < MAX_ATTEMPTS {
            back_off(attempt).await;
        }
    }

    error!(
        "{} Giving up reading the current set point after {} attempts. The fan state stays unknown until it is set",
        fan_identifier, MAX_ATTEMPTS
    );
    None
}

/// Reads where the relay's contact is now.
///
/// Retried like any other transaction, and for one reason beyond the usual: the module greets the
/// line in ASCII whenever *it* is powered, which is not the same moment the controller boots, and a
/// greeting that collides with a request leaves no frame to find at all. The client steps over a
/// greeting that merely precedes an answer; a spoiled exchange needs the second attempt.
///
/// `None` when it cannot be reached, which is honest about not knowing rather than assuming the
/// contact is open. Nothing is published then, so Home Assistant shows the switch as unknown
/// instead of showing a guess
async fn read_relay_state(client: &mut RelayClient) -> Option<bool> {
    let function = modbus::function::ReadCoils::<{ relay::coil::COUNT }>::new(
        relay::address::RELAY,
        relay::coil::RELAY,
    );

    let mut attempt = 1;
    loop {
        match client.read_coils(&function).await {
            Ok(coils) => return Some(coils & relay::coil::RELAY_BIT != 0),
            Err(error) if attempt >= MAX_ATTEMPTS => {
                error!(
                    "{} Failed to read the contact after {} attempts: {:?}",
                    RELAY_IDENTIFIER, MAX_ATTEMPTS, error
                );
                return None;
            }
            Err(error) => {
                warn!(
                    "{} Failed to read the contact on attempt {}: {:?}",
                    RELAY_IDENTIFIER, attempt, error
                );
                attempt += 1;
                back_off(attempt).await;
            }
        }
    }
}

/// Drives the relay module on its own bus, and reports where its contact actually ended up.
///
/// It holds the client rather than sharing it, because nothing else is on this UART. That also
/// makes the retry simpler than the fans': there is no lock to release between attempts, and no
/// second device whose transaction is being held up
#[embassy_executor::task]
async fn relay_routine(
    mut client: RelayClient,
    requested_state: &'static RelayStateSignal,
    mqtt_out: channel::Sender<'static, CriticalSectionRawMutex, OutgoingPublish, CHANNEL_SIZE>,
) {
    // The module holds its contact while the controller resets, so where it is now is the state to
    // start from — the same reason the fans' speeds are read back rather than assumed
    let mut current_state = read_relay_state(&mut client).await;
    if let Some(is_closed) = current_state {
        info!("{} Contact is closed: {}", RELAY_IDENTIFIER, is_closed);
        mqtt_out.send(OutgoingPublish::UpdateRelayState(is_closed.into())).await;
    }

    'signal_loop: loop {
        info!("{} Waiting for a state to be requested", RELAY_IDENTIFIER);
        let is_closed = requested_state.wait().await;

        if current_state == Some(is_closed) {
            info!(
                "{} Requested state is the state it is already in",
                RELAY_IDENTIFIER
            );
            continue;
        }

        let function = modbus::function::WriteSingleCoil::new(
            relay::address::RELAY,
            relay::coil::RELAY,
            is_closed,
        );

        let mut attempt = 1;
        loop {
            match client.write_single_coil(&function).await {
                Ok(()) => break,
                Err(error) if attempt >= MAX_ATTEMPTS => {
                    error!(
                        "{} Failed to write the contact after {} attempts: {:?}",
                        RELAY_IDENTIFIER, MAX_ATTEMPTS, error
                    );

                    // The write may or may not have been carried out — a module that stops
                    // answering partway through an exchange has still received the request. Saying
                    // the state is unknown makes the next command attempt the write rather than
                    // skip it as already satisfied, and leaves Home Assistant showing the last
                    // state that was actually confirmed
                    current_state = None;
                    continue 'signal_loop;
                }
                Err(error) => {
                    warn!(
                        "{} Failed to write the contact on attempt {}: {:?}",
                        RELAY_IDENTIFIER, attempt, error
                    );
                    attempt += 1;

                    // A newer request supersedes this one rather than being made to wait behind it
                    if requested_state.signaled() {
                        continue 'signal_loop;
                    }

                    back_off(attempt).await;
                }
            }
        }

        info!("{} Contact is now closed: {}", RELAY_IDENTIFIER, is_closed);
        current_state = Some(is_closed);
        mqtt_out.send(OutgoingPublish::UpdateRelayState(is_closed.into())).await;
    }
}

/// Receives the fan state updates and sends them to modbus as modbus messages
/// After a successful response, this sends an update to the fan display logic unit
#[embassy_executor::task(pool_size = 2)]
async fn fan_control_routine(
    fan_address: modbus::device::Address,
    current_fan_speed: &'static SetPointSignal,
    other_fan_speed: &'static SetPointSignal,
    modbus: &'static ModbusOnceLock,
    display_state: DisplayStateSender,
) {
    let fan_identifier = match *fan_address {
        2 => "[Fan 1]",
        3 => "[Fan 2]",
        _other => "Unknown (oops)",
    };

    info!("{} Waiting for MODBUS initialization", fan_identifier);
    let modbus_mutex = modbus.get().await;
    info!("{} MODBUS initialized", fan_identifier);

    // The fans keep running while the controller resets, so whatever they are set to now is the
    // state to start from. Reading it back is what lets the LEDs, Home Assistant and the button
    // describe what is actually happening instead of assuming the fans are off.
    // Stays `None` when the fan cannot be reached, which is honest about not knowing rather than
    // guessing at a speed
    let mut current_set_point =
        read_set_point(modbus_mutex, fan_address, current_fan_speed, fan_identifier).await;
    if let Some(set_point) = current_set_point {
        display_state.send(set_point);
    }
    'signal_loop: loop {
        info!("{} Waiting for fan state update", fan_identifier);
        let mut request = current_fan_speed.wait().await;
        if current_set_point.is_some_and(|speed| speed == request.set_point()) {
            //TODO consider to update fan display state nonetheless
            info!(
                "{} Fan state update received but has same state",
                fan_identifier
            );
            continue;
        }

        info!("{} Received fan state {:?}", fan_identifier, request);

        // Instruct modbus to send update
        info!("{} Attempting to acquire lock (again?)", fan_identifier);
        let mut modbus = modbus_mutex.lock().await;
        info!("{} Acquired lock on modbus (again?)", fan_identifier);
        // Check we have the latest state in case it was updated while waiting for the lock
        request = current_fan_speed.try_take().unwrap_or(request);
        let set_point = request.set_point();

        let function = modbus::function::WriteHoldingRegister::new(
            fan_address,
            fan::holding_register::REFERENCE_SET_POINT,
            *set_point,
        );

        info!("{} Sending fan state update through modbus", fan_identifier);
        let mut attempt = 1;
        while let Err(error) = modbus.write_holding_register(&function).await
            && attempt <= MAX_ATTEMPTS
        {
            // Release lock so other tasks get a chance to access modbus for sending messages to devices
            drop(modbus);

            error!(
                "{} Failed to send fan state update with attempt {}: {:?}",
                fan_identifier, attempt, error
            );
            attempt += 1;

            if current_fan_speed.signaled() {
                continue 'signal_loop;
            }

            back_off(attempt).await;
            info!("{} Waiting for lock on modbus for retry", fan_identifier);
            modbus = modbus_mutex.lock().await;
            info!("{} Acquired lock on modbus for retry", fan_identifier);
        }

        if attempt > MAX_ATTEMPTS {
            error!(
                "{} Failed to send fan state update after {} attempts",
                fan_identifier, MAX_ATTEMPTS
            );

            if !request.should_correct_other_fan() {
                // This was already the other fan pushing this one back. Pushing back a second time
                // is what used to bounce the same correction between the two fans forever
                info!(
                    "{} Leaving the other fan alone because this was its own correction",
                    fan_identifier
                );
                continue;
            }

            //TODO don't try to update other fan speed if we have a setting to allow fans to run out of sync
            // Set other fan to current fan speed to avoid them getting out of sync and creating over or underpressure in the house
            // There is no Option::copied or Option::cloned for some reason in core
            current_set_point
                .inspect(|speed| other_fan_speed.signal(RequestedSetPoint::FromOtherFan(*speed)));
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

/// How often each fan is asked what it is measuring. One poll is two modbus transactions, which
/// at 19_200 baud is roughly 60 ms of bus time per fan, so this is about half a percent of it.
/// Slow enough that a speed change never waits long behind a poll, quick enough that a fan warming
/// up is visible in Home Assistant while it happens
const SENSOR_POLL_INTERVAL: Duration = Duration::from_secs(30);

/// How often to read a fan that is on its way to a newly commanded speed.
///
/// The fans do not step to a speed, they travel to it, and how long that takes depends on how far
/// they are going — so there is no one delay that is right for both a nudge and a change across
/// the range. Rather than pick one, this reads quickly and lets `fan::sensor::Settling` say when
/// the flow has stopped moving.
///
/// This is only how finely the ramp is sampled: what counts as settled is a duration, and
/// `Settling::at_interval` derives its counts from this, so changing it changes the resolution and
/// nothing else. One poll is two reads, 68 bytes, which at 19_200 baud 8E1 is 39 ms of line time
/// per fan — so even both fans at this cadence leave the bus idle about 96 % of the time. A second
/// is the floor worth going to: below that the publishes cost more than the detail is worth
const SENSOR_SETTLE_POLL_MILLISECONDS: u32 = 2_000;
const SENSOR_SETTLE_POLL_INTERVAL: Duration =
    Duration::from_millis(SENSOR_SETTLE_POLL_MILLISECONDS as u64);

/// How long to leave the bus alone before the first poll, so the set point both fan control
/// routines read on boot — with retries, and a timeout each if a fan is silent — is done first.
/// The fans are the reason the controller exists; what they report about themselves can wait
const SENSOR_POLL_STARTUP_DELAY: Duration = Duration::from_secs(10);

/// Polls one fan for what it reports about itself and publishes it to Home Assistant.
///
/// Nothing on the device acts on these values, so a failed poll is logged and dropped rather than
/// retried: the next poll is along in [`SENSOR_POLL_INTERVAL`] and carries fresher values than a
/// retry would. That also keeps a fan that has stopped answering from holding the modbus mutex
/// through a run of timeouts while a speed change waits behind it.
///
/// Between intervals it also watches the fan's *confirmed* set point, so a speed change is
/// followed by readings rather than by up to [`SENSOR_POLL_INTERVAL`] of stale numbers. The
/// confirmed one rather than the requested one, because by then the fan has answered the write and
/// this poll cannot be queueing behind it for the modbus mutex.
///
/// A change is followed at [`SENSOR_SETTLE_POLL_INTERVAL`] until the flow stops moving, which is
/// `fan::sensor::Settling`'s decision rather than this routine's — a tolerance and a run length are
/// exactly the kind of rule that looks obviously right and is not, and nothing in this crate can be
/// tested
#[embassy_executor::task(pool_size = 2)]
async fn sensor_routine(
    fan: Fan,
    fan_address: modbus::device::Address,
    modbus: &'static ModbusOnceLock,
    mqtt_out: channel::Sender<'static, CriticalSectionRawMutex, OutgoingPublish, CHANNEL_SIZE>,
    mut display_state: DisplayStateReceiver,
) {
    let fan_identifier = match fan {
        Fan::One => "[Fan 1 sensors]",
        Fan::Two => "[Fan 2 sensors]",
    };

    let modbus_mutex = modbus.get().await;
    Timer::after(SENSOR_POLL_STARTUP_DELAY).await;

    // Every speed the fan reports is a fraction of this, so without it a speed cannot be turned
    // into a rate at all. It only changes when the fan is reconfigured, so it is read once and
    // then kept, and retried on the next poll for as long as it is not known
    let mut maximum_speed: Option<u16> = None;
    // Present while a confirmed speed change is being followed, absent at the ordinary cadence
    let mut settling: Option<fan::sensor::Settling> = None;

    loop {
        if maximum_speed.is_none() {
            let function = modbus::function::ReadHoldingRegister::new(
                fan_address,
                fan::holding_register::MAXIMUM_SPEED,
            );

            match modbus_mutex
                .lock()
                .await
                .read_holding_register(&function)
                .await
            {
                Ok(value) => {
                    info!("{} Fan's maximum speed is {} rpm", fan_identifier, value);
                    maximum_speed = Some(value);
                }
                // Not fatal: the other four values do not depend on it, and the reading reports
                // the speed as unknown until it can be read
                Err(error) => warn!(
                    "{} Failed to read the fan's maximum speed: {:?}",
                    fan_identifier, error
                ),
            }
        }

        let status_request = modbus::function::ReadInputRegisters::<
            { fan::sensor::STATUS_LENGTH },
        >::new(fan_address, fan::input_register::STATUS);
        let power_and_air_request = modbus::function::ReadInputRegisters::<
            { fan::sensor::POWER_AND_AIR_LENGTH },
        >::new(fan_address, fan::input_register::POWER_AND_AIR);

        // Both runs are read under one lock so every value describes the same moment. It costs
        // a speed change at most the two transactions rather than one, which is still well under a
        // tenth of a second
        let mut client = modbus_mutex.lock().await;
        let reading = match client.read_input_registers(&status_request).await {
            Ok(status) => match client.read_input_registers(&power_and_air_request).await {
                Ok(power_and_air) => {
                    Some(fan::sensor::decode(&status, &power_and_air, maximum_speed))
                }
                Err(error) => {
                    warn!(
                        "{} Failed to read the power and air registers: {:?}",
                        fan_identifier, error
                    );
                    None
                }
            },
            Err(error) => {
                warn!(
                    "{} Failed to read the status registers: {:?}",
                    fan_identifier, error
                );
                None
            }
        };
        drop(client);

        // Taken before the reading is turned into a payload, because the settling watcher below
        // needs it whether or not the publish gets anywhere
        let volume_flow = reading.map(|reading| reading.volume_flow);

        if let Some(reading) = reading {
            info!("{} Read {:?}", fan_identifier, reading);

            let publish = OutgoingPublish::UpdateSensors {
                fan,
                payload: reading.to_json(),
            };

            // Dropped rather than waited on, like the other display updates: only the latest
            // reading is worth anything, and the next one is along in SENSOR_POLL_INTERVAL
            if let Err(channel::TrySendError::Full(_publish)) = mqtt_out.try_send(publish) {
                error!(
                    "{} MQTT out channel is full, dropping this reading",
                    fan_identifier
                );
            }
        }

        // Only meaningful while following a speed change, and a failed poll deliberately still
        // counts as a turn there — see `Settling::MAX_READINGS`
        let keep_reading_quickly = match settling.as_mut() {
            None => false,
            Some(state) => match state.observe(volume_flow) {
                fan::sensor::Progress::Moving => true,
                fan::sensor::Progress::Settled => {
                    info!(
                        "{} Fan settled at {:?} m³/h, back to the interval",
                        fan_identifier, volume_flow
                    );
                    false
                }
                fan::sensor::Progress::GaveUp => {
                    warn!(
                        "{} Fan has not held a steady flow after {:?} readings, back to the \
                         interval anyway",
                        fan_identifier,
                        state.readings_taken()
                    );
                    false
                }
            },
        };

        if keep_reading_quickly {
            Timer::after(SENSOR_SETTLE_POLL_INTERVAL).await;
            continue;
        }

        settling = None;

        // Whichever comes first: the interval, or the fan confirming a new speed. A change during
        // a poll is not lost — the watch keeps the latest value until this receiver has seen it,
        // so `changed()` returns straight away
        match select(Timer::after(SENSOR_POLL_INTERVAL), display_state.changed()).await {
            Either::First(()) => {}
            Either::Second(set_point) => {
                info!(
                    "{} Fan confirmed {:?}, following it to the new speed rather than waiting out \
                     the interval",
                    fan_identifier, set_point
                );
                settling = Some(fan::sensor::Settling::at_interval(SENSOR_SETTLE_POLL_MILLISECONDS));
            }
        }
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

#[derive(Clone, Copy)]
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

/// Plays one cycle of the animation that displays `state` on the LEDs.
/// The caller repeats this for as long as the state stays the same.
///
/// States that are a static picture rather than an animation never return. They rely on the
/// caller cancelling them once a new state arrives. Returning instead would put the caller
/// into an endless loop without an await which starves every other task on the executor.
async fn animate<'a, 'b, T: Pin, U: Pin>(
    led_1: &mut Output<'a, T>,
    led_2: &mut Output<'b, U>,
    state: Option<LedState>,
) {
    match state {
        // Using a quick switch between states as something moving faster gives the illusion of loading faster
        None => {
            const PAUSE_TIME: u64 = 250;
            Timer::after_millis(PAUSE_TIME).await;
            led_2.set_low();
            led_1.set_high();
            Timer::after_millis(PAUSE_TIME).await;
            led_1.set_low();
            led_2.set_high();
        }
        Some(LedState::Synchronized {
            led_1: led_1_level,
            led_2: led_2_level,
        }) => {
            led_1.set_level(led_1_level);
            led_2.set_level(led_2_level);
            // Nothing to animate. Park until the caller cancels us with the next state.
            core::future::pending::<()>().await
        }
        Some(LedState::Unsynchronized {
            led_1: led_1_state,
            led_2: led_2_state,
        }) => {
            /*
             * Note: blink counts can be the same but that does not mean the fan state is the same.
             * The blink counts just represent if the fan is running within a certain range (low, medium, high).
             * Within these ranges, the fan state can be different.
             */

            // Switching between blinking the state of one LED/fan and only then the other is intended
            // to make it clearer that they don't run the same
            blink(led_1, led_1_state).await;
            Timer::after_secs(5).await;
            blink(led_2, led_2_state).await;
            Timer::after_secs(5).await;
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
        // Taking the next state and yielding to the executor both happen here and nowhere
        // else. That keeps every animation interruptible and makes it impossible for one
        // of them to starve the executor by looping without an await.
        // Signal::wait is cancellation safe so an animation that gets cancelled halfway
        // through just starts over with the new state.
        match select(
            led_state.wait(),
            animate(&mut led_1, &mut led_2, current_state),
        )
        .await
        {
            Either::First(new_state) => current_state = Some(new_state),
            // The animation played to the end without an update coming in so repeat it
            Either::Second(()) => {}
        }
    }
}
const CHANNEL_SIZE: usize = 8;

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Read before anything else touches the chip, so nothing here can be mistaken for the reason
    // the last boot ended
    let boot_reset_cause = reset_cause::read();

    let Peripherals {
        PIN_23: pin_23,
        PIN_25: pin_25,
        PIO0: pio0,
        DMA_CH0: dma_ch0,
        PIN_24: pin_24,
        PIN_29: pin_29,
        // Driver enable/disable pin to switch between sending and receiving data on UART/Modbus
        PIN_4: pin_4,
        UART0: uart0,
        // Transmitter pin UART + Modbus
        PIN_12: pin_12,
        // Receiver pin UART + Modbus
        PIN_13: pin_13,
        // The relay module's bus. UART1's transmit and receive can only be GP8 and GP9 here: its
        // other pairs are GP4, which is the fans' driver enable, and GP20/GP21, which are the
        // status LEDs. GP0 and GP1 are left alone as well, since that is where a debug probe's
        // UART bridge is conventionally wired
        UART1: uart1,
        // Driver enable for the relay's transceiver, next to the pair it belongs with
        PIN_7: pin_7,
        // Transmitter pin UART + Modbus, relay
        PIN_8: pin_8,
        // Receiver pin UART + Modbus, relay
        PIN_9: pin_9,
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
    /// Receive buffer for UART. Large enough for the longest response the fan sends, which is the
    /// run of input registers a sensor poll reads rather than the eight byte echo of a write
    static RX_BUFFER: StaticCell<[u8; 64]> = StaticCell::new();
    let rx_buffer = &mut RX_BUFFER.init([0; 64])[..];

    let client = modbus::client::Client::new(
        uart0,
        pin_12,
        pin_13,
        Irqs,
        pin_4,
        tx_buffer,
        rx_buffer,
        fan::get_configuration(),
    );

    static FANS: ModbusOnceLock = ModbusOnceLock::new();
    // Just initialize it
    _ = FANS.get_or_init(|| client.into());

    // The relay's bus. Its buffers are its own: two devices on two UARTs cannot share one, and the
    // longest frame either direction carries here is the eight byte coil write and its echo
    static RELAY_TX_BUFFER: StaticCell<[u8; 16]> = StaticCell::new();
    let relay_tx_buffer = &mut RELAY_TX_BUFFER.init([0; 16])[..];
    /// Larger than any answer the module sends, because what arrives is not always an answer: it
    /// greets the line with 49 bytes of ASCII whenever it is powered, and room to take that in is
    /// what lets the client step over it rather than read it as a frame
    static RELAY_RX_BUFFER: StaticCell<[u8; 64]> = StaticCell::new();
    let relay_rx_buffer = &mut RELAY_RX_BUFFER.init([0; 64])[..];

    let relay_client: RelayClient = modbus::client::Client::new(
        uart1,
        pin_8,
        pin_9,
        Irqs,
        pin_7,
        relay_tx_buffer,
        relay_rx_buffer,
        relay::get_configuration(),
    );

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
    static FAN_ONE_DISPLAY_STATE: DisplayStateWatch = Watch::new();
    static FAN_TWO_DISPLAY_STATE: DisplayStateWatch = Watch::new();

    let display_receivers = (
        FAN_ONE_DISPLAY_STATE
            .receiver()
            .expect("Expected the watch to be configured for DISPLAY_STATE_RECEIVERS receivers"),
        FAN_TWO_DISPLAY_STATE
            .receiver()
            .expect("Expected the watch to be configured for DISPLAY_STATE_RECEIVERS receivers"),
    );
    let sender_out = OUT.sender();

    info!("[Main] Seinding discovery");
    sender_out.send(OutgoingPublish::Discovery).await;
    info!("[Main] Sent out discocery");

    info!("[Main] Reset cause: {}", boot_reset_cause);
    sender_out
        .send(OutgoingPublish::ResetCause(boot_reset_cause))
        .await;
    unwrap!(spawner.spawn(display_routine(display_receivers, &LED_STATE, sender_out)));

    static FAN_ONE_STATE: SetPointSignal = Signal::new();
    static FAN_TWO_STATE: SetPointSignal = Signal::new();

    let button_receivers = (
        FAN_ONE_DISPLAY_STATE
            .receiver()
            .expect("Expected the watch to be configured for DISPLAY_STATE_RECEIVERS receivers"),
        FAN_TWO_DISPLAY_STATE
            .receiver()
            .expect("Expected the watch to be configured for DISPLAY_STATE_RECEIVERS receivers"),
    );
    unwrap!(spawner.spawn(input_routine(
        pin_18,
        button_receivers,
        (&FAN_ONE_STATE, &FAN_TWO_STATE)
    )));

    let brain_receivers = (
        FAN_ONE_DISPLAY_STATE
            .receiver()
            .expect("Expected the watch to be configured for DISPLAY_STATE_RECEIVERS receivers"),
        FAN_TWO_DISPLAY_STATE
            .receiver()
            .expect("Expected the watch to be configured for DISPLAY_STATE_RECEIVERS receivers"),
    );

    static RELAY_STATE: RelayStateSignal = Signal::new();

    let receiver_in = IN.receiver();
    unwrap!(spawner.spawn(mqtt_brain_routine(
        receiver_in,
        &FAN_ONE_STATE,
        &FAN_TWO_STATE,
        &RELAY_STATE,
        brain_receivers
    )));

    unwrap!(spawner.spawn(relay_routine(
        relay_client,
        &RELAY_STATE,
        OUT.sender()
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

    unwrap!(spawner.spawn(sensor_routine(
        Fan::One,
        fan::address::FAN_1,
        &FANS,
        OUT.sender(),
        FAN_ONE_DISPLAY_STATE
            .receiver()
            .expect("Expected the watch to be configured for DISPLAY_STATE_RECEIVERS receivers"),
    )));
    unwrap!(spawner.spawn(sensor_routine(
        Fan::Two,
        fan::address::FAN_2,
        &FANS,
        OUT.sender(),
        FAN_TWO_DISPLAY_STATE
            .receiver()
            .expect("Expected the watch to be configured for DISPLAY_STATE_RECEIVERS receivers"),
    )));
}
