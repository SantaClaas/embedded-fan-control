use crate::PingRequest;
use crate::fan::set_point::SetPoint;
use crate::mqtt::packet::connect::Connect;
use crate::mqtt::packet::disconnect::Disconnect;
use crate::mqtt::packet::ping_response::PingResponse;
use crate::mqtt::packet::publish;
use crate::mqtt::packet::subscribe::{Subscribe, Subscription};
use crate::mqtt::packet::subscribe_acknowledgement::SubscribeAcknowledgement;
use crate::mqtt::task::send;
use crate::mqtt::{self};
use crate::mqtt::{TryDecode, non_zero_u16};
use crate::{FanState, configuration, fan, gain_control};
use crate::{ModbusOnceLock, modbus};
use ::mqtt::QualityOfService;
use core::future::poll_fn;
use core::num::NonZeroU16;
use core::ops::DerefMut;
use core::task::Poll;
use cyw43::{Control, NetDriver};
use defmt::{Format, error, info, unwrap, warn};
use embassy_executor::Spawner;
use embassy_futures::join::{join, join4};
use embassy_net::dns::{DnsQueryType, DnsSocket};
use embassy_net::driver::Driver;
use embassy_net::tcp::{TcpSocket, TcpWriter};
use embassy_net::{Config, IpAddress, IpEndpoint, Stack, StackResources};
use embassy_rp::clocks::RoscRng;
use embassy_rp::peripherals::{DMA_CH0, PIN_23, PIN_25, PIO0};
use embassy_rp::pio::PioPin;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::{self, Channel};
use embassy_sync::mutex::Mutex;
use embassy_sync::pubsub::subscriber::Sub;
use embassy_sync::signal::Signal;
use embassy_sync::waitqueue::AtomicWaker;
use embassy_time::{Duration, Instant, Timer, with_deadline, with_timeout};
use embedded_io_async::Read;
use rand::RngCore;
use static_cell::StaticCell;

#[embassy_executor::task]
async fn network_task(stack: &'static Stack<NetDriver<'static>>) -> ! {
    stack.run().await
}

async fn handle_subscribe_acknowledgement<'f, const SUBSCRIPTIONS: usize>(
    acknowledgement: &'f SubscribeAcknowledgement<'f>,
    //TODO rework this to be a channel that sends the packet identifier as acknowledgement
    acknowledgements: &Mutex<CriticalSectionRawMutex, [bool; SUBSCRIPTIONS]>,
) {
    info!("[Subscription] Received subscribe acknowledgement");
    let mut acknowledgements = acknowledgements.lock().await;
    info!("[Subscription] Locked ACKNOWLEDGEMENTS");
    // Validate server sends a valid packet identifier or we get bamboozled and panic
    let Some(value) = acknowledgements.get_mut(acknowledgement.packet_identifier as usize) else {
        warn!(
            "[Subscription] Received subscribe acknowledgement for out of bounds packet identifier"
        );
        return;
    };

    info!(
        "[Subscription] Received acknowledgement {} Reason codes: {:#04x}",
        acknowledgement.packet_identifier, acknowledgement.reason_codes
    );
}

async fn wait_for_acknowledgement<const SUBSCRIPTIONS: usize>(
    packet_identifier: NonZeroU16,
    acknowledgements: &Mutex<CriticalSectionRawMutex, [bool; SUBSCRIPTIONS]>,
    waker: &AtomicWaker,
) {
    info!("[Subscription] Waiting for subscribe acknowledgements");
    //TODO if this function gets called multiple times it might never be woken up because there
    // is only one waker and the other call of this function will lock the mutex. To solve this
    // we could use structs from [embassy-sync::waitqueue] and/or a blocking mutex to remove the
    // try_lock which is used because the lock function is async and we can not easily await here
    poll_fn(|context| match acknowledgements.try_lock() {
        Ok(mut guard) => {
            let packet = guard.get_mut(packet_identifier.get() as usize).unwrap();
            if *packet {
                Poll::Ready(())
            } else {
                // Waker needs to be overwritten on each poll.
                // Read the Rust async book on wakers for more details
                let context_waker = context.waker();
                waker.register(context_waker);
                Poll::Pending
            }
        }
        Err(_error) => Poll::Pending,
    })
    .await;

    info!("[Subscription] Subscribe acknowledgement received")
}

