//! Module containing all the MQTT things to enable the fan controller to be integrated with Home
//! Assistant

use defmt::Format;
use embedded_io_async::Write;

use connect_acknowledgement::ConnectAcknowledgement;

use crate::mqtt::connect::Connect;
use crate::mqtt::subscribe::Subscribe;
use crate::mqtt::subscribe_acknowledgement::{
    SubscribeAcknowledgement, SubscribeAcknowledgementError,
};
use crate::mqtt::variable_byte_integer::VariableByteIntegerDecodeError;

pub(crate) mod connect;
pub(crate) mod connect_acknowledgement;
pub(crate) mod publish;
pub(crate) mod subscribe;
mod subscribe_acknowledgement;
mod variable_byte_integer;

pub(super) enum Packet {
    ConnectAcknowledgement(ConnectAcknowledgement),
    SubscribeAcknowledgement(SubscribeAcknowledgement),
}

pub(super) enum ConnectErrorReasonCode {
    UnspecifiedError = 0x80,
    MalformedPacket = 0x81,
    ProtocolError = 0x82,
    ImplementationSpecificError = 0x83,
    UnsupportedProtocolVersion = 0x84,
    ClientIdentifierNotValid = 0x85,
    BadUserNameOrPassword = 0x86,
    NotAuthorized = 0x87,
    ServerUnavailable = 0x88,
    ServerBusy = 0x89,
    Banned = 0x8A,
    BadAuthenticationMethod = 0x8C,
    TopicNameInvalid = 0x90,
    PacketTooLarge = 0x95,
    QuotaExceeded = 0x97,
    PayloadFormatInvalid = 0x99,
    RetainNotSupported = 0x9A,
    QosNotSupported = 0x9B,
    UseAnotherServer = 0x9C,
    ServerMoved = 0x9D,
    ConnectionRateExceeded = 0x9F,
}

pub struct UnknownConnectErrorReasonCode(u8);

pub(super) enum QualityOfService {
    /// At most once delivery or 0
    AtMostOnceDelivery = 0x00,
    /// At least once delivery or 1
    AtLeastOnceDelivery = 0x01,
    /// Exactly once delivery or 2
    ExactlyOnceDelivery = 0x02,
}

impl QualityOfService {
    const fn to_byte(&self) -> u8 {
        match self {
            QualityOfService::AtMostOnceDelivery => 0,
            QualityOfService::AtLeastOnceDelivery => 1,
            QualityOfService::ExactlyOnceDelivery => 2,
        }
    }
}

enum ReadConnectAcknowledgementError {
    InvalidReasonCode(UnknownConnectErrorReasonCode),
    InvalidPropertiesLength(VariableByteIntegerDecodeError),
}

pub(super) enum ReadError {
    /// The packet type is not supported. This can happen if there is a packet received that is
    /// only intended for the broker and not the client. Or the packet type is not yet implemented.
    UnsupportedPacketType(u8),
    UnexpectedPacketType(u8),
    MissingBytes(usize),
    InvalidRemainingLength(VariableByteIntegerDecodeError),
    ConnectAcknowledgementError(ReadConnectAcknowledgementError),
    SubscribeAcknowledgementError(SubscribeAcknowledgementError),
}

impl Packet {
    pub(super) fn read(buffer: &[u8]) -> Result<Packet, ReadError> {
        // Fixed header
        let packet_type = buffer[0] >> 4;
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
            // N doesn't matter here
            Subscribe::<2>::TYPE => Err(ReadError::UnsupportedPacketType(packet_type)),
            SubscribeAcknowledgement::TYPE => {
                let subscribe_acknowledgement =
                    SubscribeAcknowledgement::read(variable_header_and_payload)
                        .map_err(ReadError::SubscribeAcknowledgementError)?;

                Ok(Packet::SubscribeAcknowledgement(subscribe_acknowledgement))
            }

            unexpected => Err(ReadError::UnexpectedPacketType(unexpected)),
        }
    }
}
