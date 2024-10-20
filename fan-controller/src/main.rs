#![no_std]
#![no_main]
#![allow(warnings)]

use core::future::{poll_fn, Future};
use core::ops::{Deref, DerefMut, Sub};
use core::pin::pin;
use core::task::Poll;
use crc::{Crc, CRC_16_MODBUS};
use cyw43::{Control, NetDriver};
use cyw43_pio::PioSpi;
use defmt::*;
use embassy_executor::Spawner;
use embassy_futures::join::{join, join3};
use embassy_net::dns::{DnsQueryType, DnsSocket};
use embassy_net::tcp::client::{TcpClient, TcpClientState};
use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};
use embassy_net::{tcp, Config, IpAddress, IpEndpoint, Stack, StackResources};
use embassy_rp::clocks::RoscRng;
use embassy_rp::gpio::{Input, Level, Output, Pin, Pull};
use embassy_rp::peripherals::{
    DMA_CH0, PIN_12, PIN_13, PIN_18, PIN_23, PIN_24, PIN_25, PIN_29, PIN_4, PIO0, UART0,
};
use embassy_rp::pio::{InterruptHandler as PioInterruptHandler, Pio, PioPin};
use embassy_rp::uart::{InterruptHandler as UartInterruptHandler, Uart};
use embassy_rp::{bind_interrupts, dma, pio, uart, Peripheral, Peripherals};
use embassy_sync::blocking_mutex::raw::{CriticalSectionRawMutex, NoopRawMutex};
use embassy_sync::channel;
use embassy_sync::channel::Channel;
use embassy_sync::mutex::{Mutex, MutexGuard, TryLockError};
use embassy_sync::pubsub::PubSubChannel;
use embassy_sync::signal::Signal;
use embassy_sync::waitqueue::AtomicWaker;
use embassy_time::{with_deadline, with_timeout, Duration, Instant, Ticker, TimeoutError, Timer};
use embedded_io_async::{Read, Write};
use embedded_nal_async::{AddrType, Dns, SocketAddr, TcpConnect};
use mqtt::client::ConnectError;
use rand::RngCore;
use reqwless::client::{TlsConfig, TlsVerify};
use static_cell::StaticCell;

use {defmt_rtt as _, panic_probe as _};

use self::mqtt::packet;
use self::mqtt::packet::Packet;
use crate::async_callback::AsyncCallback;
use crate::mqtt::client::runner::State;
use crate::mqtt::connect::Connect;
use crate::mqtt::connect_acknowledgement::ConnectReasonCode;
use crate::mqtt::packet::{get_parts, FromPublish, FromSubscribeAcknowledgement};
use crate::mqtt::ping_request::PingRequest;
use crate::mqtt::ping_response::PingResponse;
use crate::mqtt::publish::Publish;
use crate::mqtt::subscribe::{Subscribe, Subscription};
use crate::mqtt::subscribe_acknowledgement::SubscribeAcknowledgement;
use crate::mqtt::task::{send, Encode};
use crate::mqtt::QualityOfService;
use crate::mqtt::{connect, publish, subscribe};
use fan::FanClient;

mod async_callback;
mod configuration;
mod fan;
mod modbus;
mod mqtt;

