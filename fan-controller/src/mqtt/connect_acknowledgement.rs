use crate::mqtt::{
    ConnectErrorReasonCode, ReadConnectAcknowledgementError, UnknownConnectErrorReasonCode,
};

pub(crate) struct ConnectAcknowledgement {
    pub(crate) is_session_present: bool,
    pub(crate) connect_reason_code: ConnectReasonCode,
}

impl ConnectAcknowledgement {
    pub(crate) const TYPE: u8 = 2;
    /// Reads the variable header and payload of a connect acknowledgement packet
    pub(crate) fn read(buffer: &[u8]) -> Result<Self, ReadConnectAcknowledgementError> {
        // Fixed header is already read.
        // Read variable header
        let is_session_present = (buffer[0] & 0b0000_0001) != 0;
        let connect_reason_code = ConnectReasonCode::try_from(buffer[1])
            .map_err(ReadConnectAcknowledgementError::InvalidReasonCode)?;

        // Properties
        // let mut offset = 2;
        // let length = variable_byte_integer::decode(buffer, &mut offset)
        //     .map_err(ReadConnectAcknowledgementError::InvalidPropertiesLength)?;

        //TODO properties

        Ok(ConnectAcknowledgement {
            is_session_present,
            connect_reason_code,
        })
    }
}

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
