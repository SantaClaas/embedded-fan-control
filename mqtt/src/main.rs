mod packet;
mod variable_byte_integer;

use crate::packet::{QualityOfService, RetainHandling, Subscription, SubscriptionOptions};
use bytes::BytesMut;
use packet::{ConnectErrorReasonCode, ConnectReasonCode, PacketType, UnknownConnectReasonCode};
use std::collections::HashMap;
use std::env;
use std::net::{AddrParseError, ToSocketAddrs};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, oneshot};
use tokio_rustls::rustls::pki_types::{InvalidDnsNameError, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;
use variable_byte_integer::{DecodeVariableByteIntegerError, VariableByteInteger};

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

async fn connect(
    stream: &mut TcpStream,
    client_identifier: Arc<str>,
    username: Arc<str>,
    password: Arc<[u8]>,
) -> Result<(), ConnectError> {
    let packet = packet::create_connect(
        client_identifier.as_ref(),
        username.as_ref(),
        password.as_ref(),
    );

    stream
        .write_all(&packet)
        .await
        .map_err(ConnectError::SendError)?;

    stream.flush().await.map_err(ConnectError::FlushError)?;

    // We read the acknowledgemetn "synchronous" as we can't receive or send other packets before the connection is established
    // Response for connection has to come in next

    read_connect_acknowledgement(stream)
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

async fn publish(
    stream: &mut TcpStream,
    topic: Arc<str>,
    payload: Arc<[u8]>,
) -> Result<(), std::io::Error> {
    let packet = packet::create_publish(topic.as_ref(), payload.as_ref());

    // Fire and forget with quality of service 0 but we can at least confirm locally if the packet was sent
    stream.write_all(&packet).await?;
    stream.flush().await?;

    // We don't wait for the PUBACK packet if there is no Quality of Service (QoS) configured
    // If there was QoS it, then we would need to assign a packet identifier and store the channel for as long as we wait for the PUBACK
    Ok(())
}

async fn subscribe(
    stream: &mut TcpStream,
    subscriptions: Arc<[Subscription]>,
) -> Result<(), SubscribeError> {
    //TODO manage available identifiers
    let packet = packet::create_subscribe(9, subscriptions.as_ref());

    stream
        .write_all(&packet)
        .await
        .map_err(SubscribeError::SendError)?;
    stream.flush().await.map_err(SubscribeError::FlushError)?;

    // We wait for the subscribe acknowledgement but we should probably do that "asynchronously"
    read_subscribe_acknowledgement(stream, subscriptions)
        .await
        .map_err(SubscribeError::AcknowledgementError)
}

async fn process_message(message: MqttActorMessage, stream: &mut TcpStream) {
    match message {
        MqttActorMessage::Connect {
            client_identifier,
            username,
            password,
            responder,
        } => {
            let result = connect(stream, client_identifier, username, password).await;

            // Ignore error if they cancelled waiting for the response
            let _ = responder.send(result);
        }
        MqttActorMessage::Publish {
            topic,
            payload,
            responder,
        } => {
            let result = publish(stream, topic, payload).await;

            // Ignore error if they cancelled waiting for the response
            let _ = responder.send(result);
        }
        MqttActorMessage::Subscribe {
            subscriptions,
            responder,
        } => {
            let result = subscribe(stream, subscriptions).await;

            // Ignore error if they cancelled waiting for the response
            let _ = responder.send(result);
        }
    }
}
async fn process_packet(first_byte: u8, actor: &mut MqttActor) {
    println!("Received packet: {}", first_byte >> 4);
    actor.send_publish.send(()).await.unwrap();
}

async fn run_actor(mut actor: MqttActor) {
    loop {
        // let a = actor.stream.read_to_end(&mut buffer).await;

        tokio::select! {
            Some(message) = actor.receiver.recv() => process_message(message, &mut actor.stream).await,
            Ok(first_byte) = actor.stream.read_u8() => process_packet(first_byte, &mut actor).await,
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
    const DISCOVERY_PAYLOAD: &[u8] = br#"{
        "name": "Fan",
        "unique_id": "testfan",
        "state_topic": "testfan/on/state",
        "command_topic": "testfan/on/set",
        "percentage_state_topic": "testfan/speed/percentage_state",
        "percentage_command_topic": "testfan/speed/percentage",

        "speed_range_min": 1,
        "speed_range_max": 64000,
        "qos": 0,
        "optimistic": true
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

async fn old_code() -> Result<(), AppError> {
    // MQTT
    // Good resource: https://www.emqx.com/en/blog/mqtt-5-0-control-packets-01-connect-connack
    // 3.1 CONNECT - Connection request

    let stream: TcpStream = todo!();
    let connect_packet = packet::create_connect("testclient", todo!(), todo!());

    stream.write_all(&connect_packet).await?;

    stream.flush().await?;

    println!("Flushed");

    // CONNACK Fixed header
    println!("Reading response packet type");
    let packet_type_and_flags = stream.read_u8().await?;
    let packet_type = PacketType::try_from(packet_type_and_flags);
    println!("Packet type: {packet_type:?}");

    let remaining_length = stream.read_u8().await?;
    println!("Remaining length: {remaining_length}");

    // CONNACK Variable header
    // Acknowledge flags
    // Bit 7-1 are reserved and must be set to 0
    // Bit 0 is the Session Present Flag
    let acknowledge_flags = stream.read_u8().await?;
    println!("Acknowledge flags: {acknowledge_flags:#x} {acknowledge_flags:#010b}");
    let is_session_present = (acknowledge_flags & 0b0000_0001) != 0;
    println!("Is session present: {is_session_present}");

    // Connect reason code
    let connect_reason_code = stream.read_u8().await?;
    println!("Connect reason code: {connect_reason_code:#x} {connect_reason_code:#010b}");

    // CONNACK Properties
    //TODO decode variable byte integer
    let property_length = stream.read_u8().await?;
    println!("Property length: {property_length} {property_length:#x} {property_length:#010b}");
    // Bail if variable byte integer is greater than one byte as we don't have decoding yet
    // (first bit is set to indicate that the next byte is part of the variable byte integer)
    if (property_length & 0b1000_0000) != 0 {
        unimplemented!("Variable byte integer support is not implemented yet");
    }

    println!("Properties:");

    let mut bytes_remaining = property_length as usize;
    while bytes_remaining > 0 {
        let identifier = stream.read_u8().await?;
        bytes_remaining -= 1;
        match identifier {
            0x1f => {
                println!("Property Reason String (0x1f)");
                let reason_string_length = stream.read_u16().await?;
                bytes_remaining -= 2;
                println!("Reason string length: {reason_string_length}");
                let mut buffer = BytesMut::with_capacity(reason_string_length as usize);
                let bytes_read = stream.read_buf(&mut buffer).await?;
                bytes_remaining -= bytes_read;
                println!("Reason string: {:?}", String::from_utf8_lossy(&buffer));
            }
            0x21 => {
                let receive_maximum = stream.read_u16().await?;
                bytes_remaining -= 2;
                println!("- Receive Maximum (0x21): {receive_maximum}");
            }
            0x22 => {
                let topic_alias_maximum = stream.read_u16().await?;
                bytes_remaining -= 2;
                println!("- Topic Alias Maximum (0x22): {topic_alias_maximum}");
            }
            other => {
                println!("Unknown property identifier: {other:#04x}");
            }
        }
    }

    println!("Sending discovery publish");

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
    const DISCOVERY_PAYLOAD: &str = r#"{
        "name": "Fan",
        "unique_id": "testfan",
        "state_topic": "testfan/on/state",
        "command_topic": "testfan/on/set",
        "percentage_state_topic": "testfan/speed/percentage_state",
        "percentage_command_topic": "testfan/speed/percentage",

        "preset_mode_state_topic": "testfan/preset_mode_state",
        "preset_mode_command_topic": "testfan/preset_mode",
        "preset_modes": ["off", "low", "medium", "high"],

        "speed_range_min": 1,
        "speed_range_max": 64000,
        "qos": 0,
        "optimistic": true
    }"#;

    // Publish a message
    let publish_packet = packet::create_publish(DISCOVERY_TOPIC, DISCOVERY_PAYLOAD.as_bytes());
    stream.write_all(&publish_packet).await?;

    stream.flush().await?;

    // No PUBACK with Quality of Service 0

    // Topics that were observed to be triggered when the state is change in Home Assistant UI
    let topics = &[
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
    ];

    let subscribe_packet = packet::create_subscribe(9, topics);
    println!("Sending subscribe packet");
    stream.write_all(&subscribe_packet).await?;
    stream.flush().await?;
    println!("Sent subscribe packet");

    println!("Reading response package type");

    let packet_type_and_flags = stream.read_u8().await?;
    let packet_type = PacketType::try_from(packet_type_and_flags);

    println!("Packet type: {packet_type:?}");

    let mut remaining_length = stream.read_u8().await?;
    println!("Remaining length: {remaining_length}");

    let packet_identifier = stream.read_u16().await?;
    println!("Packet identifier: {packet_identifier}");
    remaining_length -= 2;

    let property_length = stream.read_u8().await?;
    println!("Property length: {property_length}");
    remaining_length -= 1;

    if property_length > 0 {
        unimplemented!("Not implmemented property length > 0 handling");
    }

    // Payload
    let reason_code = stream.read_u8().await?;
    println!("Reason code: {reason_code}");
    remaining_length -= 1;

    println!("Remaining length: {}", remaining_length);
    //TODO this is the other reason codes for each topic
    let whut = stream.read_u8().await?;
    println!("what: {}", whut);
    // Packet type of next packet
    let packet_type_and_flags = stream.read_u8().await?;
    let packet_type = PacketType::try_from(packet_type_and_flags);
    println!("Packet type {packet_type:?}");

    println!("Done");
}
