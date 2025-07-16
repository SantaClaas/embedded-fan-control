use crate::mqtt::{variable_byte_integer, ConnectErrorReasonCode, UnknownConnectErrorReasonCode};
use crate::TryDecode;
use ::mqtt::QualityOfService;
use defmt::{info, Format};

// pub(crate) struct Properties {
//     /// The session expiry interval in seconds
//     session_expiry_interval: Option<u32>,
//     receive_maximum: Option<u16>,
//     maximum_qualitiy_of_service: Option<QualityOfService>,
//     is_retain_available: Option<bool>,
//     maximum_packet_size: Option<u32>,
//     //TODO
//     // assigned_client_identifier: Option<&'a str>,
//     topic_alias_maximum: Option<u16>,
//     //TODO
//     reason_string: Option<&'a str>,
// }

#[derive(Format, Clone)]
pub(crate) struct ConnectAcknowledgement {
    pub(crate) is_session_present: bool,
    pub(crate) connect_reason_code: ConnectReasonCode,
}

impl ConnectAcknowledgement {
    pub(crate) const TYPE: u8 = 2;
}

#[derive(Debug, Clone, Format)]
pub(crate) enum DecodeError {
    InvalidReasonCode(UnknownConnectErrorReasonCode),
    InvalidPropertiesLength(variable_byte_integer::DecodeError),
    InvalidPropertyStringLength {
        property_identifier: u8,
        error: variable_byte_integer::DecodeError,
    },
    UnknownPropertyIdentifier(u8),
}
impl TryDecode<'_> for ConnectAcknowledgement {
    type Error = DecodeError;

    /// Reads the variable header and payload of a connect acknowledgement packet
    fn try_decode(flags: u8, buffer: &[u8]) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        // Good resource: https://www.emqx.com/en/blog/mqtt-5-0-control-packets-01-connect-connack
        // Fixed header is already read.
        // Read variable header
        let is_session_present = (buffer[0] & 0b0000_0001) != 0;
        let connect_reason_code =
            ConnectReasonCode::try_from(buffer[1]).map_err(DecodeError::InvalidReasonCode)?;

        // Properties
        let mut offset = 2;

        let property_length = variable_byte_integer::decode(buffer, &mut offset)
            .map_err(DecodeError::InvalidPropertiesLength)?;

        info!("CONNACK Property LENGTH {}", property_length);
        let end = offset + property_length;
        while offset < end {
            let property_identifier = buffer[offset];
            offset += 1;
            info!("Property identifier {:#04x}", property_identifier);
            let bytes_read =
                match property_identifier {
                    0x11 => {
                        info!("Skipping Session Expiry Interval");
                        4
                    }
                    0x21 => {
                        info!("Skipping Receive Maximum");
                        2
                    }
                    0x24 => {
                        info!("Skipping Maximum Quality of Service");
                        1
                    }
                    0x25 => {
                        info!("Skipping Retain Available");
                        1
                    }
                    0x27 => {
                        info!("Skipping Maximum Packet Size");
                        4
                    }
                    0x12 => {
                        info!("Skipping Assigned Client Identifier");
                        // Identifier length

                        variable_byte_integer::decode(buffer, &mut offset).map_err(|error| {
                            DecodeError::InvalidPropertyStringLength {
                                property_identifier: 0x12,
                                error,
                            }
                        })?
                    }
                    0x22 => {
                        info!("Skipping Topic Alias Maximum");
                        2
                    }
                    0x1F => {
                        info!("Skipping Reason String");
                        // Reason string length

                        variable_byte_integer::decode(buffer, &mut offset).map_err(|error| {
                            DecodeError::InvalidPropertyStringLength {
                                property_identifier: 0x1F,
                                error,
                            }
                        })?
                    }
                    0x26 => {
                        info!("Skipping User Property");
                        // What is the length of a UTF-8 string pair?
                        let mut length_1 = variable_byte_integer::decode(buffer, &mut offset)
                            .map_err(|error| DecodeError::InvalidPropertyStringLength {
                                property_identifier: 0x26,
                                error,
                            })?;

                        // This is a bit weird as this is the only match branch that advances the offset
                        offset += length_1;

                        let length_2 = variable_byte_integer::decode(buffer, &mut offset).map_err(
                            |error| DecodeError::InvalidPropertyStringLength {
                                property_identifier: 0x26,
                                error,
                            },
                        )?;

                        offset += length_2;
                        continue;
                    }
                    0x28 => {
                        info!("Skipping Wildcard Subscription Available");
                        1
                    }
                    0x29 => {
                        info!("Skipping Subscription Identifier Available");
                        1
                    }
                    0x2A => {
                        info!("Skipping Shared Subscription Available");
                        1
                    }
                    0x13 => {
                        info!("Skipping Server Keep Alive");
                        2
                    }
                    0x1A => {
                        info!("Skipping Response Information");

                        variable_byte_integer::decode(buffer, &mut offset).map_err(|error| {
                            DecodeError::InvalidPropertyStringLength {
                                property_identifier: 0x1A,
                                error,
                            }
                        })?
                    }
                    0x1C => {
                        info!("Skipping Server Reference");

                        variable_byte_integer::decode(buffer, &mut offset).map_err(|error| {
                            DecodeError::InvalidPropertyStringLength {
                                property_identifier: 0x1C,
                                error,
                            }
                        })?
                    }
                    0x15 => {
                        info!("Skipping Authentication Method");

                        variable_byte_integer::decode(buffer, &mut offset).map_err(|error| {
                            DecodeError::InvalidPropertyStringLength {
                                property_identifier: 0x15,
                                error,
                            }
                        })?
                    }
                    0x16 => {
                        info!("Skipping Authentication Data");

                        variable_byte_integer::decode(buffer, &mut offset).map_err(|error| {
                            DecodeError::InvalidPropertyStringLength {
                                property_identifier: 0x16,
                                error,
                            }
                        })?
                    }
                    unknown => {
                        return Err(DecodeError::UnknownPropertyIdentifier(unknown));
                    }
                };

            offset += bytes_read;
        }

        //TODO properties

        Ok(ConnectAcknowledgement {
            is_session_present,
            connect_reason_code,
        })
    }
}

