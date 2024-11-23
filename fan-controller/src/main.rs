#![no_std]
#![no_main]

use core::future::poll_fn;
use core::num::NonZeroU16;
use core::ops::DerefMut;
use core::str::{self, from_utf8};
use core::task::Poll;
use cyw43::{Control, NetDriver};
use cyw43_pio::PioSpi;
use debounce::Debouncer;
use defmt::*;
use dhcp::{DhcpMessageType, MessageType, Options};
use embassy_executor::Spawner;
use embassy_futures::join::{join, join5};
use embassy_net::dns::{DnsQueryType, DnsSocket};
use embassy_net::driver::Driver;
use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};
use embassy_net::udp::PacketMetadata;
use embassy_net::{tcp, Config, IpEndpoint, Ipv4Address, Stack, StackResources};
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Input, Level, Output, Pull};
use embassy_rp::peripherals::{
    DMA_CH0, PIN_18, PIN_20, PIN_21, PIN_23, PIN_25, PIN_4, PIO0, UART0,
};
use embassy_rp::pio::{InterruptHandler as PioInterruptHandler, Pio, PioPin};
use embassy_rp::uart::BufferedInterruptHandler;
use embassy_rp::{bind_interrupts, Peripherals};
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embassy_sync::pubsub::PubSubChannel;
use embassy_sync::signal::Signal;
use embassy_sync::waitqueue::AtomicWaker;
use embassy_sync::watch::Watch;
use embassy_time::{with_deadline, with_timeout, Duration, Instant, TimeoutError, Timer};
use embedded_io_async::{Read, Write};
use embedded_nal_async::TcpConnect;
use encoding::TryDecode;
use mqtt::packet::disconnect::Disconnect;
use rand::RngCore;
use static_cell::StaticCell;
use storage::{Ssid, Storage, WifiPassword};

use {defmt_rtt as _, panic_probe as _};

use self::mqtt::packet;
use crate::mqtt::non_zero_u16;
use crate::mqtt::packet::connect::Connect;
use crate::mqtt::packet::get_parts;
use crate::mqtt::packet::ping_request::PingRequest;
use crate::mqtt::packet::ping_response::PingResponse;
use crate::mqtt::packet::publish::Publish;
use crate::mqtt::packet::subscribe::{Subscribe, Subscription};
use crate::mqtt::packet::subscribe_acknowledgement::SubscribeAcknowledgement;
use crate::mqtt::packet::{connect, publish, subscribe};
use crate::mqtt::task::send;
use crate::mqtt::QualityOfService;

mod async_callback;
mod configuration;
mod debounce;
mod dhcp;
mod encoding;
mod fan;
mod modbus;
mod mqtt;
mod storage;
mod wifi;

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
    MqttClient::connect(reader, writer);
}

