use crate::mqtt::connect::{Connect, EncodeError};
use crate::mqtt::connect_acknowledgement::{ConnectAcknowledgement, ConnectReasonCode};
use crate::mqtt::packet::GetPartsError;
use crate::mqtt::{packet, ConnectErrorReasonCode, ReadConnectAcknowledgementError};
use defmt::{warn, Format};
use embassy_net::tcp;
use embassy_net::tcp::TcpSocket;
use embedded_io_async::Write;

///! Tasks that need to be done to run MQTT
///! - Keep alive

pub(crate) trait Encode {
    type Error;
    fn encode(&self, buffer: &mut [u8], offset: &mut usize) -> Result<(), Self::Error>;
}

#[derive(Debug, Format)]
pub(crate) enum SendError<T> {
    EncodeError(T),
    SendError(tcp::Error),
    FlushError(tcp::Error),
}

pub(crate) async fn send<'a, T>(
    socket: &mut impl Write<Error = tcp::Error>,
    packet: T,
) -> Result<(), SendError<<T as Encode>::Error>>
where
    T: Encode,
{
    let mut offset = 0;
    let mut send_buffer = [0; 256];
    packet
        .encode(&mut send_buffer, &mut offset)
        .map_err(SendError::EncodeError)?;
    socket
        .write(&send_buffer[..offset])
        .await
        .map_err(SendError::SendError)?;
    socket.flush().await.map_err(SendError::FlushError)?;
    Ok(())
}

#[derive(Format)]
pub(crate) enum ConnectError {
    SendError(SendError<EncodeError>),
    ReadError(tcp::Error),
    PartsError(GetPartsError),
    InvalidResponsePacketType(u8),
    DecodeAcknowledgementError(ReadConnectAcknowledgementError),
    ErrorReasonCode(ConnectErrorReasonCode),
}

pub(crate) async fn connect<'a, 'b>(
    socket: &mut TcpSocket<'a>,
    packet: Connect<'b>,
) -> Result<(), ConnectError> {
    send(socket, packet)
        .await
        .map_err(ConnectError::SendError)?;

    // Wait for connect acknowledgement
    // Discard all messages before the connect acknowledgement
    // The server has to send a connect acknowledgement before sending any other packet
    let mut receive_buffer = [0; 1024];
    let bytes_read = socket
        .read(&mut receive_buffer)
        .await
        .map_err(ConnectError::ReadError)?;

    let parts =
        packet::get_parts(&receive_buffer[..bytes_read]).map_err(ConnectError::PartsError)?;
    if parts.r#type != ConnectAcknowledgement::TYPE {
        warn!(
            "Expected connect acknowledgement packet, got: {:?}",
            parts.r#type
        );
        return Err(ConnectError::InvalidResponsePacketType(parts.r#type));
    }

    let acknowledgement = ConnectAcknowledgement::read(parts.variable_header_and_payload)
        .map_err(ConnectError::DecodeAcknowledgementError)?;

    if let ConnectReasonCode::ErrorCode(error_code) = acknowledgement.connect_reason_code {
        warn!("Connect error: {:?}", error_code);
        return Err(ConnectError::ErrorReasonCode(error_code));
    }

    Ok(())
}