bind_interrupts!(struct Irqs {
    PIO0_IRQ_0 => PioInterruptHandler<PIO0>;
    UART0_IRQ  => UartInterruptHandler<UART0>;
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
        client_identifier: "testfan",
        username: configuration::MQTT_BROKER_USERNAME,
        password: configuration::MQTT_BROKER_PASSWORD,
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
        // Validate server sends a valid packet identifier or we get bamboozled and panic
        let Some(value) = acknowledgements.get_mut(acknowledgement.packet_identifier as usize)
        else {
            warn!("Received subscribe acknowledgement for out of bounds packet identifier");
            return;
        };
    }

    async fn wait_for_acknowledgement<const PACKET_IDENTIFIER: u16>() {
        //TODO if this function gets called multiple times it might never be woken up because there
        // is only one waker and the other call of this function will lock the mutex. To solve this
        // we could use structs from [embassy-sync::waitqueue] and/or a blocking mutex to remove the
        // try_lock which is used because the lock function is async and we can not easily await here
        poll_fn(|context| match ACKNOWLEDGEMENTS.try_lock() {
            Ok(mut guard) => {
                let packet = guard.get_mut(PACKET_IDENTIFIER as usize).unwrap();
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
    }

    /// A handler that takes MQTT publishes and sets the fan settings accordingly
    async fn handle_publish<'f>(publish: &'f Publish<'f>) {
        info!("Received publish");
        // This part is not MQTT and application specific
        match publish.topic_name {
            "testfan/speed/percentage" => {
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

                let Ok(setting) = FanSetting::new(set_point) else {
                    warn!(
                        "Setting fan speed out of bounds. Not accepting new setting: {}",
                        set_point
                    );
                    return;
                };

                let fans = FANS.lock().await;
            }

            other => info!(
                "Unexpected topic: {} with payload: {}",
                other, publish.payload
            ),
        }
    }

    static PING_RESPONSE: Signal<CriticalSectionRawMutex, PingResponse> = Signal::new();
    /// Callback handler for [listen](crate::listen)
    async fn handle_ping_response(ping_response: PingResponse) {
        info!("Received ping response");
        PING_RESPONSE.signal(ping_response);
    }

    async fn listen<'reader, FS, FP, FR, F>(
        reader: &mut TcpReader<'reader>,
        on_subscribe_acknowledgement: FS,
        on_publish: FP,
        on_ping_response: FR,
    ) where
        FS: for<'a> AsyncCallback<'a, SubscribeAcknowledgement<'a>>,
        FP: for<'a> AsyncCallback<'a, Publish<'a>>,
        FR: Fn(PingResponse) -> F,
        F: Future<Output = ()>,
    {
        let mut buffer = [0; 1024];
        loop {
            let bytes_read = reader.read(&mut buffer).await.unwrap();
            let parts = get_parts(&buffer[..bytes_read]).unwrap();
            match parts.r#type {
                Publish::TYPE => {
                    let publish =
                        Publish::read(parts.flags, &parts.variable_header_and_payload).unwrap();

                    on_publish.call(&publish).await;
                }
                SubscribeAcknowledgement::TYPE => {
                    let subscribe_acknowledgement =
                        SubscribeAcknowledgement::read(&parts.variable_header_and_payload).unwrap();

                    on_subscribe_acknowledgement
                        .call(&subscribe_acknowledgement)
                        .await;
                }

                other => info!("Unsupported packet type {}", other),
            }
        }
    }

    let (mut reader, mut writer) = socket.split();
    // Future 1
    let listen = listen(
        &mut reader,
        handle_subscribe_acknowledgement,
        handle_publish,
        handle_ping_response,
    );

    enum Message<'a> {
        Subscribe(Subscribe<'a>),
        Publish(Publish<'a>),
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
                    send(&mut *writer, subscribe).await.unwrap();
                }
                Message::Publish(publish) => {
                    send(&mut *writer, publish).await.unwrap();
                }
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

    async fn set_up_subscriptions<const PACKET_IDENTIFIER: u16>() {
        let message = Message::Subscribe(Subscribe {
            subscriptions: &SUBSCRIPTIONS,
            //TODO free identifier management
            packet_identifier: PACKET_IDENTIFIER,
        });

        OUTGOING.send(message).await;

        wait_for_acknowledgement::<PACKET_IDENTIFIER>().await;
    }

    // Future 3
    let set_up = set_up_subscriptions::<0>();

    enum ClientState {
        Disconnected,
        Connected,
        ConnectionLost,
    }
    static CLIENT_STATE: Signal<CriticalSectionRawMutex, ClientState> = Signal::new();

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
                    let mut writer = writer.lock().await;
                    // Send keep alive ping request
                    send(&mut *writer, PingRequest).await.unwrap();
                    // Keep alive is time from when the last packet was sent and not when the ping response
                    // was received. Therefore, we need to reset it here
                    last_send = Instant::now();

                    // Wait for ping response
                    if let Err(TimeoutError) =
                        with_timeout(configuration::TIMEOUT, PING_RESPONSE.wait()).await
                    {
                        // Assume disconnect from server
                        error!("Timeout waiting for ping response. Disconnecting");
                        CLIENT_STATE.signal(ClientState::ConnectionLost);
                        return;
                    }
                }
                Ok(last_packet) => {
                    last_send = last_packet;
                }
            }
        }
    }

    //TODO cancel all tasks when client loses connection
    // join3(listen, talk, set_up).await;
}