#[embassy_executor::task]
async fn mqtt_task(
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
    info!("Connected to MQTT broker through TCP");

    use mqtt::task;
    let packet = Connect {
        client_identifier: "fancontroller",
        username: configuration::MQTT_BROKER_CREDENTIALS.username,
        password: configuration::MQTT_BROKER_CREDENTIALS.password,
        keep_alive_seconds: configuration::KEEP_ALIVE.as_secs() as u16,
    };

    info!("Establishing MQTT connection");
    if let Err(error) = task::connect(&mut socket, packet).await {
        warn!("Error connecting to MQTT broker: {:?}", error);
        return;
    };
    info!("MQTT connection established");

    info!("Subscribing to MQTT topics");

    //TODO yes static "global" state is bad, but I am still learning how to use wakers and polling
    // with futures so this will be refactored when I made it work
    /// Contains the status of the subscribe packets send out. The packet identifier represents the
    /// index in the array
    static ACKNOWLEDGEMENTS: Mutex<CriticalSectionRawMutex, [bool; 2]> = Mutex::new([false, false]);
    /// The waker needs to be woken to complete the subscribe acknowledgement future.
    /// The embassy documentation does not explain when to use [`AtomicWaker`] but I am assuming
    /// it is useful for cases like this where I need to mutate a static.
    static WAKER: AtomicWaker = AtomicWaker::new();
    async fn handle_subscribe_acknowledgement<'f>(
        acknowledgement: &'f SubscribeAcknowledgement<'f>,
    ) {
        info!("Received subscribe acknowledgement");
        let mut acknowledgements = ACKNOWLEDGEMENTS.lock().await;
        info!("Locked ACKNOWLEDGEMENTS");
        // Validate server sends a valid packet identifier or we get bamboozled and panic
        let Some(value) = acknowledgements.get_mut(acknowledgement.packet_identifier as usize)
        else {
            warn!("Received subscribe acknowledgement for out of bounds packet identifier");
            return;
        };

        info!(
            "Received acknowledgement {} Reason codes: {:#04x}",
            acknowledgement.packet_identifier, acknowledgement.reason_codes
        );
    }

    async fn wait_for_acknowledgement(packet_identifier: NonZeroU16) {
        info!("Waiting for subscribe acknowledgements");
        //TODO if this function gets called multiple times it might never be woken up because there
        // is only one waker and the other call of this function will lock the mutex. To solve this
        // we could use structs from [embassy-sync::waitqueue] and/or a blocking mutex to remove the
        // try_lock which is used because the lock function is async and we can not easily await here
        poll_fn(|context| match ACKNOWLEDGEMENTS.try_lock() {
            Ok(mut guard) => {
                let packet = guard.get_mut(packet_identifier.get() as usize).unwrap();
                if *packet {
                    Poll::Ready(())
                } else {
                    // Waker needs to be overwritten on each poll. Read the Rust async book on wakers
                    // for more details
                    let waker = context.waker();
                    WAKER.register(waker);
                    Poll::Pending
                }
            }
            Err(_error) => Poll::Pending,
        })
        .await;

        info!("Subscribe acknowledgement received")
    }

    /// A handler that takes MQTT publishes and sets the fan settings accordingly
    async fn handle_publish<'f>(publish: &'f Publish<'f>) {
        info!("Handling publish");
        let sender = FAN_STATE.sender();

        // This part is not MQTT and application specific
        match publish.topic_name {
            "fancontroller/speed/percentage" => {
                let payload = match core::str::from_utf8(publish.payload) {
                    Ok(payload) => payload,
                    Err(error) => {
                        warn!("Expected percentage_command_topic payload (speed percentage) to be a valid UTF-8 string with a number");
                        return;
                    }
                };

                // And then to an integer...
                let set_point = payload.parse::<u16>();
                let set_point = match set_point {
                    Ok(set_point) => set_point,
                    Err(error) => {
                        warn!(
                            "Expected speed percentage to be a number string. Payload is: {}",
                            payload
                        );
                        return;
                    }
                };

                info!("SETTING FAN {}", set_point);
                let Ok(setting) = fan::Setting::new(set_point) else {
                    warn!(
                        "Setting fan speed out of bounds. Not accepting new setting: {}",
                        set_point
                    );
                    return;
                };

                sender.send(FanState {
                    setting,
                    // Set to on when there is a value set
                    is_on: true,
                });
                // Home assistant and fan update will be done by receiver
            }
            "fancontroller/on/set" => {
                info!("Received fan set on command from homeassistant");
                info!(
                    "Payload: {:?}",
                    core::str::from_utf8(publish.payload).unwrap()
                );
                let is_on = publish.payload == b"ON";
                info!("TURNING FAN {}", if is_on { "ON" } else { "OFF" });

                sender.send(FanState {
                    setting: sender
                        .try_get()
                        .map(|state| state.setting)
                        .unwrap_or(fan::Setting::ZERO),
                    is_on,
                });
                // Home assistant and fan update will be done by receiver
            }
            other => warn!(
                "Unexpected topic: {} with payload: {}",
                other, publish.payload
            ),
        }
    }

    static PING_RESPONSE: Signal<CriticalSectionRawMutex, PingResponse> = Signal::new();
    /// Callback handler for pings received when listening
    async fn handle_ping_response(ping_response: PingResponse) {
        info!("Received ping response");
        PING_RESPONSE.signal(ping_response);
    }

    enum ClientState {
        Disconnected,
        Connected,
        ConnectionLost,
    }
    static CLIENT_STATE: Signal<CriticalSectionRawMutex, ClientState> = Signal::new();

    async fn listen<'reader>(reader: &mut TcpReader<'reader>) {
        let mut buffer = [0; 1024];
        loop {
            info!("Waiting for packet");
            let result = reader.read(&mut buffer).await;
            let bytes_read = match result {
                // This indicates the TCP connection was closed. (See embassy-net documentation)
                Ok(0) => {
                    warn!("MQTT broker closed connection");
                    CLIENT_STATE.signal(ClientState::ConnectionLost);
                    return;
                }
                Ok(bytes_read) => bytes_read,
                Err(error) => {
                    warn!("Error reading from MQTT broker: {:?}", error);
                    return;
                }
            };

            info!("Packet received");

            let parts = match get_parts(&buffer[..bytes_read]) {
                Ok(parts) => parts,
                Err(error) => {
                    warn!("Error reading MQTT packet: {:?}", error);
                    return;
                }
            };

            info!("Handling packet");

            match parts.r#type {
                Publish::TYPE => {
                    info!("Received publish");
                    let publish = match Publish::try_decode(
                        parts.flags,
                        &parts.variable_header_and_payload,
                    ) {
                        Ok(publish) => publish,
                        Err(error) => {
                            error!("Error reading publish: {:?}", error);
                            continue;
                        }
                    };

                    handle_publish(&publish).await;
                    info!("Handled publish");
                }
                SubscribeAcknowledgement::TYPE => {
                    let subscribe_acknowledgement =
                        match SubscribeAcknowledgement::read(&parts.variable_header_and_payload) {
                            Ok(acknowledgement) => acknowledgement,
                            Err(error) => {
                                error!("Error reading subscribe acknowledgement: {:?}", error);
                                continue;
                            }
                        };

                    handle_subscribe_acknowledgement(&subscribe_acknowledgement).await;
                }
                PingResponse::TYPE => {
                    info!("Received ping response");
                    let ping_response = match PingResponse::try_decode(
                        parts.flags,
                        &parts.variable_header_and_payload,
                    ) {
                        Ok(response) => response,
                        // Matching to get compiler error if this changes
                        Err(Infallible) => {
                            defmt::unreachable!("Ping response is always empty so decode should always succeed if the protocol did not change")
                        }
                    };

                    handle_ping_response(ping_response).await;
                }
                Disconnect::TYPE => {
                    info!("Received disconnect");

                    let disconnect =
                        Disconnect::try_decode(parts.flags, &parts.variable_header_and_payload);
                    info!("Disconnect {:?}", disconnect);
                    //TODO disconnect TCP connection
                }
                other => info!("Unsupported packet type {}", other),
            }
        }
    }

    let (mut reader, writer) = socket.split();
    // Future 1
    let listen = listen(&mut reader);

    enum PredefinedPublish {
        FanPercentageState {
            setting: fan::Setting,
        },
        FanOnState {
            is_on: bool,
        },
        SensorTemperature {
            /// Celsius temperature as read from the sensor. This is the raw value. To get the actual temperature, divide by 10.
            /// e.g. 234 means 23.4 degrees Celsius
            temperature: u16,
            /// The fan the sensor is on
            fan: fan::Fan,
        },
    }

    enum Message<'a> {
        Subscribe(Subscribe<'a>),
        Publish(Publish<'a>),
        PredefinedPublish(PredefinedPublish),
    }

    static OUTGOING: Channel<CriticalSectionRawMutex, Message, 8> = Channel::new();
    /// The instant when the last packet was sent to determine when the next keep alive has to be sent
    static LAST_PACKET: Signal<CriticalSectionRawMutex, Instant> = Signal::new();
    async fn talk(writer: &Mutex<CriticalSectionRawMutex, TcpWriter<'_>>) {
        loop {
            let message = OUTGOING.receive().await;

            let mut writer = writer.lock().await;
            match message {
                Message::Subscribe(subscribe) => {
                    info!("Sending subscribe");
                    if let Err(error) = send(&mut *writer, subscribe).await {
                        error!("Error sending subscribe: {:?}", error);
                        continue;
                    }
                }
                Message::Publish(publish) => {
                    info!(
                        "Sending publish {:?}",
                        core::str::from_utf8(publish.payload).unwrap()
                    );
                    if let Err(error) = send(&mut *writer, publish).await {
                        error!("Error sending publish: {:?}", error);
                        continue;
                    }
                }
                Message::PredefinedPublish(publish) => match publish {
                    PredefinedPublish::FanPercentageState { setting } => {
                        info!("Sending percentage state publish {}", setting);
                        // let buffer: StringBuffer<5> = setting.into();
                        let buffer = match heapless::String::<5>::try_from(setting.0) {
                            Ok(buffer) => buffer,
                            Err(error) => {
                                error!("Error converting setting to string: {:?}", error);
                                continue;
                            }
                        };

                        let packet = Publish {
                            topic_name: "fancontroller/speed/percentage_state",
                            payload: buffer.as_bytes(),
                        };

                        if let Err(error) = send(&mut *writer, packet).await {
                            error!("Error sending predefined publish: {:?}", error);
                            continue;
                        }
                    }
                    PredefinedPublish::FanOnState { is_on } => {
                        //TODO update state
                        let packet = if is_on {
                            Publish {
                                topic_name: "fancontroller/on/state",
                                payload: b"ON",
                            }
                        } else {
                            Publish {
                                topic_name: "fancontroller/on/state",
                                payload: b"OFF",
                            }
                        };

                        info!("Sending on state publish");
                        if let Err(error) = send(&mut *writer, packet).await {
                            error!("Error sending predefined publish: {:?}", error);
                            continue;
                        }
                    }
                    PredefinedPublish::SensorTemperature { temperature, fan } => {
                        // The value is divided by 10 in the value template that is submitted with home assistant discovery
                        let formatted = match heapless::String::<3>::try_from(temperature) {
                            Ok(formatted) => formatted,
                            Err(error) => {
                                error!(
                                    "Error writing temperature {} to heapless::String",
                                    temperature
                                );
                                continue;
                            }
                        };
                        let packet = Publish {
                            topic_name: match fan {
                                fan::Fan::One => "fancontroller/sensor/fan-1/temperature",
                                fan::Fan::Two => "fancontroller/sensor/fan-2/temperature",
                            },
                            payload: formatted.as_bytes(),
                        };
                        info!("Sending sensor temperature publish");
                        if let Err(error) = send(&mut *writer, packet).await {
                            error!("Error sending predefined publish: {:?}", error);
                            continue;
                        }
                    }
                },
            }

            LAST_PACKET.signal(Instant::now());
        }
    }

    // Using a mutex for the writer, so it can be shared between the task that sends messages (for
    // subscribing and publishing fan speed updates) and the task that sends the keep alive ping
    let writer = Mutex::<CriticalSectionRawMutex, TcpWriter<'_>>::new(writer);
    // Future 2
    let talk = talk(&writer);

    // Subscribe to home assistant topics
    const SUBSCRIPTIONS: [Subscription; 2] = [
        Subscription {
            topic_filter: "fancontroller/on/set",
            options: mqtt::packet::subscribe::Options::new(
                QualityOfService::AtMostOnceDelivery,
                false,
                false,
                // mqtt::packet::subscribe::RetainHandling::DoNotSend,
                mqtt::packet::subscribe::RetainHandling::SendAtSubscribe,
            ),
        },
        Subscription {
            topic_filter: "fancontroller/speed/percentage",
            options: mqtt::packet::subscribe::Options::new(
                QualityOfService::AtMostOnceDelivery,
                false,
                false,
                // mqtt::packet::subscribe::RetainHandling::DoNotSend,
                mqtt::packet::subscribe::RetainHandling::SendAtSubscribe,
            ),
        },
    ];

    async fn set_up_subscriptions(packet_identifier: NonZeroU16) {
        info!("Setting up subscriptions");
        let message = Message::Subscribe(Subscribe {
            subscriptions: &SUBSCRIPTIONS,
            //TODO free identifier management
            packet_identifier,
        });

        OUTGOING.send(message).await;

        wait_for_acknowledgement(packet_identifier).await;
        info!("Set up subscriptions complete")
    }

    async fn set_up_discovery() {
        // Send discovery packet
        // Configuration is like the YAML configuration that would be added in Home Assistant but as JSON
        // Command topic: The MQTT topic to publish commands to change the state of the fan
        //TODO set firmware version from Cargo.toml package version
        //TODO think about setting hardware version, support url, and manufacturer
        //TODO create single home assistant device with multiple entities for sensors in fan and the bypass
        //TODO add diagnostic entity like IP address
        //TODO availability topic
        //TODO remove whitespace at compile time through macro, build script or const fn
        //TODO support availability with will message https://www.home-assistant.io/integrations/mqtt/#using-availability-topics

        // Using abbreviations to save space of binary and on the wire (haven't measured the effect though...)
        // See https://www.home-assistant.io/integrations/mqtt/ or https://github.com/home-assistant/core/blob/dev/homeassistant/components/mqtt/abbreviations.py
        // name -> name
        // uniq_id -> unique_id
        // stat_t -> state_topic
        // cmd_t -> command_topic
        // pct_stat_t -> percentage_state_topic
        // pct_cmd_t -> percentage_command_topic
        // spd_rng_max -> speed_range_max
        // dev -> device
        // mf -> manufacturer
        // o -> origin (recommended https://www.home-assistant.io/integrations/mqtt/)
        // unit_of_meas -> unit_of_measurement
        // Don't need to set speed_range_min because it is 1 by default

        // Speed set to max 32000 which is 50% of what the fans can do but more is not needed. This way the fans last longer
        const DISCOVERY_PAYLOAD: &[u8] = br#"{
            "name": "Fans",
            "uniq_id": "fancontroller",
            "stat_t": "fancontroller/on/state",
            "cmd_t": "fancontroller/on/set",
            "pct_stat_t": "fancontroller/speed/percentage_state",
            "pct_cmd_t": "fancontroller/speed/percentage",
            "spd_rng_max": 32000,
            "dev": {
                "ids": "fancontroller-device",
                "name": "Fan Controller",
                "model": "Raspberry Pi Pico W 1"
            }
        }"#;

        // "~": "fancontroller",
        //
        // "o": {
        //     "name": "Fan Controller",
        //     "url": "github.com/santaclaas/embedded-fan-control/"
        // }

        const DISCOVERY_PUBLISH: Message = Message::Publish(Publish {
            topic_name: configuration::DISCOVERY_TOPIC,
            payload: DISCOVERY_PAYLOAD,
        });

        OUTGOING.send(DISCOVERY_PUBLISH).await;
        //TODO wait for packet acknowledgement
        return;

        // Needs to be string because "°C" is not ASCII
        const DISCOVERY_TEMPERATURE_SENSOR_1: &str = r#"{
            "name": "Fan 1 Temperature Sensor",
            "uniq_id": "fan-1-temperature-sensor",
            "dev": {
                "ids": "fancontroller-device"
            },
            "stat_t": "fancontroller/fan-1/temperature",
            "value_template": "{{ value | float / 10 }}",
            "unit_of_meas": "°C"
        }"#;

        const DISCOVERY_TEMPERATURE_SENSOR_1_PUBLISH: Message = Message::Publish(Publish {
            topic_name: "homeassistant/sensor/fan-1-temperature-sensor/config",
            payload: DISCOVERY_TEMPERATURE_SENSOR_1.as_bytes(),
        });

        OUTGOING.send(DISCOVERY_TEMPERATURE_SENSOR_1_PUBLISH).await;
    }

    // Future 3
    let set_up = join(set_up_subscriptions(non_zero_u16!(1)), set_up_discovery());

    // Keep alive task
    async fn keep_alive(writer: &Mutex<CriticalSectionRawMutex, TcpWriter<'_>>) {
        // Send keep alive packets after connection with connect packet is established.
        // Wait for a packet to be sent or the keep alive interval to time the wait out.
        // If the keep alive timed the wait out, send a ping request packet.
        // Else if a packet was sent, wait for the next packet to be sent with the keep alive
        // interval as timeout.

        let start = LAST_PACKET.try_take().unwrap_or(Instant::now());
        let mut last_send = start;
        loop {
            // The server waits for 1.5 times the keep alive interval, so being off by a bit due to
            // network, async overhead or the clock not being exactly precise is fine
            let deadline = last_send + configuration::KEEP_ALIVE;
            let result = with_deadline(deadline, LAST_PACKET.wait()).await;
            match result {
                Err(TimeoutError) => {
                    info!("Sending keep alive ping request");
                    let mut writer = writer.lock().await;
                    // Send keep alive ping request
                    if let Err(error) = send(&mut *writer, PingRequest).await {
                        error!("Error sending keep alive ping request: {:?}", error);
                        CLIENT_STATE.signal(ClientState::ConnectionLost);
                        return;
                    }

                    // Keep alive is time from when the last packet was sent and not when the ping response
                    // was received. Therefore, we need to reset it here
                    last_send = Instant::now();

                    // Wait for ping response
                    if let Err(TimeoutError) =
                        with_timeout(configuration::MQTT_TIMEOUT, PING_RESPONSE.wait()).await
                    {
                        // Assume disconnect from server
                        error!("Timeout waiting for ping response. Disconnecting");
                        CLIENT_STATE.signal(ClientState::ConnectionLost);
                        return;
                    }

                    let response_duration = Instant::now() - last_send;
                    info!(
                        "Received and processed ping response in {:?} ({}μs) after sending ping request",
                        response_duration,
                        response_duration.as_micros()
                    );
                }
                Ok(last_packet) => {
                    info!("Sent packet. Resetting timer to send keep alive");
                    last_send = last_packet;
                }
            }
        }
    }

    // Future 4
    let keep_alive = keep_alive(&writer);

    // Future 5 update homeassistant when change occurs
    async fn update_homeassistant() {
        let Some(mut receiver) = FAN_STATE.receiver() else {
            error!("Fan state receiver was not set up. Cannot update Homeassistant");
            return;
        };

        // If there is no initial value this will cause an initial update to homeassistant
        // which is good as we don't know the state on homeassistant
        let mut previous_state = receiver.try_get().unwrap_or(FanState {
            is_on: false,
            setting: fan::Setting::ZERO,
        });

        loop {
            let state = receiver.changed().await;

            info!(
                "UPDATING HOMEASSISTANT {}",
                if state.is_on { "ON" } else { "OFF" }
            );
            let message =
                Message::PredefinedPublish(PredefinedPublish::FanOnState { is_on: state.is_on });
            OUTGOING.send(message).await;

            // Update setting before is on state for smoother transition in homeassistant UI
            info!("UPDATING HOMEASSISTANT {}", state.setting);
            let message = Message::PredefinedPublish(PredefinedPublish::FanPercentageState {
                setting: state.setting,
            });
            OUTGOING.send(message).await;

            previous_state = state;
        }
    }

    // Future 6
    async fn poll_sensors() {
        loop {
            let mut fans = FANS.lock().await;
            let Some(fans) = fans.deref_mut() else {
                error!("Fans were not set up. Cannot poll sensors");
                return;
            };

            let temperature = match fans.get_temperature(fan::Fan::One).await {
                Ok(temperature) => temperature,
                Err(fan::Error::Timeout(TimeoutError)) => {
                    error!("Timeout getting temperature for fan 1");
                    return;
                }
                Err(fan::Error::Uart(error)) => {
                    error!("Uart error getting temperature for fan 1: {:?}", error);
                    return;
                }
            };

            Timer::after_secs(10).await;
        }
    }

    //TODO cancel all tasks when client loses connection
    join5(
        listen,
        talk,
        keep_alive,
        set_up,
        // Join because there is only a join with max 5 arguments 😬
        // join(update_homeassistant(), poll_sensors()),
        update_homeassistant(),
    )
    .await;
}