/// A handler that takes MQTT publishes and sets the fan settings accordingly
async fn handle_publish<'f>(
    publish: &'f publish::Publish<'f>,
    sender: &embassy_sync::watch::Sender<'_, CriticalSectionRawMutex, FanState, 3>,
) {
    info!("Handling publish");

    // This part is not MQTT but application specific
    match publish.topic_name {
        "fancontroller/speed/percentage" => {
            let payload = match core::str::from_utf8(publish.payload) {
                Ok(payload) => payload,
                Err(error) => {
                    warn!(
                        "Expected percentage_command_topic payload (speed percentage) to be a valid UTF-8 string with a number"
                    );
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
            let Ok(setting) = SetPoint::new(set_point) else {
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
        topic::fan_controller::COMMAND => {
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
                    .unwrap_or(SetPoint::ZERO),
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

/// Callback handler for pings received when listening
async fn handle_ping_response(
    ping_response: PingResponse,
    signal: &Signal<CriticalSectionRawMutex, PingResponse>,
) {
    info!("Received ping response");
    signal.signal(ping_response);
}

enum ClientState {
    Disconnected,
    Connected,
    ConnectionLost,
}

async fn listen<
    ReadError: Format,
    FromError,
    Send: for<'a> TryFrom<publish::Publish<'a>, Error = FromError>,
    const SEND: usize,
    const SUBSCRIPTIONS: usize,
>(
    reader: &mut impl Read<Error = ReadError>,
    sender: &channel::Sender<'_, CriticalSectionRawMutex, Result<Send, FromError>, SEND>,
    client_state: &Signal<CriticalSectionRawMutex, ClientState>,
    acknowledgements: &Mutex<CriticalSectionRawMutex, [bool; SUBSCRIPTIONS]>,
    ping_response_signal: &Signal<CriticalSectionRawMutex, PingResponse>,
) {
    let mut buffer = [0; 1024];
    loop {
        info!("Waiting for packet");
        let result = reader.read(&mut buffer).await;
        let bytes_read = match result {
            // This indicates the TCP connection was closed. (See embassy-net documentation)
            Ok(0) => {
                warn!("MQTT broker closed connection");
                client_state.signal(ClientState::ConnectionLost);
                return;
            }
            Ok(bytes_read) => bytes_read,
            Err(error) => {
                warn!("Error reading from MQTT broker: {:?}", error);
                return;
            }
        };

        info!("Packet received");

        let parts = match mqtt::packet::get_parts(&buffer[..bytes_read]) {
            Ok(parts) => parts,
            Err(error) => {
                warn!("Error reading MQTT packet: {:?}", error);
                return;
            }
        };

        info!("Handling packet");

        match parts.r#type {
            publish::Publish::TYPE => {
                info!("Received publish");
                let publish = match publish::Publish::try_decode(
                    parts.flags,
                    parts.variable_header_and_payload,
                ) {
                    Ok(publish) => publish,
                    Err(error) => {
                        error!("Error reading publish: {:?}", error);
                        continue;
                    }
                };

                // MQTT does not concern itself with validity of the payload
                // We allow for a TryFrom implementation anyways to reduce boilerplate for users
                // because if we only allow a From implementation users would have to create their own result type
                // that implements the From trait and stores the error to be passed on.
                // If users still want to use a From implementation then they can still do this
                // as any type that implements From, automatically implements TryFrom
                let result = Send::try_from(publish);
                sender.send(result).await;
                info!("Handled publish")
            }
            SubscribeAcknowledgement::TYPE => {
                let subscribe_acknowledgement =
                    match SubscribeAcknowledgement::read(parts.variable_header_and_payload) {
                        Ok(acknowledgement) => acknowledgement,
                        Err(error) => {
                            error!(
                                "[Subscription] Error reading subscribe acknowledgement: {:?}",
                                error
                            );
                            continue;
                        }
                    };

                handle_subscribe_acknowledgement(&subscribe_acknowledgement, acknowledgements)
                    .await;
            }
            PingResponse::TYPE => {
                info!("Received ping response");
                let ping_response = match PingResponse::try_decode(
                    parts.flags,
                    parts.variable_header_and_payload,
                ) {
                    Ok(response) => response,
                    // Matching to get compiler error if this changes
                    Err(_) => {
                        defmt::unreachable!(
                            "Ping response is always empty so decode should always succeed if the protocol did not change"
                        )
                    }
                };

                handle_ping_response(ping_response, ping_response_signal).await;
            }
            Disconnect::TYPE => {
                info!("Received disconnect");

                let disconnect =
                    Disconnect::try_decode(parts.flags, parts.variable_header_and_payload);
                info!("Disconnect {:?}", disconnect);
                //TODO disconnect TCP connection
            }
            other => info!("Unsupported packet type {}", other),
        }
    }
}

enum PredefinedPublish {
    FanPercentageState {
        setting: SetPoint,
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

enum Message<'a, T: Publish> {
    Subscribe(Subscribe<'a>),
    Publish(T),
    /// Publish messages used internally to manage the MQTT state
    PublishInternal(publish::Publish<'a>),
    #[deprecated(
        note = "Migrate to MQTT not being aware of used messages. Use normal Publish with generic T"
    )]
    PredefinedPublish(PredefinedPublish),
}

async fn talk<T: Publish>(
    writer: &Mutex<CriticalSectionRawMutex, TcpWriter<'_>>,
    outgoing: &Channel<CriticalSectionRawMutex, Message<'_, T>, 8>,
    last_packet: &Signal<CriticalSectionRawMutex, Instant>,
) {
    loop {
        let message = outgoing.receive().await;

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
                    core::str::from_utf8(publish.payload()).unwrap()
                );
                if let Err(error) = send(&mut *writer, publish).await {
                    error!("Error sending publish: {:?}", error);
                    continue;
                }
            }
            Message::PublishInternal(publish) => {
                info!(
                    "Sending internal publish {:?}",
                    core::str::from_utf8(publish.payload).unwrap()
                );
                if let Err(error) = send(&mut *writer, publish).await {
                    error!("Error sending internal publish: {:?}", error);
                    continue;
                }
            }
            Message::PredefinedPublish(publish) => match publish {
                PredefinedPublish::FanPercentageState { setting } => {
                    info!("Sending percentage state publish {}", setting);
                    // let buffer: StringBuffer<5> = setting.into();
                    let buffer = match heapless::String::<5>::try_from(*setting) {
                        Ok(buffer) => buffer,
                        Err(error) => {
                            error!("Error converting setting to string: {:?}", error);
                            continue;
                        }
                    };

                    let packet = publish::Publish {
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
                        publish::Publish {
                            topic_name: "fancontroller/on/state",
                            payload: b"ON",
                        }
                    } else {
                        publish::Publish {
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
                    let packet = publish::Publish {
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

        last_packet.signal(Instant::now());
    }
}

async fn set_up_subscriptions<T: Publish, const SUBSCRIPTIONS: usize>(
    packet_identifier: NonZeroU16,
    acknowledgements: &Mutex<CriticalSectionRawMutex, [bool; SUBSCRIPTIONS]>,
    outgoing: &Channel<CriticalSectionRawMutex, Message<'_, T>, 8>,
    subscriptions: &'static [Subscription<'static>; SUBSCRIPTIONS],
    waker: &AtomicWaker,
) {
    info!("[Subscription] Setting up subscriptions");
    for subscription in subscriptions {
        info!(
            "[Subscription] Subscribing to MQTT topic: {}",
            subscription.topic_filter
        );
    }

    let message = Message::Subscribe(Subscribe {
        subscriptions,
        //TODO free identifier management
        packet_identifier,
    });

    outgoing.send(message).await;

    const TIMEOUT: Duration = Duration::from_secs(30);
    let result = with_timeout(
        TIMEOUT,
        wait_for_acknowledgement(packet_identifier, acknowledgements, waker),
    )
    .await;

    if let Err(error) = result {
        error!(
            "[Subscription] Timed out setting up subscriptions. Waiting for subscription acknowledgements timed out after {:?}: {:?}",
            TIMEOUT, error
        );
        return;
    }
    info!("[Subscription] Set up subscriptions complete")
}

async fn set_up_discovery<T: Publish>(
    outgoing: &Channel<CriticalSectionRawMutex, Message<'_, T>, 8>,
) {
    const DISCOVERY_PAYLOAD: &[u8] = env!("FAN_CONTROLLER_DISCOVERY_PAYLOAD").as_bytes();
    info!(
        "Sending discovery payload to {}",
        topic::fan_controller::DISCOVERY,
    );
    //TODO make constant and refactor to allow people to dynamically subscribe
    let discovery_publish = Message::PublishInternal(publish::Publish {
        topic_name: topic::fan_controller::DISCOVERY,
        payload: DISCOVERY_PAYLOAD,
    });

    outgoing.send(discovery_publish).await;
    //TODO wait for packet acknowledgement
}

/// Keep alive task
async fn keep_alive(
    writer: &Mutex<CriticalSectionRawMutex, TcpWriter<'_>>,
    last_packet: &Signal<CriticalSectionRawMutex, Instant>,
    client_state: &Signal<CriticalSectionRawMutex, ClientState>,
    ping_response: &Signal<CriticalSectionRawMutex, PingResponse>,
) {
    // Send keep alive packets after connection with connect packet is established.
    // Wait for a packet to be sent or the keep alive interval to time the wait out.
    // If the keep alive timed the wait out, send a ping request packet.
    // Else if a packet was sent, wait for the next packet to be sent with the keep alive
    // interval as timeout.

    let start = last_packet.try_take().unwrap_or(Instant::now());
    let mut last_send = start;
    loop {
        // The server waits for 1.5 times the keep alive interval, so being off by a bit due to
        // network, async overhead or the clock not being exactly precise is fine
        let deadline = last_send + configuration::KEEP_ALIVE;
        let result = with_deadline(deadline, last_packet.wait()).await;
        match result {
            Err(TimeoutError) => {
                info!("Sending keep alive ping request");
                let mut writer = writer.lock().await;
                // Send keep alive ping request
                if let Err(error) = send(&mut *writer, PingRequest).await {
                    error!("Error sending keep alive ping request: {:?}", error);
                    client_state.signal(ClientState::ConnectionLost);
                    return;
                }

                // Keep alive is time from when the last packet was sent and not when the ping response
                // was received. Therefore, we need to reset it here
                last_send = Instant::now();

                // Wait for ping response
                if let Err(TimeoutError) =
                    with_timeout(configuration::MQTT_TIMEOUT, ping_response.wait()).await
                {
                    // Assume disconnect from server
                    error!("Timeout waiting for ping response. Disconnecting");
                    client_state.signal(ClientState::ConnectionLost);
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

async fn update_homeassistant<T: Publish>(
    outgoing: &Channel<CriticalSectionRawMutex, Message<'_, T>, 8>,
    receiver: &mut embassy_sync::watch::Receiver<'_, CriticalSectionRawMutex, FanState, 3>,
) {
    // let Some(mut receiver): Option<
    //     embassy_sync::watch::Receiver<'_, CriticalSectionRawMutex, FanState, 3>,
    // > = FAN_CONTROLLER.fan_states.0.receiver() else {
    //     error!("Fan state receiver was not set up. Cannot update Homeassistant");
    //     return;
    // };

    // If there is no initial value this will cause an initial update to homeassistant
    // which is good as we don't know the state on homeassistant
    let mut previous_state = receiver.try_get().unwrap_or(FanState {
        is_on: false,
        setting: SetPoint::ZERO,
    });

    loop {
        let state = receiver.changed().await;

        info!(
            "UPDATING HOMEASSISTANT {}",
            if state.is_on { "ON" } else { "OFF" }
        );
        let message =
            Message::PredefinedPublish(PredefinedPublish::FanOnState { is_on: state.is_on });
        outgoing.send(message).await;

        // Update setting before is on state for smoother transition in homeassistant UI
        info!("UPDATING HOMEASSISTANT {}", state.setting);
        let message = Message::PredefinedPublish(PredefinedPublish::FanPercentageState {
            setting: state.setting,
        });
        outgoing.send(message).await;

        previous_state = state;
    }
}

async fn poll_sensors(fans: ModbusOnceLock) {
    loop {
        let mut fans = fans.get().await.lock().await;
        let fans = fans.deref_mut();

        let temperature = match fans.get_temperature(fan::Fan::One).await {
            Ok(temperature) => temperature,
            Err(modbus::client::Error::Timeout(_)) => {
                error!("Timeout getting temperature for fan 1");
                return;
            }
            Err(modbus::client::Error::Uart(error)) => {
                error!("Uart error getting temperature for fan 1: {:?}", error);
                return;
            }
        };

        Timer::after_secs(10).await;
        todo!("Send temperature to Home Assistant");
    }
}

const SUBSCRIBE_OPTIONS: mqtt::packet::subscribe::Options = mqtt::packet::subscribe::Options::new(
    QualityOfService::AtMostOnceDelivery,
    false,
    false,
    // mqtt::packet::subscribe::RetainHandling::DoNotSend,
    mqtt::packet::subscribe::RetainHandling::SendAtSubscribe,
);

const SUBSCRIPTIONS_LENGTH: usize = 3;
// Subscribe to home assistant topics
const SUBSCRIPTIONS: [Subscription; SUBSCRIPTIONS_LENGTH] = [
    Subscription {
        topic_filter: topic::fan_controller::COMMAND,
        options: SUBSCRIBE_OPTIONS,
    },
    Subscription {
        topic_filter: topic::fan_controller::fan_1::COMMAND,
        options: SUBSCRIBE_OPTIONS,
    },
    Subscription {
        topic_filter: topic::fan_controller::fan_2::COMMAND,
        options: SUBSCRIBE_OPTIONS,
    },
];

/// Trait must be implemented by types that represent messages that can be published to MQTT.
pub(super) trait Publish {
    const TYPE: u8 = 3;
    /// Providing the topic string for the publish through a function allows implementing types to be more flexible.
    /// For example, they can be defined as an enum and match internally to provide the appropriate string for the enum variant.
    fn topic(&self) -> &str;
    fn payload(&self) -> &[u8];
}

pub(super) async fn set_up_network_stack(
    spawner: Spawner,
    pwr_pin: PIN_23,
    cs_pin: PIN_25,
    pio: PIO0,
    dma: DMA_CH0,
    dio: impl PioPin,
    clk: impl PioPin,
) -> &'static Stack<NetDriver<'static>> {
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
    join_wifi_network(&mut control).await;

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

    stack
}

async fn join_wifi_network(control: &mut Control<'_>) {
    loop {
        match control
            .join_wpa2(configuration::WIFI_NETWORK, configuration::WIFI_PASSWORD)
            .await
        {
            Ok(_) => break,
            Err(error) => info!("Error joining Wi-Fi network with status: {}", error.status),
        }
    }
}

async fn resolve_mqtt_broker_address<'a, D>(
    driver: &'a Stack<D>,
    mqtt_broker_address: &str,
) -> IpAddress
where
    D: Driver + 'static,
{
    let dns_client = DnsSocket::new(driver);

    loop {
        //TODO support IPv6
        let result = dns_client.query(mqtt_broker_address, DnsQueryType::A).await;

        let mut addresses = match result {
            Ok(addresses) => addresses,
            Err(error) => {
                info!(
                    "Error resolving MQTT broker IP address with {}: {:?}",
                    mqtt_broker_address, error,
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

        return addresses.swap_remove(0);
    }
}

/// Set in Homeassitant under Settings > People > Users Tab. Not to be confused with the People tab.
/// The Users Tab might only be visible in advanced mode as administrator.
/// A separate account is recommended for each device.
pub(crate) struct MqttBrokerCredentials<'a> {
    pub(crate) username: &'a str,
    pub(crate) password: &'a [u8],
}

pub(super) struct MqttBrokerConfiguration<'a> {
    pub(super) client_identifier: &'a str,
    pub(super) address: &'a str,
    pub(super) credentials: MqttBrokerCredentials<'a>,
    pub(super) keep_alive_seconds: Duration,
}

pub(super) async fn mqtt_with_connect<
    'tcp,
    'sender,
    'receiver,
    'configuration,
    FromError,
    Receive: for<'a> TryFrom<publish::Publish<'a>, Error = FromError>,
    const RECEIVE: usize,
    Send: Publish,
    const SEND: usize,
    D: Driver + 'static,
>(
    stack: &Stack<D>,
    sender: channel::Sender<'sender, CriticalSectionRawMutex, Result<Receive, FromError>, RECEIVE>,
    receiver: channel::Receiver<'receiver, CriticalSectionRawMutex, Send, SEND>,
    mqtt_broker_configuration: &MqttBrokerConfiguration<'configuration>,
)
// -> Client<'tcp, 'sender, 'receiver, Send, SEND, Receive, RECEIVE>
{
    // Now we can use it
    let mut receive_buffer = [0; 1024];
    let mut send_buffer = [0; 1024];
    let mut socket = TcpSocket::new(stack, &mut receive_buffer, &mut send_buffer);

    info!("Resolving MQTT broker IP address");
    // Get home assistant MQTT broker IP address
    let address = resolve_mqtt_broker_address(stack, mqtt_broker_configuration.address).await;
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

    use crate::mqtt::task;

    //TODO error handling
    let packet = defmt::unwrap!(Connect::try_from(mqtt_broker_configuration));

    info!("Establishing MQTT connection");
    let (mut reader, mut writer) = socket.split();

    if let Err(error) = task::connect(&mut writer, &mut reader, packet).await {
        warn!("Error connecting to MQTT broker: {:?}", error);
        return;
    };
    info!("MQTT connection established");

    //TODO yes static "global" state is bad, but I am still learning how to use wakers and polling
    // with futures so this will be refactored when I made it work
    // Contains the status of the subscribe packets send out. The packet identifier represents the
    // index in the array
    let acknowledgements: Mutex<CriticalSectionRawMutex, [bool; SUBSCRIPTIONS_LENGTH]> =
        Mutex::new([false; SUBSCRIPTIONS_LENGTH]);
    // The waker needs to be woken to complete the subscribe acknowledgement future.
    // The embassy documentation does not explain when to use [`AtomicWaker`] but I am assuming
    // it is useful for cases like this where I need to mutate a static.
    let waker: AtomicWaker = AtomicWaker::new();

    let ping_response: Signal<CriticalSectionRawMutex, PingResponse> = Signal::new();

    let client_state: Signal<CriticalSectionRawMutex, ClientState> = Signal::new();

    let outgoing: Channel<CriticalSectionRawMutex, Message<Send>, 8> = Channel::new();
    // The instant when the last packet was sent to determine when the next keep alive has to be sent
    let last_packet: Signal<CriticalSectionRawMutex, Instant> = Signal::new();

    // Using a mutex for the writer, so it can be shared between the task that sends messages (for
    // subscribing and publishing fan speed updates) and the task that sends the keep alive ping
    let writer = Mutex::<CriticalSectionRawMutex, TcpWriter<'_>>::new(writer);

    // Future 1
    let listen = listen(
        &mut reader,
        &sender,
        &client_state,
        &acknowledgements,
        &ping_response,
    );

    // Future 2
    let talk = talk(&writer, &outgoing, &last_packet);

    // Future 3
    let set_up = join(
        set_up_subscriptions(
            non_zero_u16!(1),
            &acknowledgements,
            &outgoing,
            &SUBSCRIPTIONS,
            &waker,
        ),
        set_up_discovery(&outgoing),
    );

    // Future 4
    let keep_alive = keep_alive(&writer, &last_packet, &client_state, &ping_response);

    // Future 5 update homeassistant when change occurs
    // let update_homeassistant = update_homeassistant(&outgoing, &mut receiver);

    // Future 6
    // let poll_sensors = poll_sensors(fans);

    //TODO cancel all tasks when client loses connection

    join4(
        listen, talk, keep_alive,
        set_up,
        // Join because there is only a join with max 5 arguments 😬
        // join(update_homeassistant(), poll_sensors(fans)),
        // update_homeassistant,
    )
    .await;
}
