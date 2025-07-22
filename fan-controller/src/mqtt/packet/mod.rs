use crate::mqtt::packet::connect::Connect;
use crate::mqtt::packet::connect_acknowledgement::ConnectAcknowledgement;
use crate::mqtt::packet::publish::Publish;
use crate::mqtt::packet::subscribe::Subscribe;
use crate::mqtt::packet::subscribe_acknowledgement::{
    SubscribeAcknowledgement, SubscribeAcknowledgementError,
};
use crate::mqtt::variable_byte_integer;
use crate::mqtt::DecodeError;
use defmt::{info, Format};

use super::TryDecode;

pub(crate) mod connect;
pub(crate) mod connect_acknowledgement;
pub(crate) mod disconnect;
pub(crate) mod ping_request;
pub(crate) mod ping_response;
pub(crate) mod publish;
pub(crate) mod subscribe;
pub(crate) mod subscribe_acknowledgement;

#[derive(Clone, Format, Debug)]
pub(crate) enum GetPartsError {
    EmptyBuffer,
    InvalidRemainingLength(variable_byte_integer::DecodeError),
    MissingBytes(usize),
}

#[derive(Debug, Clone, Format)]
pub(crate) enum ReadError {
    /// The packet type is not supported. This can happen if there is a packet received that is
    /// only intended for the broker and not the client. Or the packet type is not yet implemented.
    UnsupportedPacketType(u8),
    UnexpectedPacketType(u8),
    PartsError(GetPartsError),
    ConnectAcknowledgementError(connect_acknowledgement::DecodeError),
    PublishError(publish::ReadError),
    SubscribeAcknowledgementError(SubscribeAcknowledgementError),
}

pub(crate) struct PacketParts<'a> {
    pub(crate) r#type: u8,
    pub(crate) flags: u8,
    pub(crate) variable_header_and_payload: &'a [u8],
}

/// T is for users of this MQTT implementation to define as the publish packets they expect vary by
/// application. The only requirement is that they can be created from a publish packet which
/// contains the topic name and payload. This is to get around the problem that publish topic names
/// and payloads can have a variable unknown length and are difficult to pass around with lifetimes.
#[derive(Format)]
pub(crate) enum Packet<T, S>
where
    T: FromPublish,
    S: FromSubscribeAcknowledgement,
{
    ConnectAcknowledgement(ConnectAcknowledgement),
    SubscribeAcknowledgement(S),
    Publish(T),
}

pub(crate) fn get_parts(buffer: &[u8]) -> Result<PacketParts, GetPartsError> {
    if buffer.is_empty() {
        return Err(GetPartsError::EmptyBuffer);
    }

    let packet_type = buffer[0] >> 4;
    let flags = buffer[0] & 0b0000_1111;
    let mut offset = 1;
    let remaining_length = variable_byte_integer::decode(buffer, &mut offset)
        .map_err(GetPartsError::InvalidRemainingLength)?;

    if buffer.len() < offset + remaining_length {
        let missing_bytes = buffer.len() - offset - remaining_length;
        return Err(GetPartsError::MissingBytes(missing_bytes));
    }

    let variable_header_and_payload = &buffer[offset..offset + remaining_length];

    Ok(PacketParts {
        r#type: packet_type,
        flags,
        variable_header_and_payload,
    })
}

impl<T, S> Packet<T, S>
where
    T: FromPublish,
    S: FromSubscribeAcknowledgement,
{
    pub(crate) fn read(buffer: &[u8]) -> Result<Packet<T, S>, ReadError> {
        // Fixed header

        let parts = get_parts(buffer).map_err(ReadError::PartsError)?;

        match parts.r#type {
            Connect::TYPE => Err(ReadError::UnsupportedPacketType(parts.r#type)),
            ConnectAcknowledgement::TYPE => {
                let connect_acknowledgement = ConnectAcknowledgement::try_decode(
                    parts.flags,
                    parts.variable_header_and_payload,
                )
                .map_err(ReadError::ConnectAcknowledgementError)?;

                Ok(Packet::ConnectAcknowledgement(connect_acknowledgement))
            }
            Publish::TYPE => {
                let publish = Publish::try_decode(parts.flags, parts.variable_header_and_payload)
                    .map_err(ReadError::PublishError)?;

                let packet = T::from_publish(publish);
                Ok(Packet::Publish(packet))
            }
            Subscribe::TYPE => Err(ReadError::UnsupportedPacketType(parts.r#type)),
            SubscribeAcknowledgement::TYPE => {
                let subscribe_acknowledgement =
                    SubscribeAcknowledgement::read(parts.variable_header_and_payload)
                        .map_err(ReadError::SubscribeAcknowledgementError)?;

                Ok(Packet::SubscribeAcknowledgement(
                    S::from_subscribe_acknowledgement(subscribe_acknowledgement),
                ))
            }

            unexpected => Err(ReadError::UnexpectedPacketType(unexpected)),
        }
    }

    pub(crate) const fn get_type(&self) -> u8 {
        match self {
            Packet::ConnectAcknowledgement(_) => ConnectAcknowledgement::TYPE,
            Packet::SubscribeAcknowledgement(_) => SubscribeAcknowledgement::TYPE,
            Packet::Publish(_) => Publish::TYPE,
        }
    }
}

pub(crate) trait FromPublish {
    fn from_publish(publish: Publish) -> Self;
}

/// Temporary just for testing. TODO remove
impl FromPublish for () {
    fn from_publish(publish: Publish) -> Self {}
}

pub(crate) trait FromSubscribeAcknowledgement {
    fn from_subscribe_acknowledgement(subscribe_acknowledgement: SubscribeAcknowledgement) -> Self;
}
