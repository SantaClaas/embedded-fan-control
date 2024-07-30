mod variable_byte_integer;

use bytes::BytesMut;
use std::net::{AddrParseError, SocketAddr, ToSocketAddrs};
use std::ops::{BitOrAssign, Rem};
use std::rc::Rc;
use std::str::FromStr;
use std::sync::Arc;
use std::{cmp, env, ops};
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::{CertificateDer, InvalidDnsNameError, ServerName};
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::TlsConnector;

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
}

async fn set_up_tcp_connection(
    broker_address: String,
) -> Result<impl AsyncReadExt + AsyncWriteExt, AppError> {
    let address = (broker_address.as_ref(), 8883)
        .to_socket_addrs()?
        .next()
        .unwrap();

    let stream = TcpStream::connect(&address).await?;

    Ok(stream)
}

async fn set_up_tls_connection(
    broker_address: String,
) -> Result<impl AsyncReadExt + AsyncWriteExt, AppError> {
    let mut root_certificate_store = RootCertStore::empty();
    root_certificate_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    // root_certificate_store.add(CertificateDer::from(certificate)).unwrap();
    let configuration = ClientConfig::builder()
        .with_root_certificates(root_certificate_store)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(configuration));

    let address = (broker_address.as_ref(), 8883)
        .to_socket_addrs()?
        .next()
        .unwrap();
    // let address = SocketAddr::from_str(MQTT_BROKER_ADDRESS)?;
    let server_name = ServerName::try_from(broker_address)?;

    let stream = TcpStream::connect(&address).await?;

    let stream = connector.connect(server_name, stream).await?;

    Ok(stream)
}

fn create_connect_packet(client_identifier: &str, username: &str, password: &[u8]) -> Vec<u8> {
    //TODO validate identifier length is between 1 and 23 bytes and contains only valid characters
    let identifier_length = client_identifier.len() as u16;

    // Length of str might not be the same as length of bytes afaik
    let username = username.as_bytes();
    //TODO validate user name is less than u16::MAX
    let username_length = username.len() as u16;
    //TODO validate password is less than u16::MAX
    let password_length = password.len() as u16;

    // Remaining length can only be a byte
    //TODO check length is not exceeding u8

    // 2 = length of length fields
    let remaining_length: u8 = (VARIABLE_HEADER.len() as u16
        + 2
        + identifier_length
        + 2
        + username_length
        + 2
        + password_length) as u8;

    // Fixed header
    let fixed_header = [
        // CONNECT package type (1) in the first four bits
        1 << 4,
        // Remaining length
        remaining_length,
    ];

    // Create variable header
    const VARIABLE_HEADER: [u8; 11] = [
        // Protocol name length
        0x00,
        0x04,
        // Protocol name
        b'M',
        b'Q',
        b'T',
        b'T',
        // Protocol version
        5,
        // Connect Flags
        // USER_NAME_FLAG | PASSWORD_FLAG | CLEAN_START
        0b1100_0010,
        // Keep alive = 60 seconds
        0x00,
        60,
        // Property length 0 (no properties). Has to be set to 0 if there are no properties
        0,
    ];

    let mut packet = Vec::with_capacity(fixed_header.len() + remaining_length as usize);
    // packet.append(fixed_header)
    packet.extend_from_slice(&fixed_header);

    // Variable header
    packet.extend_from_slice(&VARIABLE_HEADER);

    // Payload

    // Client identifier
    let length_bytes = identifier_length.to_be_bytes();
    // Client identifier length
    packet.push(length_bytes[0]);
    packet.push(length_bytes[1]);
    // Client identifier
    packet.extend_from_slice(client_identifier.as_bytes());

    // User name
    let length_bytes = username_length.to_be_bytes();
    packet.push(length_bytes[0]);
    packet.push(length_bytes[1]);

    // User name
    packet.extend_from_slice(username);

    // Password is any binary data
    let length_bytes = password_length.to_be_bytes();
    packet.push(length_bytes[0]);
    packet.push(length_bytes[1]);
    // Password
    packet.extend_from_slice(password);

    packet
}

fn create_publish_packet(topic_name: &str, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(0);
    // Shadowing topic name to not confuse str and bytes length
    let topic_name = topic_name.as_bytes();
    let topic_name_length = topic_name.len() as u16;
    let payload_length = payload.len() as u16;

    // Fixed header
    // Packet type PUBLISH (3) and flags
    packet.push(3 << 4);
    //TODO ensure remaining length is less than max u8

    // 2 = name length
    // 1 = length property length bytes
    let remaining_length: u8 = (2 + topic_name_length + 1 + payload_length) as u8;
    packet.push(remaining_length);

    // Variable header
    // Topic name length
    let length_bytes = topic_name_length.to_be_bytes();
    packet.push(length_bytes[0]);
    packet.push(length_bytes[1]);

    // Topic name
    packet.extend_from_slice(topic_name);

    // Property length
    // No properties supported for now so set to 0
    packet.push(0);

    // Payload
    // No need to set length as it can be calculated
    packet.extend_from_slice(payload);

    packet
}

#[test]
fn can_create_publish_packet() {
    const PAYLOAD: [u8; 11] = *b"testpayload";
    // Act
    let publish_packet = create_publish_packet("test", &PAYLOAD);

    // Assert
    // Fixed header
    assert_eq!(publish_packet[..2], [0b0011_0000, 18]);

    // Variable header
    assert_eq!(
        publish_packet[2..9],
        [
            // Topic name length
            0x00, 0x04, // Topic name
            b't', b'e', b's', b't', // Property length
            0x00,
        ]
    );

    // Payload
    assert_eq!(publish_packet[9..], PAYLOAD);
}