enum PublishReceiveError {
    ReadError(tcp::Error),
    ReadPacketError(packet::ReadError),
}
async fn mqtt_routine<'a>(mut socket: TcpSocket<'a>) {
    core::todo!()

    // info!("Sending MQTT connect packet");
    // // No experience how big this buffer should be
    // let mut send_buffer = [0; 256];
    // // Send MQTT connect packet
    // let packet = Connect {
    //     client_identifier: "testfan",
    //     username: MQTT_BROKER_USERNAME,
    //     password: MQTT_BROKER_PASSWORD,
    //     keep_alive_seconds: KEEP_ALIVE.as_secs() as u16,
    // };

    // let mut offset = 0;
    // packet
    //     .write(&mut send_buffer, &mut offset)
    //     .map_err(MqttError::WriteConnectError)?;

    // socket
    //     .write_all(&send_buffer[..offset])
    //     .await
    //     .map_err(MqttError::WriteError)?;
    // socket.flush().await.map_err(MqttError::FlushError)?;

    // info!("Sent MQTT connect packet");

    // info!("Waiting for MQTT connect acknowledgement");

    // // Wait for connection acknowledgement
    // let mut receive_buffer = [0; 1024];
    // let bytes_read = socket
    //     .read(&mut receive_buffer)
    //     .await
    //     .map_err(MqttError::ReadError)?;
    // let packet = Packet::read(&receive_buffer[..bytes_read]).map_err(MqttError::ReadPacketError)?;

    // match packet {
    //     Packet::ConnectAcknowledgement(acknowledgement) => {
    //         if let ConnectReasonCode::ErrorCode(error_code) = acknowledgement.connect_reason_code {
    //             return Err(MqttError::ConnectError(error_code));
    //         }
    //     }
    //     unexpected => {
    //         // Unexpected packet type -> disconnect
    //         info!("Unexpected packet");
    //         return Err(MqttError::UnexpectedPacketType(unexpected.get_type()));
    //     }
    // }

    // // Start keep alive after connect acknowledgement?
    // //TODO use keep alive interval received from server in connect acknowledgement
    // let mut keep_alive = Ticker::every(KEEP_ALIVE);

    // // Subscribe to home assistant topics
    // // Before sending discovery packet
    // let subscriptions = [
    //     Subscription {
    //         topic_filter: "testfan/on/set",
    //         options: mqtt::subscribe::Options::new(
    //             QualityOfService::AtMostOnceDelivery,
    //             false,
    //             false,
    //             mqtt::subscribe::RetainHandling::DoNotSend,
    //         ),
    //     },
    //     Subscription {
    //         topic_filter: "testfan/speed/percentage",
    //         options: mqtt::subscribe::Options::new(
    //             QualityOfService::AtMostOnceDelivery,
    //             false,
    //             false,
    //             mqtt::subscribe::RetainHandling::DoNotSend,
    //         ),
    //     },
    // ];

    // //TODO packet identifier management
    // let packet = Subscribe {
    //     subscriptions,
    //     packet_identifier: 42,
    // };

    // offset = 0;
    // let mut last_send = Instant::now();
    // packet
    //     .write(&mut send_buffer, &mut offset)
    //     .map_err(MqttError::WriteSubscribeError)?;
    // socket
    //     .write_all(&send_buffer[..offset])
    //     .await
    //     .map_err(MqttError::WriteError)?;
    // socket.flush().await.map_err(MqttError::FlushError)?;
    // keep_alive.reset();

    // // Read subscribe acknowledgement
    // let bytes_read = socket
    //     .read(&mut receive_buffer)
    //     .await
    //     .map_err(MqttError::ReadError)?;

    // let packet = Packet::read(&receive_buffer[..bytes_read]).map_err(MqttError::ReadPacketError)?;
    // match packet {
    //     Packet::SubscribeAcknowledgement(_acknowledgement) => {
    //         // Ignoring acknowledgement errors for now as their information is only useful for debugging currently
    //     }
    //     unexpected => {
    //         // Unexpected packet type -> disconnect
    //         info!("Unexpected packet");

    //         return Err(MqttError::UnexpectedPacketType(unexpected.get_type()));
    //     }
    // }

    // //TODO listen for publishes to topics from here on out
    // let (reader, writer) = socket.split();

    // let (send_result, listen_result) = join(
    //     send_discovery_and_keep_alive(writer, send_buffer, keep_alive),
    //     listen_for_publishes(reader, receive_buffer),
    // )
    // .await;
    // Ok(())
}

