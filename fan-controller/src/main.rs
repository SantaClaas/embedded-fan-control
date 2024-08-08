#![no_std]
#![no_main]
#![allow(warnings)]

use crc::{Crc, CRC_16_MODBUS};
use cyw43::{Control, NetDriver};
use cyw43_pio::PioSpi;
use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::join::join;
use embassy_net::{Config, IpAddress, IpEndpoint, Stack, StackResources, tcp};
use embassy_net::dns::{DnsQueryType, DnsSocket};
use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use embassy_rp::{bind_interrupts, dma, Peripheral, Peripherals, pio, uart};
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Input, Level, Output, Pin, Pull};
use embassy_rp::peripherals::{
    DMA_CH0, PIN_12, PIN_13, PIN_18, PIN_23, PIN_24, PIN_25, PIN_29, PIN_4, PIO0, UART0,
};
use embassy_rp::pio::{InterruptHandler, Pio, PioPin};
use embassy_rp::uart::Uart;
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::channel;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embassy_sync::pubsub::PubSubChannel;
use embassy_sync::signal::Signal;
use embassy_time::{Duration, Instant, Ticker, Timer};
use embedded_io_async::{Read, Write};
use embedded_nal_async::{AddrType, Dns, SocketAddr, TcpConnect};
use rand::RngCore;
use reqwless::client::{TlsConfig, TlsVerify};
use static_cell::StaticCell;

use {defmt_rtt as _, panic_probe as _};

use crate::mqtt::{connect, publish, subscribe};
use crate::mqtt::{Packet, QualityOfService};
use crate::mqtt::connect::Connect;
use crate::mqtt::connect_acknowledgement::ConnectReasonCode;
use crate::mqtt::ping_request::PingRequest;
use crate::mqtt::publish::Publish;
use crate::mqtt::subscribe::{Subscribe, Subscription};

mod fan;
mod modbus;
mod mqtt;

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

//TODO make configurable
/// Don't put credentials in the source code
const WIFI_NETWORK: &str = ""; //  env!("FAN_CONTROL_WIFI_NETWORK");

//TODO make configurable
/// Don't put credentials in the source code
const WIFI_PASSWORD: &str = ""; //env!("FAN_CONTROL_WIFI_PASSWORD");

//TODO make configurable
const MQTT_BROKER_ADDRESS: &str = "homeassistant.local";

//TODO make configurable
const MQTT_BROKER_PORT: u16 = 1883;

//TODO make configurable
/// The broker IP address can be configured manually. It will be used instead of the [MQTT_BROKER_ADDRESS]
/// if it is set as it does not require DNS resolution.
const MQTT_BROKER_IP_ADDRESS: Option<IpAddress> = None;

//TODO make configurable
const MQTT_BROKER_USERNAME: &str = "";

//TODO make configurable
const MQTT_BROKER_PASSWORD: &[u8] = b"";

//TODO make configurable
/// Prefix is "homeassistant", but it can be changed in home assistant configuration
const DISCOVERY_TOPIC: &str = "homeassistant/fan/testfan/config";

#[embassy_executor::task]
async fn network_task(stack: &'static Stack<NetDriver<'static>>) -> ! {
    stack.run().await
}

enum State {
    Connecting,
    Connected,
}

type StateSignal = Signal<CriticalSectionRawMutex, State>;
static STATE: StateSignal = StateSignal::new();

enum MqttError {
    WriteConnectError(connect::WriteError),
    WriteError(tcp::Error),
    FlushError(tcp::Error),
    ReadError(tcp::Error),
    ReadPacketError(mqtt::ReadError),
    ConnectError(mqtt::ConnectErrorReasonCode),
    WriteSubscribeError(subscribe::WriteError),
    UnexpectedPacketType(u8),
    WritePublishError(publish::WriteError),
}

async fn mqtt_receive<'a>(reader: TcpReader<'a>) {}