#[test]
fn can_create_connect_packet() {
    // Act
    let connect_packet = create_connect_packet("mqttx_0x668d0d", "admin", b"public");
    // Assert
    // Fixed header
    assert_eq!(connect_packet[..2], [0b0001_0000, 42]);
    // Variable header
    assert_eq!(
        connect_packet[2..13],
        [
            // Protocol name length
            0x00,
            0x04,
            // Protocol name
            b'M',
            b'Q',
            b'T',
            b'T',
            // 0x4d, 0x51, 0x54, 0x54,
            // Protocol version
            5,
            // Connect Flags
            // USER_NAME_FLAG | PASSWORD_FLAG | CLEAN_START
            0b1100_0010,
            // 0xc2,
            // Keep alive 60 seconds
            0x00,
            60,
            // Property length 0 (no properties). Has to be set to 0 if there are no properties
            0,
        ]
    );

    // Payload
    assert_eq!(
        connect_packet[13..],
        // Rust formatting is doing some wild stuff here for some reason
        #[rustfmt::skip]
        [
            // Client identifier length
            0x00, 0x0e,
            // Client identifier
            b'm', b'q', b't', b't', b'x', b'_', b'0', b'x', b'6', b'6', b'8', b'd', b'0', b'd',
            // User name length
            0x00, 0x05,
            // User name
            0x61, 0x64, 0x6d, 0x69, 0x6e,
            // Password length
            0x00, 0x06,
            // Password
            b'p', b'u', b'b', b'l', b'i', b'c',
        ]
    );
}

#[tokio::main]
async fn main() -> Result<(), AppError> {
    // Set up .env environment variables
    dotenvy::dotenv()?;
    let broker_address = env::var("MQTT_BROKER_ADDRESS")?;
    let username = env::var("MQTT_BROKER_USERNAME")?;
    let password = env::var("MQTT_BROKER_PASSWORD")?;

    let mut stream = set_up_tls_connection(broker_address).await?;

    // MQTT
    // Good resource: https://www.emqx.com/en/blog/mqtt-5-0-control-packets-01-connect-connack
    // 3.1 CONNECT - Connection request

    let connect_packet =
        create_connect_packet("testclient", username.as_str(), password.as_bytes());

    stream.write_all(&connect_packet).await?;

    stream.flush().await?;

    println!("Flushed");

    // CONNACK Fixed header
    println!("Reading response package type");
    let package_type = stream.read_u8().await?;
    println!("Package type: {package_type} {package_type:#x} {package_type:#010b}");

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
                // let mut buffer = Vec::with_capacity(reason_string_length as usize);
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

    println!("Sending PUBLISH");
    // Publish a message
    let publish_packet = create_publish_packet("test", b"testpayload");
    stream.write_all(&publish_packet).await?;
    // stream
    //     .write_all(&[
    //         // Fixed header
    //         // Packet type PUBLISH (3) and flags
    //         0b0011_0000,
    //         //TODO Remaining length variable byte integer
    //         13,
    //         // Variable header
    //         // Topic name length
    //         0x00,
    //         0x04,
    //         // Topic name
    //         b't',
    //         b'e',
    //         b's',
    //         b't',
    //         // Property length
    //         0x00,
    //         // Payload
    //         b't',
    //         b'e',
    //         b's',
    //         b't',
    //         b'p',
    //         b'l',
    //     ])
    //     .await?;
    // 30 31 00 07 72 65 71 75 65 73 74 10 02 00 00 01 2c 08 00 08
    // 72 65 73 70 6f 6e 73 65 54 68 69 73 20 69 73 20 61 20 51 6f
    // 53 20 30 20 6d 65 73 73 61 67 65
    // stream.write_all(&[
    //     0b0011_0000,
    //     0x31, 0x00, 0x07, 0x72, 0x65, 0x71, 0x75, 0x65, 0x73, 0x74, 0x10, 0x02, 0x00, 0x00, 0x01, 0x2c, 0x08, 0x00, 0x08,
    //     0x72, 0x65, 0x73, 0x70, 0x6f, 0x6e, 0x73, 0x65, 0x54, 0x68, 0x69, 0x73, 0x20, 0x69, 0x73, 0x20, 0x61, 0x20, 0x51, 0x6f,
    //     0x53, 0x20, 0x30, 0x20, 0x6d, 0x65, 0x73, 0x73, 0x61, 0x67, 0x65,
    // ]).await?;

    stream.flush().await?;

    // No PUBACK with Quality of Service 0
    // PUBACK Fixed header
    // println!("Reading response package type");
    // let package_type = stream.read_u8().await?;
    // println!(
    //     "Package type: {package_type} {package_type:#x} {package_type:#010b} {}",
    //     package_type >> 4
    // );
    // let remaining_length = stream.read_u8().await?;
    // println!("Remaining length: {remaining_length}");
    //
    // // Disconnect package type
    // if (package_type & 0b1110_0000) != 0 {
    //     // Read reason for disconnect
    //     let reason_code = stream.read_u8().await?;
    //     println!("Reason code: {reason_code:#x} {reason_code:#010b}");
    // }

    println!("Done");
    Ok(())
}