async fn listen_for_publishes<T, S>(
    mut reader: TcpReader<'_>,
    mut buffer: [u8; 1024],
) -> Result<(), PublishReceiveError>
where
    T: FromPublish,
    S: FromSubscribeAcknowledgement,
{
    loop {
        // Should read at least one byte
        let bytes_read = reader
            .read(&mut buffer)
            .await
            .map_err(PublishReceiveError::ReadError)?;

        let packet = Packet::<T, S>::read(&buffer[..bytes_read])
            .map_err(PublishReceiveError::ReadPacketError)?;
        match packet {
            //TODO handle disconnect packet
            packet @ Packet::ConnectAcknowledgement(_)
            | packet @ Packet::SubscribeAcknowledgement(_) => {
                warn!("Received unexpected packet type: {}", packet.get_type());
            }
            Packet::Publish(publish) => {
                info!("Received publish");
                // This part is not MQTT and application specific
                // match publish.topic_name {
                //     "testfan/speed/percentage" => {
                //         let payload = match core::str::from_utf8(&publish.payload) {
                //             Ok(payload) => payload,
                //             Err(error) => {
                //                 warn!("Expected percentage_command_topic payload (speed percentage) to be a valid UTF-8 string with a number");
                //                 continue;
                //             }
                //         };
                //
                //         // And then to an integer...
                //         let set_point = payload.parse::<u16>();
                //         let set_point = match set_point {
                //             Ok(set_point) => set_point,
                //             Err(error) => {
                //                 warn!("Expected speed percentage to be a number string. Payload is: {}", payload);
                //                 continue;
                //             }
                //         };
                //
                //         let Ok(setting) = FanSetting::new(set_point) else {
                //             warn!(
                //                 "Setting fan speed out of bounds. Not accepting new setting: {}",
                //                 set_point
                //             );
                //             continue;
                //         };
                //
                //         //TODO confirm there was no error on the modbus side
                //         // MODBUS_SIGNAL.signal(setting);
                //     }
                //     "testfan/on/set" => {
                //         let payload = match core::str::from_utf8(publish.payload) {
                //             Ok(payload) => payload,
                //             Err(error) => {
                //                 warn!(
                //                     "Expected command_topic payload to be a valid UTF-8 string with either \"ON\" or \"OFF\". Payload is: {}",
                //                     publish.payload
                //                 );
                //                 continue;
                //             }
                //         };
                //
                //         match payload {
                //             "ON" => {
                //                 //TODO set to last known state before off
                //                 println!("Turning on");
                //             }
                //             "OFF" => {
                //                 println!("Turning off");
                //                 // MODBUS_SIGNAL.signal(FanSetting::ZERO);
                //             }
                //             unknown => {
                //                 warn!(
                //                     "Expected either \"ON\" or \"OFF\" but received: {}",
                //                     unknown
                //                 );
                //             }
                //         }
                //     }
                //     unknown => {
                //         warn!(
                //             "Unexpected topic: {} with payload: {}",
                //             publish.topic_name, publish.payload
                //         );
                //     }
                // }
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

    // Using abbreviations to save space of binary and on the wire (haven't measured the effect though...)
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
        topic_name: configuration::DISCOVERY_TOPIC,
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
        let _ = PingRequest.encode(&mut send_buffer, &mut offset);
        writer
            .write_all(&send_buffer[..offset])
            .await
            .map_err(MqttError::WriteError)?;
        writer.flush().await.map_err(MqttError::FlushError)?;
        //TODO read ping response or disconnect after "a reasonable amount of time"
    }
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

        let mut fans = FANS.lock().await;
        let Some(fan) = fans.deref_mut() else {
            warn!("No fan client found");
            continue;
        };

        fan.set_set_point(set_point).await;
    }
}

type Fans = Mutex<CriticalSectionRawMutex, Option<FanClient<'static, UART0, PIN_4>>>;
/// Use this to make calls to the fans through modbus
static FANS: Fans = Mutex::new(None);
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
        PIN_4: pin_4,
        UART0: uart0,
        PIN_12: pin_12,
        PIN_13: pin_13,
        PIN_18: pin_18,
        ..
    } = embassy_rp::init(Default::default());

    // UART

    let client = FanClient::new(uart0, pin_12, pin_13, Irqs, dma_ch1, dma_ch2, pin_4);
    // Inner scope to drop the guard after assigning
    {
        *(FANS.lock().await) = Some(client);
    }

    // Button input task waits for button presses and send according signals to the modbus task
    unwrap!(spawner.spawn(input_task(pin_18)));
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
        assert_eq!(FanSetting::new(64_000), Ok(FanSetting(64_000)));
        assert_eq!(FanSetting::new(64_000 + 1), Err(SetPointOutOfBoundsError));
        assert_eq!(FanSetting::new(u16::MAX), Err(SetPointOutOfBoundsError));
    }
}
