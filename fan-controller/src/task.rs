use crate::mqtt::packet::connect::Connect;
use crate::mqtt::packet::disconnect::Disconnect;
use crate::mqtt::packet::ping_response::PingResponse;
use crate::mqtt::packet::publish::Publish;
use crate::mqtt::packet::subscribe::{Subscribe, Subscription};
use crate::mqtt::packet::subscribe_acknowledgement::SubscribeAcknowledgement;
use crate::mqtt::packet::{get_parts, ping_response};
use crate::mqtt::task::send;
use crate::mqtt::{non_zero_u16, TryDecode};
use crate::Fans;
use crate::PingRequest;
use crate::{configuration, fan, gain_control, FanState, FAN_CONTROLLER};
use crate::{mqtt, FanController};
use ::mqtt::QualityOfService;
use core::future::poll_fn;
use core::num::NonZeroU16;
use core::ops::DerefMut;
use core::task::Poll;
use cyw43::NetDriver;
use defmt::{error, info, unwrap, warn};
use embassy_executor::Spawner;
use embassy_futures::join::{join, join5};
use embassy_net::dns::{DnsQueryType, DnsSocket};
use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};
use embassy_net::{Config, IpEndpoint, Stack, StackResources};
use embassy_rp::clocks::RoscRng;
use embassy_rp::peripherals::{DMA_CH0, PIN_23, PIN_25, PIO0};
use embassy_rp::pio::PioPin;
use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::Mutex;
use embassy_sync::signal::Signal;
use embassy_sync::waitqueue::AtomicWaker;
use embassy_time::{with_deadline, with_timeout, Instant, Timer};
use rand::RngCore;
use static_cell::StaticCell;
use topic::fan_controller;

#[embassy_executor::task]
async fn network_task(stack: &'static Stack<NetDriver<'static>>) -> ! {
    stack.run().await
}

async fn handle_subscribe_acknowledgement<'f>(
    acknowledgement: &'f SubscribeAcknowledgement<'f>,
    acknowledgements: &Mutex<CriticalSectionRawMutex, [bool; 2]>,
) {
    info!("Received subscribe acknowledgement");
    let mut acknowledgements = acknowledgements.lock().await;
    info!("Locked ACKNOWLEDGEMENTS");
    // Validate server sends a valid packet identifier or we get bamboozled and panic
    let Some(value) = acknowledgements.get_mut(acknowledgement.packet_identifier as usize) else {
        warn!("Received subscribe acknowledgement for out of bounds packet identifier");
        return;
    };

    info!(
        "Received acknowledgement {} Reason codes: {:#04x}",
        acknowledgement.packet_identifier, acknowledgement.reason_codes
    );
}

async fn wait_for_acknowledgement(
    packet_identifier: NonZeroU16,
    acknowledgements: &Mutex<CriticalSectionRawMutex, [bool; 2]>,
    waker: &AtomicWaker,
) {
    info!("Waiting for subscribe acknowledgements");
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

    info!("Subscribe acknowledgement received")
}

/// A handler that takes MQTT publishes and sets the fan settings accordingly
async fn handle_publish<'f>(
    publish: &'f Publish<'f>,
    sender: embassy_sync::watch::Sender<'_, CriticalSectionRawMutex, FanState, 3>,
) {
    info!("Handling publish");

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

async fn listen<'reader>(
    reader: &mut TcpReader<'reader>,
    fan_controller: &'static FanController,
    client_state: &Signal<CriticalSectionRawMutex, ClientState>,
    acknowledgements: &Mutex<CriticalSectionRawMutex, [bool; 2]>,
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
            Publish::TYPE => {
                info!("Received publish");
                let publish =
                    match Publish::try_decode(parts.flags, &parts.variable_header_and_payload) {
                        Ok(publish) => publish,
                        Err(error) => {
                            error!("Error reading publish: {:?}", error);
                            continue;
                        }
                    };

                let sender = fan_controller.fan_states.0.sender();
                handle_publish(&publish, sender).await;
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

                handle_subscribe_acknowledgement(&subscribe_acknowledgement, acknowledgements)
                    .await;
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

                handle_ping_response(ping_response, ping_response_signal).await;
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

async fn talk(
    writer: &Mutex<CriticalSectionRawMutex, TcpWriter<'_>>,
    outgoing: &Channel<CriticalSectionRawMutex, Message<'_>, 8>,
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

        last_packet.signal(Instant::now());
    }
}

async fn set_up_subscriptions(
    packet_identifier: NonZeroU16,
    acknowledgements: &Mutex<CriticalSectionRawMutex, [bool; 2]>,
    outgoing: &Channel<CriticalSectionRawMutex, Message<'_>, 8>,
    subscriptions: &'static [Subscription<'static>],
    waker: &AtomicWaker,
) {
    info!("Setting up subscriptions");
    let message = Message::Subscribe(Subscribe {
        subscriptions: subscriptions,
        //TODO free identifier management
        packet_identifier,
    });

    outgoing.send(message).await;

    wait_for_acknowledgement(packet_identifier, &acknowledgements, &waker).await;
    info!("Set up subscriptions complete")
}

