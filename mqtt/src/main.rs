mod packet;
mod variable_byte_integer;

use crate::packet::{QualityOfService, RetainHandling, Subscription, SubscriptionOptions};
use bytes::BytesMut;
use packet::{ConnectErrorReasonCode, ConnectReasonCode, PacketType, UnkownConnectReasonCode};
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

enum MqttActorMessage {
    Connect {
        client_identifier: Arc<str>,
        username: Arc<str>,
        password: Arc<[u8]>,
        responder: oneshot::Sender<Result<(), ConnectError>>,
    },
}

struct MqttActor {
    receiver: mpsc::Receiver<MqttActorMessage>,
    stream: TcpStream,
}

/// Errors while reading the Connect Acknowledgement packet (CONNACK)
#[derive(Debug)]
enum ConnectAcknowledgementError {
    /// The acknowledgement packet is smaller than the minimum length for an connect acknowledge packet
    /// This might be due to having received an invalid packet type
    InvalidLength,
    /// The server sent a different packet type than connect acknowledgement (2)
    UnexpectedPacketType(u8),
    InvalidRemainingLength(DecodeVariableByteIntegerError),
    /// The remaining length indicates bytes are missing
    MissingData,
    InvalidReasonCode(UnkownConnectReasonCode),
    ErrorReasonCode(ConnectErrorReasonCode),
}

#[derive(Debug)]
enum ConnectError {
    EmptyResponse,
    SendError(std::io::Error),
    ReadAcknowledgementError(std::io::Error),
    AcknowledgementError(ConnectAcknowledgementError),
}

fn read_connect_acknowledgement(
    buffer: Vec<u8>,
    bytes_read: usize,
) -> Result<(), ConnectAcknowledgementError> {
    // Fixed header
    let packet_type_and_flags = buffer[0];
    if (packet_type_and_flags >> 4) != 2 {
        return Err(ConnectAcknowledgementError::UnexpectedPacketType(
            packet_type_and_flags >> 4,
        ));
    }

    //TODO determine minimum length for smallest CONNACK packet
    if bytes_read < 2 {
        return Err(ConnectAcknowledgementError::InvalidLength);
    }

    let (remaining_length, remaining_length_length) = VariableByteInteger::decode(&buffer)
        .map_err(ConnectAcknowledgementError::InvalidRemainingLength)?;

    if buffer.len() != 2 + remaining_length {
        return Err(ConnectAcknowledgementError::MissingData);
    }

    // CONNACK Variable header
    // Acknowledge flags
    // Bit 7-1 are reserved and must be set to 0, although we don't validate this
    // Bit 0 is the Session Present Flag
    // let flags = buffer[remaining_length_length + 1];
    // let is_session_present = (acknowledge_flags & 0b0000_0001) != 0;

    let connect_reason_code = buffer[remaining_length + 2];
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

    // We read the acknowledgemetn "synchronous" as we can't receive or send other packets before the connection is established
    // Response for connection has to come in next
    let mut buffer = Vec::default();
    let bytes_read = stream
        .read_to_end(&mut buffer)
        .await
        .map_err(ConnectError::ReadAcknowledgementError)?;

    dbg!("here");
    if bytes_read == 0 {
        return Err(ConnectError::EmptyResponse);
    }

    read_connect_acknowledgement(buffer, bytes_read).map_err(ConnectError::AcknowledgementError)
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
    }
}
fn process_mqtt_message(buffer: &Vec<u8>) {
    todo!("Process MQTT message")
}

async fn run_actor(mut actor: MqttActor) {
    let mut buffer = Vec::new();
    loop {
        // let a = actor.stream.read_to_end(&mut buffer).await;

        tokio::select! {
            Some(message) = actor.receiver.recv() => process_message(message, &mut actor.stream).await,
            Ok(_bytes_read) = actor.stream.read_to_end(&mut buffer) => process_mqtt_message(&buffer),
            else => break,
        }

        buffer.clear();
    }
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
    fn new(stream: TcpStream) -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let actor = MqttActor { stream, receiver };

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

    // Could directly set it up in the handle new but eh
    let stream = set_up_tcp_connection(broker_address, broker_port).await?;
    let actor = MqttActorHandle::new(stream);

    actor
        .connect(
            "testclient".into(),
            username.into(),
            password.as_bytes().into(),
        )
        .await
        .unwrap();

    println!("Connected");
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
            "testfan/on/set",
            SubscriptionOptions::new(
                QualityOfService::AtMostOnceDelivery,
                false,
                false,
                RetainHandling::DoNotSend,
            ),
        ),
        // Listen to speed changes from home assistant
        Subscription::new(
            "testfan/speed/percentage",
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
    //TODO figure out what this trailing byte in the subscribe acknolwedgement is
    let whut = stream.read_u8().await?;
    println!("what: {}", whut);
    // Packet type of next packet
    let packet_type_and_flags = stream.read_u8().await?;
    let packet_type = PacketType::try_from(packet_type_and_flags);
    println!("Packet type {packet_type:?}");

    println!("Done");
}