async fn mqtt_task(
    spawner: Spawner,
    net_device: NetDriver<'static>,
    control: &mut Control<'static>,
) -> () {
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
        match control.join_wpa2(WIFI_NETWORK, WIFI_PASSWORD).await {
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

    let state = StateSignal::new();

    // Now we can use it
    // loop {
    let client_state = TcpClientState::<1, 1024, 1024>::new();
    let mut receive_buffer = [0; 1024];
    let mut send_buffer = [0; 1024];
    let mut socket = TcpSocket::new(stack, &mut receive_buffer, &mut send_buffer);

    let dns_client = DnsSocket::new(stack);

    // Get home assistant MQTT broker IP address
    let address = loop {
        //TODO support IPv6
        let result = dns_client.query(MQTT_BROKER_ADDRESS, DnsQueryType::A).await;

        let mut addresses = match result {
            Ok(addresses) => addresses,
            Err(error) => {
                info!(
                    "Error resolving Home Assistant MQTT broker IP address: {:?}",
                    error
                );
                // Exponential backoff doesn't seem necessary here
                // Maybe the current installation of Home Assistant is in the process of
                // being set up and the entry is not yet available
                Timer::after_millis(500).await;
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

    // let address = SocketAddr::new(address, MQTT_BROKER_PORT);
    let endpoint = IpEndpoint::new(address, MQTT_BROKER_PORT);
    // Connect
    while let Err(error) = socket.connect(endpoint).await {
        info!(
            "Error connecting to Home Assistant MQTT broker: {:?}",
            error
        );
        Timer::after_millis(500).await;
    }

    // }
}

enum PublishReceiveError {
    ReadError(tcp::Error),
    ReadPacketError(mqtt::ReadError),
}
async fn listen_for_publishes(
    mut reader: TcpReader<'_>,
    mut buffer: [u8; 1024],
) -> Result<(), PublishReceiveError> {
    loop {
        // Should read at least one byte
        let bytes_read = reader
            .read(&mut buffer)
            .await
            .map_err(PublishReceiveError::ReadError)?;

        let packet =
            Packet::read(&buffer[..bytes_read]).map_err(PublishReceiveError::ReadPacketError)?;
        match packet {
            //TODO handle disconnect packet
            packet @ Packet::ConnectAcknowledgement(_)
            | packet @ Packet::SubscribeAcknowledgement(_) => {
                warn!("Received unexpected packet type: {}", packet.get_type());
            }
            Packet::Publish(publish) => {
                info!("Received publish: {:?}", publish);
                // This part is not MQTT and application specific
                match publish.topic_name {
                    "testfan/speed/percentage" => {
                        let payload = match core::str::from_utf8(&publish.payload) {
                            Ok(payload) => payload,
                            Err(error) => {
                                warn!("Expected percentage_command_topic payload (speed percentage) to be a valid UTF-8 string with a number");
                                continue;
                            }
                        };

                        // And then to an integer...
                        let set_point = payload.parse::<u16>();
                        let set_point = match set_point {
                            Ok(set_point) => set_point,
                            Err(error) => {
                                warn!("Expected speed percentage to be a number string. Payload is: {}", payload);
                                continue;
                            }
                        };

                        let Ok(setting) = FanSetting::new(set_point) else {
                            warn!(
                                "Setting fan speed out of bounds. Not accepting new setting: {}",
                                set_point
                            );
                            continue;
                        };

                        //TODO confirm there was no error on the modbus side
                        MODBUS_SIGNAL.signal(setting);
                    }
                    "testfan/on/set" => {
                        let payload = match core::str::from_utf8(publish.payload) {
                            Ok(payload) => payload,
                            Err(error) => {
                                warn!(
                                    "Expected command_topic payload to be a valid UTF-8 string with either \"ON\" or \"OFF\". Payload is: {}",
                                    publish.payload
                                );
                                continue;
                            }
                        };

                        match payload {
                            "ON" => {
                                //TODO set to last known state before off
                                println!("Turning on");
                            }
                            "OFF" => {
                                println!("Turning off");
                                MODBUS_SIGNAL.signal(FanSetting::ZERO);
                            }
                            unknown => {
                                warn!(
                                    "Expected either \"ON\" or \"OFF\" but received: {}",
                                    unknown
                                );
                            }
                        }
                    }
                    unknown => {
                        warn!(
                            "Unexpected topic: {} with payload: {}",
                            publish.topic_name, publish.payload
                        );
                    }
                }
            }
        }
    }
}

async fn send_discovery_and_keep_alive(
    mut writer: TcpWriter<'_>,
    mut send_buffer: [u8; 256],
    mut keep_alive: Ticker,
) -> Result<(), MqttError> {
    // Send discovery packet
    // Configuration is like the YAML configuration that would be added in Home Assistant but as JSON
    // Command topic: The MQTT topic to publish commands to change the state of the fan
    //TODO set firmware version from Cargo.toml package version
    //TODO think about setting hardware version, support url, and manufacturer
    //TODO create single home assistant device with multiple entities for sensors in fan and the bypass
    //TODO add diagnostic entity like IP address
    //TODO availability topic
    //TODO remove whitespace at compile time through macro, build script or const fn

    // Using abbreviations to save space of binary and on the wire (haven't measured effect though...)
    // name -> name
    // uniq_id -> unique_id
    // stat_t -> state_topic
    // cmd_t -> command_topic
    // pct_stat_t -> percentage_state_topic
    // pct_cmd_t -> percentage_command_topic
    // spd_rng_max -> speed_range_max
    // Don't need to set speed_range_min because it is 1 by default
    const DISCOVERY_PAYLOAD: &[u8] = br#"{
        "name": "Fan",
        "uniq_id": "testfan",
        "stat_t": "testfan/on/state",
        "cmd_t": "testfan/on/set",
        "pct_stat_t": "testfan/speed/percentage_state",
        "pct_cmd_t": "testfan/speed/percentage",
        "spd_rng_max": 64000
    }"#;

    const DISCOVERY_PUBLISH: Publish = Publish {
        topic_name: DISCOVERY_TOPIC,
        payload: DISCOVERY_PAYLOAD,
    };

    let mut offset = 0;
    DISCOVERY_PUBLISH
        .write(&mut send_buffer, &mut offset)
        .map_err(MqttError::WritePublishError)?;

    writer
        .write_all(&send_buffer[..offset])
        .await
        .map_err(MqttError::WriteError)?;
    writer.flush().await.map_err(MqttError::FlushError)?;

    keep_alive.reset();
    // No acknowledgement to read for quality of service 0

    loop {
        keep_alive.next().await;
        // Send keep alive ping request
        PingRequest.write(&mut send_buffer, &mut offset);
        writer
            .write_all(&send_buffer[..offset])
            .await
            .map_err(MqttError::WriteError)?;
        writer.flush().await.map_err(MqttError::FlushError)?;
        //TODO read ping response or disconnect after "a reasonable amount of time"
    }
}

const KEEP_ALIVE: Duration = Duration::from_secs(60);
const _: () = {
    // Check if the representation as u16 is correct
    let seconds = KEEP_ALIVE.as_secs() as u16;

    core::assert!(seconds == 60);
};

async fn mqtt_routine<'a>(mut socket: TcpSocket<'a>) -> Result<(), MqttError> {
    // No experience how big this buffer should be
    let mut send_buffer = [0; 256];
    // Send MQTT connect packet
    let packet = Connect {
        client_identifier: "testfan",
        username: MQTT_BROKER_USERNAME,
        password: MQTT_BROKER_PASSWORD,
        keep_alive_seconds: KEEP_ALIVE.as_secs() as u16,
    };

    let mut offset = 0;
    packet
        .write(&mut send_buffer, &mut offset)
        .map_err(MqttError::WriteConnectError)?;

    socket
        .write_all(&send_buffer[..offset])
        .await
        .map_err(MqttError::WriteError)?;
    socket.flush().await.map_err(MqttError::FlushError)?;

    // Wait for connection acknowledgement
    let mut receive_buffer = [0; 1024];
    let bytes_read = socket
        .read(&mut receive_buffer)
        .await
        .map_err(MqttError::ReadError)?;
    let packet = Packet::read(&receive_buffer[..bytes_read]).map_err(MqttError::ReadPacketError)?;

    match packet {
        Packet::ConnectAcknowledgement(acknowledgement) => {
            if let ConnectReasonCode::ErrorCode(error_code) = acknowledgement.connect_reason_code {
                return Err(MqttError::ConnectError(error_code));
            }
        }
        unexpected => {
            // Unexpected packet type -> disconnect
            info!("Unexpected packet");
            return Err(MqttError::UnexpectedPacketType(unexpected.get_type()));
        }
    }

    // Start keep alive after connect acknowledgement?
    //TODO use keep alive interval received from server in connect acknowledgement
    let mut keep_alive = Ticker::every(KEEP_ALIVE);

    // Subscribe to home assistant topics
    // Before sending discovery packet
    let subscriptions = [
        Subscription {
            topic_filter: "testfan/on/set",
            options: mqtt::subscribe::Options::new(
                QualityOfService::AtMostOnceDelivery,
                false,
                false,
                mqtt::subscribe::RetainHandling::DoNotSend,
            ),
        },
        Subscription {
            topic_filter: "testfan/speed/percentage",
            options: mqtt::subscribe::Options::new(
                QualityOfService::AtMostOnceDelivery,
                false,
                false,
                mqtt::subscribe::RetainHandling::DoNotSend,
            ),
        },
    ];

    //TODO packet identifier management
    let packet = Subscribe {
        subscriptions,
        packet_identifier: 42,
    };

    offset = 0;
    let mut last_send = Instant::now();
    packet
        .write(&mut send_buffer, &mut offset)
        .map_err(MqttError::WriteSubscribeError)?;
    socket
        .write_all(&send_buffer[..offset])
        .await
        .map_err(MqttError::WriteError)?;
    socket.flush().await.map_err(MqttError::FlushError)?;
    keep_alive.reset();

    // Read subscribe acknowledgement
    let bytes_read = socket
        .read(&mut receive_buffer)
        .await
        .map_err(MqttError::ReadError)?;

    let packet = Packet::read(&receive_buffer[..bytes_read]).map_err(MqttError::ReadPacketError)?;
    match packet {
        Packet::SubscribeAcknowledgement(_acknowledgement) => {
            // Ignoring acknowledgement errors for now as their information is only useful for debugging currently
        }
        unexpected => {
            // Unexpected packet type -> disconnect
            info!("Unexpected packet");

            return Err(MqttError::UnexpectedPacketType(unexpected.get_type()));
        }
    }

    //TODO listen for publishes to topics from here on out
    let (reader, writer) = socket.split();

    let (send_result, listen_result) = join(
        send_discovery_and_keep_alive(writer, send_buffer, keep_alive),
        listen_for_publishes(reader, receive_buffer),
    )
    .await;

    Ok(())
}
#[derive(Debug)]
struct FanSetting(u16);
#[derive(Debug)]

struct SetPointOutOfBoundsError;

impl FanSetting {
    const ZERO: Self = Self(0);
    const fn new(set_point: u16) -> Result<Self, SetPointOutOfBoundsError> {
        if set_point > fan::MAX_SET_POINT {
            return Err(SetPointOutOfBoundsError);
        }

        Ok(Self(set_point))
    }

    const fn get(&self) -> u16 {
        self.0
    }
}

static MODBUS_SIGNAL: Signal<CriticalSectionRawMutex, FanSetting> = Signal::new();

/// Controls the fans through the MAX845 and modbus communication
/// Waits for [MODBUS_SIGNAL] to be signalled to change the fan speed
#[embassy_executor::task]
async fn modbus_task(pin_4: PIN_4, uart0: UART0, pin_12: PIN_12, pin_13: PIN_13) {
    // UART things
    // PIN_4 seems to refer to GP4 on the Pico W pinout
    let mut driver_enable = Output::new(pin_4, Level::Low);
    let mut uart = Uart::new_blocking(uart0, pin_12, pin_13, fan::get_configuration());

    loop {
        // Wait for settings change
        let setting = MODBUS_SIGNAL.wait().await;
        // Send update through UART to MAX845 to modbus fans
        // Form message to fan 1
        let set_point = setting.get();
        let mut message: [u8; 8] = [
            // Device address fan 1
            fan::address::FAN_1,
            // Modbus function code
            modbus::function_code::WRITE_SINGLE_REGISTER,
            // Holding register address
            fan::holding_registers::REFERENCE_SET_POINT[0],
            fan::holding_registers::REFERENCE_SET_POINT[1],
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
        driver_enable.set_high();
        let result = uart.blocking_write(&message);
        info!("uart result: {:?}", result);

        // Before closing we need to flush the buffer to ensure that all data is written
        let result = uart.blocking_flush();

        // Wait to not cut off last byte when turning off driver enable
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
    }
}

/// This task handles inputs from physical buttons to change the fan speed
#[embassy_executor::task]
async fn input_task(pin_18: PIN_18) {
    // The button just rotates through fan settings. This is because we currently only have one button
    // Will probably use something more advanced in the future
    let mut button = Input::new(pin_18, Pull::Up);

    let mut fan_state = fan::State::default();
    //TODO debounce
    loop {
        // Falling edge for our button -> button down (pressing down
        // Rising edge for our button -> button up (letting go after press)
        // Act on press as there is delay between pressing and letting go and it feels snappier
        button.wait_for_falling_edge().await;

        // Advance to next fan state
        // Could make this a state machine with phantom data, but chose not to as long as it is this simple
        fan_state = match fan_state {
            fan::State::Off => fan::State::Low,
            fan::State::Low => fan::State::Medium,
            fan::State::Medium => fan::State::High,
            fan::State::High => fan::State::Off,
        };

        // Setting values low on purpose for testing
        let set_point = match fan_state {
            fan::State::Off => 0,
            // 10%
            fan::State::Low => fan::MAX_SET_POINT / 10,
            // 25%
            fan::State::Medium => fan::MAX_SET_POINT / 4,
            // 50%
            fan::State::High => fan::MAX_SET_POINT / 2,
        };

        // Send signal to change fan speed
        let Ok(setting) = FanSetting::new(set_point) else {
            warn!("Setting fan speed out of bounds. Not accepting new setting");
            continue;
        };

        MODBUS_SIGNAL.signal(setting);
    }
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

    let (net_device, mut control) =
        gain_control(spawner, pin_23, pin_25, pio0, dma_ch0, pin_24, pin_29).await;
    // UART things
    // PIN_4 seems to refer to GP4 on the Pico W pinout
    let mut driver_enable = Output::new(pin_4, Level::Low);
    //TODO read up on interrupts
    let mut uart = Uart::new_blocking(uart0, pin_12, pin_13, fan::get_configuration());

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

#[cfg(test)]
mod tests {
    // The tests don't run on the embedded target, so we need to import the std crate
    extern crate std;
    use super::*;

    /// These are important hardcoded values I want to make sure are not changed accidentally
    #[test]
    fn setting_does_not_exceed_max_set_point() {
        assert_eq!(fan::MAX_SET_POINT, 64_000);
        assert_eq!(FanSetting::new(64_000), Ok(FanSetting(64_000)));
        assert_eq!(FanSetting::new(64_000 + 1), Err(SetPointOutOfBoundsError));
        assert_eq!(FanSetting::new(u16::MAX), Err(SetPointOutOfBoundsError));
    }
}
