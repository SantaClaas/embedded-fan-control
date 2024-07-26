use std::net::{AddrParseError, SocketAddr, ToSocketAddrs};
use std::str::FromStr;
use std::sync::Arc;
use tokio::net::TcpStream;
use tokio_rustls::rustls::{ClientConfig, RootCertStore};
use tokio_rustls::rustls::pki_types::{InvalidDnsNameError, ServerName};
use tokio_rustls::TlsConnector;

const MQTT_BROKER_ADDRESS: &str = "91c57a00c93443dc90b40bfcaefa7aa3.s1.eu.hivemq.cloud";

#[derive(Debug, thiserror::Error)]
enum AppError {
    #[error("Failed to create address")]
    CreateAddressError(#[from] AddrParseError),
    #[error("Could not create server name")]
    CreateServerNameError(#[from] InvalidDnsNameError),
    #[error("Failed to connect to broker")]
    ConnectError(#[from] std::io::Error),
}

/// Encodes an integer as variable byte integer according to the MQTT specification
fn encode_variable_byte_integer(mut x: u32) -> Vec<u8> {
    // Each byte can hold 7 bits, so how many 7 bit "bytes" do we need for 32 bits?
    // 32 / 7
    let length = 32usize.div_ceil(7);
    let mut output = Vec::with_capacity(length);
    loop {
        // 128 = 0b1000_0000

        // x = 128 = 0b1000_0000
        // encoded_byte = 128 % 0b1000_0000 = 0
        let mut encoded_byte = (x % 0b1000_0000);
        println!("Encoded byte (remainder): {encoded_byte:#010b} {encoded_byte} {x} {}", 0x00_00_80_01_u32);

        // x = 128 / 128 = 1
        x = x / 0b1000_0000;
        // If there is more data to encode, set the top bit of this byte
        if x > 0 {
            encoded_byte |= 0b1000_0000;
        }

        output.push(encoded_byte as u8);

        if x > 0 {
            continue;
        } else {
            break;
        }
    }

    output
}

#[test]
fn can_encode_variable_byte_integer() {
    // Random examples from https://www.emqx.com/en/blog/mqtt-5-0-control-packets-01-connect-connack
    assert_eq!(encode_variable_byte_integer(47), [0x2f]);
    assert_eq!(encode_variable_byte_integer(19), [0x13]);

    // Range one byte
    assert_eq!(encode_variable_byte_integer(0), [0b0000_0000]);
    assert_eq!(encode_variable_byte_integer(1), [0b0000_0001]);
    assert_eq!(encode_variable_byte_integer(127), [0b0111_1111]);
    // Range two bytes
    assert_eq!(encode_variable_byte_integer(128), [0b1000_0000, 0b0000_0001]);
    assert_eq!(encode_variable_byte_integer(16_383), [0b1111_1111, 0b0111_1111]);

    // Range three bytes
    assert_eq!(encode_variable_byte_integer(16_384), [0b1000_0000, 0b1000_0000, 0b0000_0001]);
    assert_eq!(encode_variable_byte_integer(2_097_151), [0b1111_1111, 0b1111_1111, 0b0111_1111]);
    // Range four bytes
    assert_eq!(encode_variable_byte_integer(2_097_152), [0b1000_0000, 0b1000_0000, 0b1000_0000, 0b0000_0001]);
    assert_eq!(encode_variable_byte_integer(268_435_455), [0b1111_1111, 0b1111_1111, 0b1111_1111, 0b0111_1111]);
}

#[derive(Debug)]
enum DecodeVariableByteIntegerError {
    MalformedVariableByteIntegerError,
    UnexpectedEndOfInput,
}


fn decode_variable_byte_integer(bytes: impl IntoIterator<Item=u8> + std::fmt::Debug) -> Result<u32, DecodeVariableByteIntegerError> {
    dbg!(&bytes);
    let mut multiplier = 1;
    let mut value: u32 = 0;

    let mut bytes = bytes.into_iter();
    loop {
        let encoded_byte = bytes.next().ok_or(DecodeVariableByteIntegerError::UnexpectedEndOfInput)?;
        dbg!(encoded_byte);

        value += (encoded_byte & 127) as u32 * multiplier;

        if multiplier > 128 * 128 * 128 {
            return Err(DecodeVariableByteIntegerError::MalformedVariableByteIntegerError);
        }

        multiplier *= 128;

        // The last byte has the most significant bit set to 0 indicating that there are no more bytes to follow
        if (encoded_byte & 128) != 0 {
            continue;
        } else {
            break;
        }
    }

    Ok(value)
}

#[test]
fn can_decode_variable_byte_integer() {
    // Random examples from https://www.emqx.com/en/blog/mqtt-5-0-control-packets-01-connect-connack
    assert_eq!(decode_variable_byte_integer([0x2f]).unwrap(), 47);
    assert_eq!(decode_variable_byte_integer([0x13]).unwrap(), 19);

    // Range one byte
    assert_eq!(decode_variable_byte_integer([0b0000_0000]).unwrap(), 0);
    assert_eq!(decode_variable_byte_integer([0b0000_0001]).unwrap(), 1);
    assert_eq!(decode_variable_byte_integer([0b0111_1111]).unwrap(), 127);

    // Range two bytes
    assert_eq!(decode_variable_byte_integer([0b1000_0000, 0b0000_0001]).unwrap(), 128);
    assert_eq!(decode_variable_byte_integer([0b1111_1111, 0b0111_1111]).unwrap(), 16_383);

    // Range three bytes
    assert_eq!(decode_variable_byte_integer([0b1000_0000, 0b1000_0000, 0b0000_0001]).unwrap(), 16_384);
    assert_eq!(decode_variable_byte_integer([0b1111_1111, 0b1111_1111, 0b0111_1111]).unwrap(), 2_097_151);

    // Range four bytes
    assert_eq!(decode_variable_byte_integer([0b1000_0000, 0b1000_0000, 0b1000_0000, 0b0000_0001]).unwrap(), 2_097_152);
    assert_eq!(decode_variable_byte_integer([0b1111_1111, 0b1111_1111, 0b1111_1111, 0b0111_1111]).unwrap(), 268_435_455);
}

#[tokio::main]
async fn main() -> Result<(), AppError> {
    let mut root_certificate_store = RootCertStore::empty();
    root_certificate_store.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let configuration = ClientConfig::builder()
        .with_root_certificates(root_certificate_store)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(configuration));
    let address = (MQTT_BROKER_ADDRESS, 8883).to_socket_addrs()?.next().unwrap();
    // let address = SocketAddr::from_str(MQTT_BROKER_ADDRESS)?;
    let server_name = ServerName::try_from("91c57a00c93443dc90b40bfcaefa7aa3.s1.eu.hivemq.cloud")?;

    let stream = TcpStream::connect(&address).await?;
    let mut stream = connector.connect(server_name, stream).await?;

    // MQTT
    // Connection request
    let fixed_header = 0b001_0000_u8;


    println!("Hello, world! {fixed_header}");
    Ok(())
}