#[derive(Format, Clone)]
pub(crate) enum ConnectReasonCode {
    Success,
    ErrorCode(ConnectErrorReasonCode),
}

impl TryFrom<u8> for ConnectReasonCode {
    type Error = UnknownConnectErrorReasonCode;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(ConnectReasonCode::Success),
            0x80 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::UnspecifiedError,
            )),
            0x81 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::MalformedPacket,
            )),
            0x82 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::ProtocolError,
            )),
            0x83 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::ImplementationSpecificError,
            )),
            0x84 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::UnsupportedProtocolVersion,
            )),
            0x85 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::ClientIdentifierNotValid,
            )),
            0x86 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::BadUserNameOrPassword,
            )),
            0x87 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::NotAuthorized,
            )),
            0x88 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::ServerUnavailable,
            )),
            0x89 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::ServerBusy,
            )),
            0x8A => Ok(ConnectReasonCode::ErrorCode(ConnectErrorReasonCode::Banned)),
            0x8C => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::BadAuthenticationMethod,
            )),
            0x90 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::TopicNameInvalid,
            )),
            0x95 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::PacketTooLarge,
            )),
            0x97 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::QuotaExceeded,
            )),
            0x99 => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::PayloadFormatInvalid,
            )),
            0x9A => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::RetainNotSupported,
            )),
            0x9B => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::QosNotSupported,
            )),
            0x9C => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::UseAnotherServer,
            )),
            0x9D => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::ServerMoved,
            )),
            0x9F => Ok(ConnectReasonCode::ErrorCode(
                ConnectErrorReasonCode::ConnectionRateExceeded,
            )),
            unknown => Err(UnknownConnectErrorReasonCode(unknown)),
        }
    }
}
