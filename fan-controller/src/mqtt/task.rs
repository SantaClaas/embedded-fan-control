use super::packet::connect_acknowledgement;
use crate::mqtt::packet::connect::{Connect, EncodeError};
use crate::mqtt::packet::connect_acknowledgement::{ConnectAcknowledgement, ConnectReasonCode};
use crate::mqtt::packet::GetPartsError;
use crate::mqtt::{packet, ConnectErrorReasonCode};
use crate::mqtt::{TryDecode, TryEncode};
use core::fmt::Debug;
use defmt::{info, warn, Format};
use embassy_net::tcp;
use embassy_net::tcp::TcpSocket;
use embedded_io_async::Write;

///! Tasks that need to be done to run MQTT
///! - Keep alive

#[derive(Debug, Format)]
pub(crate) enum SendError<T: Debug + Format> {
    Encode(T),
    Write(tcp::Error),
    Flush(tcp::Error),
}

pub(crate) async fn send<T>(
    socket: &mut impl Write<Error = tcp::Error>,
    packet: T,
) -> Result<(), SendError<<T as TryEncode>::Error>>
where
    T: TryEncode<Error: Debug + Format>,
{
    info!("Sending packet");
    let mut offset = 0;
    let mut send_buffer = [0; 1024];
    packet
        .try_encode(&mut send_buffer, &mut offset)
        .map_err(SendError::Encode)?;

    socket
        .write(&send_buffer[..offset])
        .await
        .map_err(SendError::Write)?;
    socket.flush().await.map_err(SendError::Flush)?;
    Ok(())
}

#[derive(Format)]
pub(crate) enum ConnectError {
    Send(SendError<EncodeError>),
    Read(tcp::Error),
    Parts(GetPartsError),
    InvalidResponsePacketType(u8),
    DecodeAcknowledgement(connect_acknowledgement::DecodeError),
    ErrorReasonCode(ConnectErrorReasonCode),
}

pub(crate) async fn connect<'a, 'b>(
    socket: &mut TcpSocket<'a>,
    packet: Connect<'b>,
) -> Result<(), ConnectError> {
    send(socket, packet).await.map_err(ConnectError::Send)?;

    // Wait for connect acknowledgement
    // Discard all messages before the connect acknowledgement
    // The server has to send a connect acknowledgement before sending any other packet
    let mut receive_buffer = [0; 1024];
    let bytes_read = socket
        .read(&mut receive_buffer)
        .await
        .map_err(ConnectError::Read)?;

    let parts = packet::get_parts(&receive_buffer[..bytes_read]).map_err(ConnectError::Parts)?;

    if parts.r#type != ConnectAcknowledgement::TYPE {
        warn!(
            "Expected connect acknowledgement packet, got: {:?}",
            parts.r#type
        );
        return Err(ConnectError::InvalidResponsePacketType(parts.r#type));
    }

    info!("Connect acknowledgement packet received");

    let acknowledgement =
        ConnectAcknowledgement::try_decode(parts.flags, parts.variable_header_and_payload)
            .map_err(ConnectError::DecodeAcknowledgement)?;

    info!("Connect acknowledgement read");
    if let ConnectReasonCode::ErrorCode(error_code) = acknowledgement.connect_reason_code {
        warn!("Connect error: {:?}", error_code);
        return Err(ConnectError::ErrorReasonCode(error_code));
    }

    info!("Connection complete");
    Ok(())
}
