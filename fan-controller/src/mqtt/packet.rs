use crate::mqtt::connect::Connect;
use crate::mqtt::connect_acknowledgement::ConnectAcknowledgement;
use crate::mqtt::publish::Publish;
use crate::mqtt::subscribe::Subscribe;
use crate::mqtt::subscribe_acknowledgement::{
    SubscribeAcknowledgement, SubscribeAcknowledgementError,
};
use crate::mqtt::variable_byte_integer::VariableByteIntegerDecodeError;
use crate::mqtt::{publish, variable_byte_integer, ReadConnectAcknowledgementError};
use defmt::Format;

#[derive(Debug, Clone, Format)]
pub(crate) enum ReadError {
    /// The packet type is not supported. This can happen if there is a packet received that is
    /// only intended for the broker and not the client. Or the packet type is not yet implemented.
    UnsupportedPacketType(u8),
    UnexpectedPacketType(u8),
    MissingBytes(usize),
    InvalidRemainingLength(VariableByteIntegerDecodeError),
    ConnectAcknowledgementError(ReadConnectAcknowledgementError),
    PublishError(publish::ReadError),
    SubscribeAcknowledgementError(SubscribeAcknowledgementError),
}

/// T is for users of this MQTT implementation to define as the publish packets they expect vary by
/// application. The only requirement is that they can be created from a publish packet which
/// contains the topic name and payload. This is to get around the problem that publish topic names
/// and payloads can have a variable unknown length and are difficult to pass around with lifetimes.
#[derive(Format, Clone)]
pub(crate) enum Packet<T>
where
    T: FromPublish,
{
    ConnectAcknowledgement(ConnectAcknowledgement),
    SubscribeAcknowledgement(SubscribeAcknowledgement),
    Publish(T),
}

impl<T> Packet<T>
where
    T: FromPublish,
{
    pub(crate) fn read(buffer: &[u8]) -> Result<Packet<T>, ReadError> {
        // Fixed header
        let packet_type = buffer[0] >> 4;
        let flags = buffer[0] & 0b0000_1111;
        let mut offset = 2;
        let remaining_length = variable_byte_integer::decode(buffer, &mut offset)
            .map_err(ReadError::InvalidRemainingLength)?;

        if buffer.len() < offset + remaining_length {
            return Err(ReadError::MissingBytes(
                buffer.len() - offset - remaining_length,
            ));
        }
        let variable_header_and_payload = &buffer[offset..offset + remaining_length];

        match packet_type {
            Connect::TYPE => Err(ReadError::UnsupportedPacketType(packet_type)),
            ConnectAcknowledgement::TYPE => {
                let connect_acknowledgement =
                    ConnectAcknowledgement::read(variable_header_and_payload)
                        .map_err(ReadError::ConnectAcknowledgementError)?;

                Ok(Packet::ConnectAcknowledgement(connect_acknowledgement))
            }
            Publish::TYPE => {
                let publish = Publish::read(flags, variable_header_and_payload)
                    .map_err(ReadError::PublishError)?;

                let packet = T::from_publish(publish);
                Ok(Packet::Publish(packet))
            }
            Subscribe::TYPE => Err(ReadError::UnsupportedPacketType(packet_type)),
            SubscribeAcknowledgement::TYPE => {
                let subscribe_acknowledgement =
                    SubscribeAcknowledgement::read(variable_header_and_payload)
                        .map_err(ReadError::SubscribeAcknowledgementError)?;

                Ok(Packet::SubscribeAcknowledgement(subscribe_acknowledgement))
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
    fn from_publish(publish: Publish) -> Self {
        ()
    }
}