/// This task handles inputs from physical buttons to change the fan speed
#[embassy_executor::task]
async fn input_task(pin_18: PIN_18) {
    // The button just rotates through fan settings. This is because we currently only have one button
    // Will probably use something more advanced in the future
    let mut button = Debouncer::new(Input::new(pin_18, Pull::Up), Duration::from_millis(250));

    let mut fan_state = fan::State::default();
    let sender = FAN_STATE.sender();

    loop {
        // Falling edge for our button -> button down (pressing down
        // Rising edge for our button -> button up (letting go after press)
        // Act on press as there is delay between pressing and letting go and it feels snappier
        button.debounce_falling_edge().await;
        info!("Button pressed");
        // Record time of button press for naive debounce
        let start = Instant::now();

        // Advance to next fan state
        // Could make this a state machine with phantom data, but chose not to as long as it is this simple
        fan_state = match fan_state {
            fan::State::Off => fan::State::Low,
            fan::State::Low => fan::State::Medium,
            fan::State::Medium => fan::State::High,
            fan::State::High => fan::State::Off,
        };

        // Setting values low on purpose for testing
        let state = match fan_state {
            fan::State::Off => {
                let setting = FAN_STATE
                    .try_get()
                    .map(|state| state.setting)
                    .unwrap_or(fan::Setting::ZERO);
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
    let Some(mut receiver) = FAN_STATE.receiver() else {
        // Not using asserts because they are hard to debug on embedded where it crashed
        error!("No receiver for fan is on state. This should never happen.");
        return;
    };

    // Only comparing on state causes button triggers to be ignored
    let mut previous = FAN_STATE.try_get().unwrap_or(FanState {
        is_on: false,
        setting: fan::Setting::ZERO,
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
            fan::Setting::ZERO
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
    setting: fan::Setting,
}

/// Is on and the setting update independent of each other on homeassistant.
/// Update this state even though it might not yet be set on the fan devices.
/// Use optimistic updates.
/// Senders:
/// - MQTT (server to client)
/// - Button
/// Receivers:
/// - Fan
/// - MQTT (client to server)
static FAN_STATE: Watch<CriticalSectionRawMutex, FanState, 3> = Watch::new();

/// Displays fan status with 2 LEDs:
/// Off Off -> Fans Off
/// On Off -> Fan on low setting
/// Off On -> Fan on medium setting
/// On On -> Fan on high setting
#[embassy_executor::task]
async fn display_status(pin_21: PIN_21, pin_20: PIN_20) {
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

    let Some(mut receiver) = FAN_STATE.receiver() else {
        // Not using asserts because they are hard to debug on embedded where it crashed
        error!("No receiver for fan is on state. This should never happen.");
        return;
    };

    // Set initial state
    let mut current_state = FAN_STATE.try_get().unwrap_or(FanState {
        is_on: false,
        setting: fan::Setting::ZERO,
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

// #[embassy_executor::task]
// async fn

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    // Glossary:
    // DMA: Direct Memory Access
    // PIO: Programmable Input/Output
    // UART: Universal Asynchronous Receiver/Transmitter
    // SPI: Serial Peripheral Interface
    // I2C: Inter-Integrated Circuit
    // GPIO: General Purpose Input/Output
    let Peripherals {
        PIN_23: pin_23,
        PIN_25: pin_25,
        PIO0: pio0,
        DMA_CH1: dma_ch1,
        DMA_CH2: dma_ch2,
        PIN_24: pin_24,
        PIN_29: pin_29,
        PIN_4: pin_4,
        UART0: uart0,
        PIN_12: pin_12,
        PIN_13: pin_13,
        PIN_18: pin_18,
        // Status LEDs
        PIN_20: pin_20,
        PIN_21: pin_21,
        // Persistent flash storage to store configuration
        FLASH: flash,
        DMA_CH0: dma_ch0,
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
    unwrap!(spawner.spawn(input_task(pin_18)));
    unwrap!(spawner.spawn(update_fans()));
    unwrap!(spawner.spawn(display_status(pin_21, pin_20)));

    //TODO enable MQTT task when wifi is set up. Otherwise run as access point and host a web interface to configure wifi

    // Read wifi credentials from flash storage
    // Baseed on https://github.com/embassy-rs/embassy/blob/8d8cd78f634b2f435e3a997f7f8f3ac0b8ca300c/examples/rp/src/bin/flash.rs
    // And https://github.com/tweedegolf/sequential-storage/blob/b6f7da77b7f3d66bc1ac36992a6a7b399b5ba030/example/src/main.rs

    // Add some delay to give an attached debug probe time to parse the
    // defmt RTT header. Reading that header might touch flash memory, which
    // interferes with flash write operations.
    // https://github.com/knurling-rs/defmt/pull/683
    Timer::after_millis(10).await;

    info!("Reading from flash");
    // let mut storage = Storage::new(flash, dma_ch0);
    // let wifi_ssid = storage.read_wifi_ssid().await;
    // match wifi_ssid {
    //     Ok(wifi_ssid) => {
    //         info!("WiFi SSID: {}", wifi_ssid);
    //     }
    //     Err(error) => {
    //         error!("Error reading WiFi SSID from flash");
    //         match error {
    //             storage::Error::FlashError(error) => {
    //                 info!("Flash error: {:?}", defmt::Debug2Format(&error))
    //             }

    //             storage::Error::Utf8Error(utf8_error) => {
    //                 info!("UTF8 error: {:?}", defmt::Debug2Format(&utf8_error))
    //             }
    //         }
    //     }
    // }
    // storage.write();
    let storage = Storage::create(flash).await.unwrap();

    info!("Reading SSID from flash storage");
    let ssid: Option<Ssid> = match storage.get().await {
        Ok(ssid) => Some(ssid),
        Err(ekv::ReadError::KeyNotFound) => {
            info!("No SSID found");
            None
        }
        Err(error) => {
            // We can't stop the device as base functionality should still work
            error!("Error reading SSID from flash {:?}", error);
            None
        }
    };

    info!("Reading wifi password from flash storage");
    let password: Option<WifiPassword> = match storage.get().await {
        Ok(password) => Some(password),
        Err(ekv::ReadError::KeyNotFound) => {
            info!("No password found");
            None
        }
        Err(error) => {
            // We can't stop the device as base functionality should still work
            error!("Error reading password from flash {:?}", error);
            None
        }
    };
    info!("got from flash");

    let (net_device, mut control) =
        gain_control(spawner, pin_23, pin_25, pio0, dma_ch0, pin_24, pin_29).await;

    //TODO limit retries until we go back into configuration mode

    //TODO read mac from stack
    //TODO read id from flash

    // If we have at least an SSID, we can attempt to connect to WiFi
    if let Some(ssid) = ssid {
        if let Err(error) = wifi::join_wifi(&mut control, ssid, password).await {
            // Create wifi network and host web server to configure wifi

            // Need to probably use link local addresses
            const DEVICE_ADDRESS: Ipv4Address = Ipv4Address::new(192, 168, 178, 1);
            let configuration = embassy_net::StaticConfigV4 {
                address: embassy_net::Ipv4Cidr::new(DEVICE_ADDRESS, 24),
                gateway: Some(DEVICE_ADDRESS),
                // gateway: None,
                dns_servers: heapless::Vec::new(),
            };
            // let configuration = embassy_net::StaticConfigV4 {
            //     address: embassy_net::Ipv4Cidr::new(
            //         embassy_net::Ipv4Address::new(169, 254, 1, 1),
            //         16,
            //     ),
            //     dns_servers: heapless::Vec::new(),
            //     gateway: None,
            // };

            let configuration = Config::ipv4_static(configuration);

            // Idk where the channel 5 comes from
            control.start_ap_open("fancontroller", 5).await;

            // Stack
            let mut random = RoscRng;
            let seed = random.next_u64();
            // Initialize network stack
            static RESOURCES: StaticCell<StackResources<3>> = StaticCell::new();

            static STACK: StaticCell<Stack<NetDriver<'static>>> = StaticCell::new();
            let stack = &*STACK.init(Stack::new(
                net_device,
                configuration,
                RESOURCES.init(StackResources::<3>::new()),
                seed,
            ));

            unwrap!(spawner.spawn(network_task(stack)));
            // Use it
            // Try out DHCP
            let mut transmit_buffer = [0; 4096];
            let mut receive_buffer = [0; 4096];
            let mut buffer = [0; 4096];
            let mut transmit_meta = [PacketMetadata::EMPTY; 16];
            let mut receive_meta = [PacketMetadata::EMPTY; 16];

            loop {
                let mut socket = embassy_net::udp::UdpSocket::new(
                    stack,
                    &mut receive_meta,
                    &mut receive_buffer,
                    &mut transmit_meta,
                    &mut transmit_buffer,
                );

                info!("Binding UDP socket");
                let result = socket.bind(67);
                if let Err(error) = result {
                    error!("Failed to bind to port 67: {:?}", error);
                    Timer::after_secs(10).await;
                    continue;
                }

                // Listen for incoming packets
                loop {
                    info!("Waiting for receive");
                    let (bytes_read, meta_data) = match socket.recv_from(&mut buffer).await {
                        Ok(result) => result,
                        Err(error) => {
                            error!("Failed to receive packet: {:?}", error);
                            Timer::after_secs(10).await;
                            continue;
                        }
                    };

                    info!("Received UDP packet {:x}", &buffer[..bytes_read]);
                    let packet = match dhcp::Packet::try_decode(&buffer[..bytes_read]) {
                        Ok(packet) => {
                            info!("Received DHCP packet: {:#?}", packet);
                            packet
                        }
                        Err(error) => {
                            error!("Error decoding DHCP packet: {:#?}", error);
                            continue;
                        }
                    };

                    let Some(message_type) = packet.options.message_type else {
                        warn!("No message type in DHCP packet");
                        continue;
                    };

                    match message_type {
                        DhcpMessageType::Discover => {
                            info!("Received DHCP discover packet");

                            // Send DHCP offer
                            let offer = dhcp::Packet {
                                option: MessageType::Reply,
                                address_type: dhcp::HardwareAddressType::Ethernet,
                                hardware_address_length: 6,
                                hops_count: 0,
                                transaction_id: packet.transaction_id,
                                seconds_elapsed: 0,
                                flags: dhcp::ReplyType::Unicast,
                                client_address: Ipv4Address::default(),
                                your_address: Ipv4Address::new(192, 168, 178, 2),
                                server_address: DEVICE_ADDRESS,
                                gateway_address: Ipv4Address::default(),
                                client_hardware_address: packet.client_hardware_address,
                                //TODO
                                options: Options {
                                    message_type: Some(DhcpMessageType::Offer),
                                    subnet_mask: Some(Ipv4Address::new(255, 255, 255, 0)),
                                    router: Some(DEVICE_ADDRESS),
                                    address_time: Some(86400),
                                    //TODO try without DNS server
                                    ..Default::default()
                                },
                            };
                        }
                        other => warn!("Received unsupported DHCP message type: {:?}", other),
                    }
                }
            }

            let mut transmit_buffer = [0; 4096];
            let mut receive_buffer = [0; 4096];
            let mut buffer = [0; 4096];

            loop {
                let mut socket = TcpSocket::new(stack, &mut receive_buffer, &mut transmit_buffer);
                socket.set_timeout(Some(Duration::from_secs(10)));

                control.gpio_set(0, false).await;
                info!("Listening on TCP:1234...");
                if let Err(e) = socket.accept(1234).await {
                    warn!("accept error: {:?}", e);
                    continue;
                }

                info!("Received connection from {:?}", socket.remote_endpoint());
                control.gpio_set(0, true).await;

                loop {
                    let n = match socket.read(&mut buffer).await {
                        Ok(0) => {
                            warn!("read EOF");
                            break;
                        }
                        Ok(n) => n,
                        Err(e) => {
                            warn!("read error: {:?}", e);
                            break;
                        }
                    };

                    info!("rxd {}", from_utf8(&buffer[..n]).unwrap());

                    const HTTP_RESPONSE: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><body><h1>Hello World!</h1></body></html>";

                    match socket.write_all(&HTTP_RESPONSE).await {
                        Ok(()) => {}
                        Err(error) => {
                            warn!("write error: {:?}", error);
                            break;
                        }
                    };
                }
            }
        }
    }

    // match ssid.map(|ssid| ssid.try_into_string()) {
    //     Some(Ok(ssid)) => match password.map(|password| password.try_into_string()) {
    //         Some(Ok(passowrd)) => {
    //             let result = control.join_wpa2(ssid, passowrd).await;
    //         }
    //         Some(Err(core::str::Utf8Error { .. })) => {
    //             warn!("Stored WiFi password is not valid string");
    //         }
    //         None => {}
    //     },
    //     Some(Err(core::str::Utf8Error { .. })) => {
    //         warn!("Stored WiFi SSID is not valid string");
    //     }
    //     None => {}
    // }

    // If no wifi credentials are found, run as access point and host a web interface to configure wifi
    //TODO implement reset button to reset device to "factory" settings and allow reconfiguation

    // The MQTT task waits for publishes from MQTT and sends them to the modbus task.
    // It also sends updates from the modbus task that happen through button inputs to MQTT
    // unwrap!(spawner.spawn(mqtt_task(
    //     spawner, pin_23, pin_25, pio0, dma_ch0, pin_24, pin_29
    // )));
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
        assert_eq!(Setting::new(64_000), Ok(Setting(64_000)));
        assert_eq!(Setting::new(64_000 + 1), Err(SetPointOutOfBoundsError));
        assert_eq!(Setting::new(u16::MAX), Err(SetPointOutOfBoundsError));
    }
}
