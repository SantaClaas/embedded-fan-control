//! Tasks that need to be done to run MQTT
//! - Keep alive

use super::packet::connect_acknowledgement;
use crate::mqtt::packet::GetPartsError;
use crate::mqtt::packet::connect::{Connect, EncodeError};
use crate::mqtt::packet::connect_acknowledgement::{ConnectAcknowledgement, ConnectReasonCode};
use crate::mqtt::{ConnectErrorReasonCode, packet};
use crate::mqtt::{TryDecode, TryEncode};
use core::fmt::Debug;
use defmt::{Format, info, warn};
use embedded_io_async::{Read, Write};

#[derive(Debug, Format)]
pub(crate) enum SendError<T: Debug + Format, E> {
    Encode(T),
    Write(E),
    Flush(E),
}

/// How much room a packet is encoded into before it goes out.
///
/// A publish's payload is no longer part of that — it goes to the socket straight from where it
/// already lives, so this only has to hold the largest *header*, plus the two packets that are
/// encoded whole, `Connect` and `Subscribe`. Those are a few hundred bytes between them.
///
/// It is a buffer inside the MQTT task's future, which lives in the executor's fixed task arena.
/// At 8 kB, sized for the discovery payload back when the payload was copied through it, the
/// firmware hard faulted before it ever reached the broker
pub(crate) const SEND_BUFFER_SIZE: usize = 1024;

pub(crate) async fn send<T, TWrite: Write<Error = TWriteError>, TWriteError>(
    socket: &mut TWrite,
    packet: T,
) -> Result<(), SendError<<T as TryEncode>::Error, TWriteError>>
where
    T: TryEncode<Error: Debug + Format>,
{
    info!("Sending packet");
    let mut offset = 0;
    let mut send_buffer = [0; SEND_BUFFER_SIZE];
    packet
        .try_encode(&mut send_buffer, &mut offset)
        .map_err(SendError::Encode)?;

    // `write` returns how many bytes it took, which is capped by the room left in the socket's own
    // send buffer, and the rest would be dropped without a word. That is survivable for a short
    // state update and not for the discovery payload, which is several times that buffer
    socket
        .write_all(&send_buffer[..offset])
        .await
        .map_err(SendError::Write)?;

    // A publish's payload follows its header rather than being copied into the buffer with it.
    // TCP is a stream, so the broker cannot tell the two writes apart
    let trailing_payload = packet.trailing_payload();
    if !trailing_payload.is_empty() {
        socket
            .write_all(trailing_payload)
            .await
            .map_err(SendError::Write)?;
    }

    socket.flush().await.map_err(SendError::Flush)?;
    Ok(())
}

#[derive(Format)]
pub(crate) enum ConnectError<TWriteError, TReadError> {
    Send(SendError<EncodeError, TWriteError>),
    Read(TReadError),
    Parts(GetPartsError),
    InvalidResponsePacketType(u8),
    DecodeAcknowledgement(connect_acknowledgement::DecodeError),
    ErrorReasonCode(ConnectErrorReasonCode),
}

pub(crate) async fn connect<
    'a,
    TWrite: Write<Error = TWriteError>,
    TWriteError: Debug + Format,
    TRead: Read<Error = TReadError>,
    TReadError,
>(
    writer: &mut TWrite,
    reader: &mut TRead,
    packet: Connect<'a>,
) -> Result<(), ConnectError<TWriteError, TReadError>> {
    send(writer, packet).await.map_err(ConnectError::Send)?;

    // Wait for connect acknowledgement
    // Discard all messages before the connect acknowledgement
    // The server has to send a connect acknowledgement before sending any other packet
    let mut receive_buffer = [0; 1024];
    let bytes_read = reader
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