async fn set_up_discovery(outgoing: &Channel<CriticalSectionRawMutex, Message<'_>, 8>) {
    const DISCOVERY_PAYLOAD: &[u8] = env!("FAN_CONTROLLER_DISCOVERY_PAYLOAD").as_bytes();
    const DISCOVERY_PUBLISH: Message = Message::Publish(Publish {
        topic_name: configuration::DISCOVERY_TOPIC,
        payload: DISCOVERY_PAYLOAD,
    });

    outgoing.send(DISCOVERY_PUBLISH).await;
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

async fn update_homeassistant(
    outgoing: &Channel<CriticalSectionRawMutex, Message<'_>, 8>,
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

async fn poll_sensors(fans: Fans) {
    loop {
        let mut fans = fans.lock().await;
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
        todo!("Send temperature to Home Assistant");
    }
}

#[embassy_executor::task]
pub(super) async fn mqtt(
    spawner: Spawner,
    pwr_pin: PIN_23,
    cs_pin: PIN_25,
    pio: PIO0,
    dma: DMA_CH0,
    dio: impl PioPin,
    clk: impl PioPin,
    fans: &'static Fans,
    fan_controller: &'static FanController,
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

    use crate::mqtt::task;
    let packet = Connect {
        client_identifier: env!("CARGO_PKG_NAME"),
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

    static PING_RESPONSE: Signal<CriticalSectionRawMutex, PingResponse> = Signal::new();

    static CLIENT_STATE: Signal<CriticalSectionRawMutex, ClientState> = Signal::new();

    let (mut reader, mut writer) = socket.split();
    // Future 1
    let listen = listen(
        &mut reader,
        fan_controller,
        &CLIENT_STATE,
        &ACKNOWLEDGEMENTS,
        &PING_RESPONSE,
    );

    static OUTGOING: Channel<CriticalSectionRawMutex, Message, 8> = Channel::new();
    /// The instant when the last packet was sent to determine when the next keep alive has to be sent
    static LAST_PACKET: Signal<CriticalSectionRawMutex, Instant> = Signal::new();

    // Using a mutex for the writer, so it can be shared between the task that sends messages (for
    // subscribing and publishing fan speed updates) and the task that sends the keep alive ping
    let writer = Mutex::<CriticalSectionRawMutex, TcpWriter<'_>>::new(writer);
    // Future 2
    let talk = talk(&writer, &OUTGOING, &LAST_PACKET);

    // Subscribe to home assistant topics
    const SUBSCRIPTIONS: [Subscription; 2] = [
        Subscription {
            topic_filter: topic::fan_controller::COMMAND,
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

    // Future 3
    let set_up = join(
        set_up_subscriptions(
            non_zero_u16!(1),
            &ACKNOWLEDGEMENTS,
            &OUTGOING,
            &SUBSCRIPTIONS,
            &WAKER,
        ),
        set_up_discovery(&OUTGOING),
    );

    // Future 4
    let keep_alive = keep_alive(&writer, &LAST_PACKET, &CLIENT_STATE, &PING_RESPONSE);

    let Some(mut receiver) = fan_controller.fan_states.0.receiver() else {
        error!("Fan states were not set up. Cannot receive fan state changes");
        return;
    };

    // Future 5 update homeassistant when change occurs
    let update_homeassistant = update_homeassistant(&OUTGOING, &mut receiver);

    // Future 6
    // let poll_sensors = poll_sensors(fans);

    //TODO cancel all tasks when client loses connection
    join5(
        listen,
        talk,
        keep_alive,
        set_up,
        // Join because there is only a join with max 5 arguments 😬
        // join(update_homeassistant(), poll_sensors(fans)),
        update_homeassistant,
    )
    .await;
}
