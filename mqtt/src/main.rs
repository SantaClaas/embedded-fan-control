mod variable_byte_integer;
mod packet;

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

async fn set_up_tcp_connection(
    broker_address: String,
    broker_port: u16,
) -> Result<impl AsyncReadExt + AsyncWriteExt, AppError> {
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

    let mut stream = set_up_tcp_connection(broker_address, broker_port).await?;

    // MQTT
    // Good resource: https://www.emqx.com/en/blog/mqtt-5-0-control-packets-01-connect-connack
    // 3.1 CONNECT - Connection request

    let connect_packet =
        packet::create_connect("testclient", username.as_str(), password.as_bytes());

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
    let publish_packet = packet::create_publish("test", b"testpayload");
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
