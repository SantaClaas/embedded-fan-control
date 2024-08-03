use std::env;
use std::net::{AddrParseError, ToSocketAddrs};
use std::str::Utf8Error;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Interval;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::rustls::pki_types::{InvalidDnsNameError, ServerName};
use tokio_rustls::TlsConnector;

use packet::{ConnectErrorReasonCode, ConnectReasonCode, UnknownConnectReasonCode};
use variable_byte_integer::{DecodeVariableByteIntegerError, VariableByteInteger};

use crate::packet::{QualityOfService, RetainHandling, Subscription, SubscriptionOptions};

mod packet;
mod variable_byte_integer;

/// We send a ping request every 60 seconds to keep the connection alive
/// The server will wait 1.5 times the keep alive time before disconnecting
const KEEP_ALIVE_SECONDS: u16 = 60;

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("Failed to create address")]
    CreateAddressError(#[from] AddrParseError),
    #[error("Could not create server name")]
    CreateServerNameError(#[from] InvalidDnsNameError),
    #[error("Failed to connect to broker")]
    ConnectError(#[from] std::io::Error),
    #[error("Failed to load .env file and environment variables")]
    DotenvError(#[from] dotenvy::Error),
    #[error("Error reading environment variable")]
    EnvVarError(#[from] env::VarError),

    #[error("Port environment variable is not a valid number")]
    PortEnvVarError(#[from] std::num::ParseIntError),
}

async fn set_up_tls_connection(
    broker_address: String,
    port: u16,
) -> Result<impl AsyncReadExt + AsyncWriteExt, AppError> {
    let mut root_certificate_store = RootCertStore::empty();
    root_certificate_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    // root_certificate_store.add(CertificateDer::from(certificate)).unwrap();
    let configuration = ClientConfig::builder()
        .with_root_certificates(root_certificate_store)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(configuration));

    let address = (broker_address.as_ref(), port)
        .to_socket_addrs()?
        .next()
        .unwrap();

    let server_name = ServerName::try_from(broker_address)?;

    let stream = TcpStream::connect(&address).await?;

    let stream = connector.connect(server_name, stream).await?;

    Ok(stream)
}

/// Errors while reading the Connect Acknowledgement packet (CONNACK)
#[derive(Debug, thiserror::Error)]
enum ConnectAcknowledgementError {
    #[error("Error while reading from the stream: {0}")]
    ReadError(#[from] std::io::Error),
    /// The server sent a different packet type than connect acknowledgement (2)
    #[error("Unexpected packet type: {0:#04x}")]
    UnexpectedPacketType(u8),
    #[error("Invalid remaining length: {0}")]
    InvalidRemainingLength(DecodeVariableByteIntegerError),
    #[error("Invalid reason code: {0}")]
    InvalidReasonCode(UnknownConnectReasonCode),
    #[error("Error reason code: {0:?}")]
    ErrorReasonCode(ConnectErrorReasonCode),
}

#[derive(Debug)]
enum ConnectError {
    SendError(std::io::Error),
    FlushError(std::io::Error),
    AcknowledgementError(ConnectAcknowledgementError),
}

/// Errors while reading the Subscribe Acknowledgement packet (SUBACK)
#[derive(Debug, thiserror::Error)]
enum SubscribeAcknowledgementError {
    #[error("Error while reading from the stream: {0}")]
    ReadError(#[from] std::io::Error),
    /// The server sent a different packet type than connect acknowledgement (2)
    #[error("Unexpected packet type: {0:#04x}")]
    UnexpectedPacketType(u8),
    #[error("Invalid remaining length: {0}")]
    InvalidRemainingLength(DecodeVariableByteIntegerError),
    #[error("Invalid properties length: {0}")]
    InvalidPropertiesLength(DecodeVariableByteIntegerError),
    #[error("Invalid reason code length: expected {expected} topic(s), got {actual}")]
    InvalidReasonCodeLength { expected: usize, actual: usize },
}

#[derive(Debug)]
enum SubscribeError {
    SendError(std::io::Error),
    FlushError(std::io::Error),
    AcknowledgementError(SubscribeAcknowledgementError),
}

enum MqttActorMessage {
    Connect {
        client_identifier: Arc<str>,
        username: Arc<str>,
        password: Arc<[u8]>,
        responder: oneshot::Sender<Result<(), ConnectError>>,
    },
    Publish {
        topic: Arc<str>,
        payload: Arc<[u8]>,
        responder: oneshot::Sender<Result<(), std::io::Error>>,
    },
    Subscribe {
        subscriptions: Arc<[Subscription]>,
        responder: oneshot::Sender<Result<(), SubscribeError>>,
    },
}

struct MqttActor {
    receiver: mpsc::Receiver<MqttActorMessage>,
    stream: TcpStream,
    send_publish: mpsc::Sender<()>,
    // Use ticker in embassy-time
    //TODO put this in a "Connected" variant as this has no effect if the connection is not established
    /// The last time we send a packet to the broker. Used with [crate::KEEP_ALIVE_SECONDS]
    /// to determine when to send the next ping request to keep the connection alive
    ping_interval: Interval,
    is_connected: bool,
}

async fn read_connect_acknowledgement(
    stream: &mut TcpStream,
) -> Result<(), ConnectAcknowledgementError> {
    // Fixed header
    let packet_type_and_flags = stream.read_u8().await?;
    if (packet_type_and_flags >> 4) != 2 {
        return Err(ConnectAcknowledgementError::UnexpectedPacketType(
            packet_type_and_flags >> 4,
        ));
    }

    let (remaining_length, _remaining_length_length) = VariableByteInteger::decode(stream)
        .await
        .map_err(ConnectAcknowledgementError::InvalidRemainingLength)?;

    let mut buffer = Vec::with_capacity(remaining_length);
    // Could warn if we read less or more than remaining_length
    let _bytes_read = stream.read_buf(&mut buffer).await?;

    // CONNACK Variable header
    // Acknowledge flags
    // Bit 7-1 are reserved and must be set to 0, although we don't validate this
    // Bit 0 is the Session Present Flag
    // let flags = stream.read_u8().await?;
    // let is_session_present = (flags & 0b0000_0001) != 0;

    let connect_reason_code = buffer[1];
    let code = ConnectReasonCode::try_from(connect_reason_code)
        .map_err(ConnectAcknowledgementError::InvalidReasonCode)?;

    if let ConnectReasonCode::Error(error_code) = code {
        return Err(ConnectAcknowledgementError::ErrorReasonCode(error_code));
    }

    // Ignore properties for now
    Ok(())
}

async fn send_connect(
    actor: &mut MqttActor,
    client_identifier: Arc<str>,
    username: Arc<str>,
    password: Arc<[u8]>,
) -> Result<(), ConnectError> {
    let packet = packet::create_connect::<KEEP_ALIVE_SECONDS>(
        client_identifier.as_ref(),
        username.as_ref(),
        password.as_ref(),
    );

    actor
        .stream
        .write_all(&packet)
        .await
        .map_err(ConnectError::SendError)?;

    actor
        .stream
        .flush()
        .await
        .map_err(ConnectError::FlushError)?;

    actor.ping_interval.reset();

    // We read the acknowledgement "synchronous" as we can't receive or send other packets before the connection is established
    // Response for connection has to come in next
    read_connect_acknowledgement(&mut actor.stream)
        .await
        .map_err(ConnectError::AcknowledgementError)
}

async fn read_subscribe_acknowledgement(
    stream: &mut TcpStream,
    subscriptions: Arc<[Subscription]>,
) -> Result<(), SubscribeAcknowledgementError> {
    // Fixed header
    let packet_type_and_flags = stream.read_u8().await?;
    if (packet_type_and_flags >> 4) != 9 {
        return Err(SubscribeAcknowledgementError::UnexpectedPacketType(
            packet_type_and_flags >> 4,
        ));
    }

    let (mut remaining_length, _remaining_length_length) = VariableByteInteger::decode(stream)
        .await
        .map_err(SubscribeAcknowledgementError::InvalidRemainingLength)?;

    let mut buffer = Vec::with_capacity(remaining_length);
    // Could warn if we read less or more than remaining_length
    let _bytes_read = stream.read_buf(&mut buffer).await?;

    // Variable header
    let packet_identifier: u16 = ((buffer[0] as u16) << 8) | buffer[1] as u16;
    println!("Packet identifier {packet_identifier}");
    remaining_length -= 2;
    println!("Remaining length {remaining_length}");

    // Properties
    let (properties_length, properties_length_length) =
        VariableByteInteger::decode_2(buffer.as_ref())
            .map_err(SubscribeAcknowledgementError::InvalidPropertiesLength)?;
    println!("Properties length {properties_length}");
    remaining_length -= properties_length_length;
    println!("Remaining length {remaining_length}");
    //TODO read properties

    // Payload
    // Reason code for each subscribed topic in the same order
    let mut topic_index = 0;
    if remaining_length != subscriptions.len() {
        return Err(SubscribeAcknowledgementError::InvalidReasonCodeLength {
            expected: subscriptions.len(),
            actual: remaining_length,
        });
    }

    // Index should be aligned as we checked length before
    while remaining_length > 0 {
        let reason_code = buffer[2 + properties_length_length + topic_index];
        remaining_length -= 1;

        let topic = &subscriptions[topic_index].topic_filter;
        topic_index += 1;
        println!("Reason code for topic {topic}: {reason_code:#04x}");
        println!("Remaining length {remaining_length}");
    }

    Ok(())
}

/// Errors while reading the Subscribe Acknowledgement packet (SUBACK)
#[derive(Debug, thiserror::Error)]
enum ReadPublishError {
    #[error("Error while reading from the stream: {0}")]
    ReadError(#[from] std::io::Error),
    /// The server sent a different packet type than connect acknowledgement (2)
    #[error("Unexpected packet type: {0:#04x}")]
    UnexpectedPacketType(u8),
    #[error("Invalid remaining length: {0}")]
    InvalidRemainingLength(DecodeVariableByteIntegerError),
    #[error("Topic name is not valid UTF-8: {0}")]
    InvalidTopicName(#[from] Utf8Error),
    #[error("Zero length topic name is a protocol error")]
    ZeroLengthTopicName,
    #[error("Invalid properties length: {0}")]
    InvalidPropertiesLength(DecodeVariableByteIntegerError),
    #[error("Invalid reason code length: expected {expected} topic(s), got {actual}")]
    InvalidReasonCodeLength { expected: usize, actual: usize },
}

struct Publish {
    topic_name: Arc<str>,
    payload: Arc<[u8]>,
}

async fn receive_publish(stream: &mut TcpStream) -> Result<Publish, ReadPublishError> {
    let type_and_flags = stream.read_u8().await?;

    if (type_and_flags >> 4) != 3 {
        return Err(ReadPublishError::UnexpectedPacketType(type_and_flags >> 4));
    }

    // Flags matter here but we ignore them for now
    // let is_re_delivery = (type_and_flags & 0b0000_1000) != 0;
    let quality_of_service_level = (type_and_flags & 0b0000_0110) >> 1;
    if quality_of_service_level > 0 {
        // This changes the layout of the packet and adds a packet identifier which we don't expect right now
        unimplemented!("Quality of service level other than 0 is not implemented yet")
    }

    // Ignore retain as it should only matter for packets send to the server

    let (remaining_length, _remaining_length_length) = VariableByteInteger::decode(stream)
        .await
        .map_err(ReadPublishError::InvalidRemainingLength)?;

    let mut buffer = Vec::with_capacity(remaining_length);
    let _bytes_read = stream.read_buf(&mut buffer).await?;
    //TODO check if remaining length matches bytes read

    // Variable header
    let topic_length = ((buffer[0] as u16) << 8) | buffer[1] as u16;
    if topic_length == 0 {
        return Err(ReadPublishError::ZeroLengthTopicName);
    }

    let topic_name = std::str::from_utf8(&buffer[2..2 + topic_length as usize])?;
    //TODO validate topic name does not contain MQTT wildcard characters

    //TODO read packet identifier if QoS > 0
    let (property_length, property_length_length) =
        VariableByteInteger::decode_2(&buffer[2 + topic_length as usize..])
            .map_err(ReadPublishError::InvalidPropertiesLength)?;

    // Ignore properties for now

    // Payload
    let variable_header_length =
        2 + topic_length as usize + property_length_length + property_length;
    let payload_length = remaining_length - variable_header_length;
    //TODO validate there is enough space left in the buffer
    let payload = &buffer[variable_header_length..variable_header_length + payload_length];

    Ok(Publish {
        topic_name: topic_name.into(),
        payload: payload.into(),
    })
}

async fn send_publish(
    actor: &mut MqttActor,
    topic: Arc<str>,
    payload: Arc<[u8]>,
) -> Result<(), std::io::Error> {
    let packet = packet::create_publish(topic.as_ref(), payload.as_ref());

    // Fire and forget with quality of service 0, but we can at least confirm locally if the packet was sent
    actor.stream.write_all(&packet).await?;
    actor.stream.flush().await?;

    actor.ping_interval.reset();

    // We don't wait for the PUBACK packet if there is no Quality of Service (QoS) configured
    // If there was QoS, then we would need to assign a packet identifier and store the channel for as long as we wait for the PUBACK
    Ok(())
}

async fn send_subscribe(
    actor: &mut MqttActor,
    subscriptions: Arc<[Subscription]>,
) -> Result<(), SubscribeError> {
    //TODO manage available identifiers
    let packet = packet::create_subscribe(9, subscriptions.as_ref());

    actor
        .stream
        .write_all(&packet)
        .await
        .map_err(SubscribeError::SendError)?;
    actor
        .stream
        .flush()
        .await
        .map_err(SubscribeError::FlushError)?;

    actor.ping_interval.reset();
    // We wait for the subscribe acknowledgement, but we should probably do that "asynchronously"
    read_subscribe_acknowledgement(&mut actor.stream, subscriptions)
        .await
        .map_err(SubscribeError::AcknowledgementError)
}

async fn process_message(actor: &mut MqttActor, message: MqttActorMessage) {
    //TODO handle case when connection is closed or lost and might want to reconnect
    match message {
        MqttActorMessage::Connect {
            client_identifier,
            username,
            password,
            responder,
        } => {
            let result = send_connect(actor, client_identifier, username, password).await;
            if result.is_ok() {
                actor.is_connected = true;
            }
            // Ignore error if they cancelled waiting for the response
            let _ = responder.send(result);
        }
        MqttActorMessage::Publish {
            topic,
            payload,
            responder,
        } => {
            let result = send_publish(actor, topic, payload).await;

            // Ignore error if they cancelled waiting for the response
            let _ = responder.send(result);
        }
        MqttActorMessage::Subscribe {
            subscriptions,
            responder,
        } => {
            let result = send_subscribe(actor, subscriptions).await;

            // Ignore error if they cancelled waiting for the response
            let _ = responder.send(result);
        }
    }
}

async fn process_publish(actor: &mut MqttActor, publish: Publish) {
    println!("Received publish");
    println!("Topic: {}", publish.topic_name);
    println!("Payload: {:?}", publish.payload);

    match publish.topic_name.as_ref() {
        "testfan/speed/percentage" => {
            let payload = match std::str::from_utf8(&publish.payload) {
                Ok(payload) => payload,
                Err(error) => {
                    eprintln!(
                        "Expected percentage_command_topic payload (speed percentage) to be a valid UTF-8 string with a number: {}",
                        error
                    );
                    return;
                }
            };

            let set_point = payload.parse::<u16>();

            match set_point {
                Ok(set_point) if set_point > 64000 => {
                    eprintln!("Received higher speed set point than configured by this device: {set_point}");
                }
                Ok(set_point) => {
                    println!("Set point: {}", set_point);
                }
                Err(error) => {
                    eprintln!("Expected speed percentage to be a number: {}", error);
                }
            }

            //TODO implement debounce. Gather all update requests for a while and only send the latest if there hasn't been any in specific time window. This should avoid overburdening the modbus channel and avoid weird race conditions
            // Mock sending update to the device
            tokio::time::sleep(Duration::from_secs(1)).await;

            // Send update through publish to percentage_state_topic
            let result = crate::send_publish(
                actor,
                "testfan/speed/percentage_state".into(),
                publish.payload,
            )
            .await;
            //TODO error handling: sink the result in some log or something
            println!("Published result: {:?}", result);
        }
        "testfan/on/set" => {
            let payload = match std::str::from_utf8(&publish.payload) {
                Ok(payload) => payload,
                Err(error) => {
                    eprintln!(
                        "Expected comman_topic payload to be a valid UTF-8 string with either \"ON\" or \"OFF\": {}",
                        error
                    );
                    return;
                }
            };

            match payload {
                "ON" => {
                    println!("Turning on");
                }
                "OFF" => {
                    println!("Turning off");
                }
                unknown => {
                    eprintln!(
                        "Expected either \"ON\" or \"OFF\" but received: {}",
                        unknown
                    );
                }
            }

            //TODO implement debounce. Gather all update requests for a while and only send the latest if there hasn't been any in specific time window. This should avoid overburdening the modbus channel and avoid weird race conditions
            // Mock sending update to the device
            tokio::time::sleep(Duration::from_secs(1)).await;
            // We just echo the state send to us
            let result = send_publish(actor, "testfan/on/state".into(), publish.payload).await;
            //TODO error handling: sink the result in some log or something
            println!("Published result: {:?}", result);
        }
        unexpected => {
            eprintln!(
                "Unexpected topic: {:?} with payload: {:?}",
                unexpected, publish.payload
            );
        }
    }
}

#[derive(Debug)]
enum PingRequestError {
    WriteError(std::io::Error),
    FlushError(std::io::Error),
}

async fn send_ping_request(actor: &mut MqttActor) -> Result<(), PingRequestError> {
    println!("Sending ping request");
    // Probably the simplest of them all
    actor
        .stream
        .write_all(&[12 << 4, 0])
        .await
        .map_err(PingRequestError::WriteError)?;
    actor
        .stream
        .flush()
        .await
        .map_err(PingRequestError::FlushError)?;

    // Wait for the ping response
    let read = actor.stream.read_u16();
    
    // Specification only says "reasonable amount of time". How long is reasonable?
    let result = tokio::time::timeout(Duration::from_secs(30), read).await;
    
    if result.is_err() {
        println!("Ping response timed out");
        actor.is_connected = false;
    }

    //TODO handle other packet type than ping response

    Ok(())
}

async fn run_actor(mut actor: MqttActor) {
    loop {
        tokio::select! {
            Some(message) = actor.receiver.recv() => process_message(&mut actor, message).await,
            Ok(publish) = receive_publish(&mut actor.stream) => process_publish(&mut actor, publish).await,
            //TODO remove unwrap
            _ = actor.ping_interval.tick(), if actor.is_connected => send_ping_request(&mut actor).await.unwrap(),
            else => break,
        }
    }

    println!("Actor done");
}

struct MqttActorHandle {
    sender: mpsc::Sender<MqttActorMessage>,
}

#[derive(Debug)]
enum HandleError<SE: std::error::Error, RE: std::error::Error, AE> {
    SendError(SE),
    ReceiveError(RE),
    ActorError(AE),
}

impl MqttActorHandle {
    fn new(stream: TcpStream, send_publish: mpsc::Sender<()>) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let actor = MqttActor {
            stream,
            receiver,
            send_publish,
            ping_interval: tokio::time::interval(Duration::from_secs(KEEP_ALIVE_SECONDS as u64)),
            is_connected: false,
        };

        tokio::spawn(run_actor(actor));

        Self { sender }
    }

    async fn connect(
        &self,
        client_identifier: Arc<str>,
        username: Arc<str>,
        password: Arc<[u8]>,
    ) -> Result<(), HandleError<impl std::error::Error, impl std::error::Error, ConnectError>> {
        let (sender, receiver) = oneshot::channel();

        type Error = HandleError<
            mpsc::error::SendError<MqttActorMessage>,
            oneshot::error::RecvError,
            ConnectError,
        >;

        self.sender
            .send(MqttActorMessage::Connect {
                client_identifier,
                username,
                password,
                responder: sender,
            })
            .await
            .map_err(Error::SendError)?;

        receiver
            .await
            .map_err(Error::ReceiveError)?
            .map_err(Error::ActorError)
    }

    async fn publish(
        &self,
        topic: Arc<str>,
        payload: Arc<[u8]>,
    ) -> Result<(), HandleError<impl std::error::Error, impl std::error::Error, std::io::Error>>
    {
        type Error = HandleError<
            mpsc::error::SendError<MqttActorMessage>,
            oneshot::error::RecvError,
            std::io::Error,
        >;
        let (sender, receiver) = oneshot::channel();
        self.sender
            .send(MqttActorMessage::Publish {
                topic,
                payload,
                responder: sender,
            })
            .await
            .map_err(Error::SendError)?;

        receiver
            .await
            .map_err(Error::ReceiveError)?
            .map_err(Error::ActorError)
    }

    async fn subscribe(
        &self,
        subscriptions: Arc<[Subscription]>,
    ) -> Result<(), HandleError<impl std::error::Error, impl std::error::Error, SubscribeError>>
    {
        let (sender, receiver) = oneshot::channel();

        type Error = HandleError<
            mpsc::error::SendError<MqttActorMessage>,
            oneshot::error::RecvError,
            SubscribeError,
        >;

        self.sender
            .send(MqttActorMessage::Subscribe {
                subscriptions,
                responder: sender,
            })
            .await
            .map_err(Error::SendError)?;

        receiver
            .await
            .map_err(Error::ReceiveError)?
            .map_err(Error::ActorError)
    }
}

async fn set_up_tcp_connection(
    broker_address: String,
    broker_port: u16,
) -> Result<TcpStream, AppError> {
    let address = (broker_address.as_ref(), broker_port)
        .to_socket_addrs()?
        .next()
        .unwrap();

    let stream = TcpStream::connect(&address).await?;

    Ok(stream)
}

#[tokio::main]
async fn main() -> Result<(), AppError> {
    // Set up .env environment variables
    dotenvy::dotenv()?;
    let broker_address = env::var("MQTT_BROKER_ADDRESS")?;
    let broker_port: u16 = env::var("MQTT_BROKER_PORT")?.parse()?;
    let username = env::var("MQTT_BROKER_USERNAME")?;
    // The password might need to be surrounded by single quotes (e.g. 'password') to be read correctly
    let password = env::var("MQTT_BROKER_PASSWORD")?;

    // Channel to receive publish messages
    let (sender, mut receiver) = mpsc::channel(8);

    // Could directly set it up in the handle new but eh
    let stream = set_up_tcp_connection(broker_address, broker_port).await?;
    let actor = MqttActorHandle::new(stream, sender);

    actor
        .connect(
            "testclient".into(),
            username.into(),
            password.as_bytes().into(),
        )
        .await
        .unwrap();
    // homeassistant/{domain}/{object_id}/config

    // Prefix is "homeassistant", but it can be changed in home assistant configuration
    const DISCOVERY_TOPIC: &str = "homeassistant/fan/testfan/config";

    // Configuration is like the YAML configuration that would be added in Home Assistant but as JSON
    // Command topic: The MQTT topic to publish commands to change the state of the fan
    //TODO set firmware version from Cargo.toml package version
    //TODO think about setting hardware version, support url, and manufacturer
    //TODO create single home assistant device with multiple entities for sensors in fan and the bypass
    //TODO add diagnostic entity like IP address
    //TODO availability topic
    // Using abbreviations to save space of binary and on the wire (haven't measured effect though...)
    // name -> name
    // uniq_id -> unique_id
    // stat_t -> state_topic
    // cmd_t -> command_topic
    // pct_stat_t -> percentage_state_topic
    // pct_cmd_t -> percentage_command_topic
    // spd_rng_max -> speed_range_max
    // Don't need to set speed_range_min because it is 1 by default
    //TODO remove whitespace at compile time through macro, build script or const fn
    const DISCOVERY_PAYLOAD: &[u8] = br#"{
        "name": "Fan",
        "uniq_id": "testfan",
        "stat_t": "testfan/on/state",
        "cmd_t": "testfan/on/set",
        "pct_stat_t": "testfan/speed/percentage_state",
        "pct_cmd_t": "testfan/speed/percentage",
        "spd_rng_max": 64000
    }"#;

    println!("Connected");

    // Subscribing to the state topics before telling home assistant to use them to control the fan

    // Send discover publish packet
    actor
        .publish(DISCOVERY_TOPIC.into(), DISCOVERY_PAYLOAD.into())
        .await
        .unwrap();

    println!("Published discovery");
    let subscriptions = Arc::new([
        // Listen to when the fan should be turned on or off
        // Payload will be "ON" or "OFF"
        Subscription::new(
            "testfan/on/set".into(),
            SubscriptionOptions::new(
                QualityOfService::AtMostOnceDelivery,
                false,
                false,
                RetainHandling::DoNotSend,
            ),
        ),
        // Listen to speed changes from home assistant
        Subscription::new(
            "testfan/speed/percentage".into(),
            SubscriptionOptions::new(
                QualityOfService::AtMostOnceDelivery,
                false,
                false,
                RetainHandling::DoNotSend,
            ),
        ),
    ]);
    actor.subscribe(subscriptions).await.unwrap();

    while let Some(_) = receiver.recv().await {
        println!("Received message")
    }

    Ok(())
}
