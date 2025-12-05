use crate::mqtt::TryDecode;
use defmt::Format;

#[derive(Debug, Format)]
pub(crate) enum ReasonCode {
    NormalDisconnection = 0x00,
    DisconnectWithWillMessage = 0x04,
    UnspecifiedError = 0x80,
    MalformedPacket = 0x81,
    ProtocolError = 0x82,
    ImplementationSpecificError = 0x83,
    NotAuthorized = 0x87,
    ServerBusy = 0x89,
    ServerShuttingDown = 0x8B,
    KeepAliveTimeout = 0x8D,
    SessionTakenOver = 0x8E,
    TopicFilterInvalid = 0x8F,
    TopicNameInvalid = 0x90,
    ReceiveMaximumExceeded = 0x93,
    TopicAliasInvalid = 0x94,
    PacketTooLarge = 0x95,
    MessageRateTooHigh = 0x96,
    QuotaExceeded = 0x97,
    AdministrativeAction = 0x98,
    PayloadFormatInvalid = 0x99,
    RetainNotSupported = 0x9A,
    QosNotSupported = 0x9B,
    UseAnotherServer = 0x9C,
    ServerMoved = 0x9D,
    SharedSubscriptionsNotSupported = 0x9E,
    ConnectionRateExceeded = 0x9F,
    MaximumConnectTime = 0xA0,
    SubscriptionIdentifiersNotSupported = 0xA1,
    WildcardSubscriptionsNotSupported = 0xA2,
}

#[derive(Debug, Format)]
pub(crate) struct UnknownReasonCode(u8);

impl TryFrom<u8> for ReasonCode {
    type Error = UnknownReasonCode;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(ReasonCode::NormalDisconnection),
            0x04 => Ok(ReasonCode::DisconnectWithWillMessage),
            0x80 => Ok(ReasonCode::UnspecifiedError),
            0x81 => Ok(ReasonCode::MalformedPacket),
            0x82 => Ok(ReasonCode::ProtocolError),
            0x83 => Ok(ReasonCode::ImplementationSpecificError),
            0x87 => Ok(ReasonCode::NotAuthorized),
            0x89 => Ok(ReasonCode::ServerBusy),
            0x8B => Ok(ReasonCode::ServerShuttingDown),
            0x8D => Ok(ReasonCode::KeepAliveTimeout),
            0x8E => Ok(ReasonCode::SessionTakenOver),
            0x8F => Ok(ReasonCode::TopicFilterInvalid),
            0x90 => Ok(ReasonCode::TopicNameInvalid),
            0x93 => Ok(ReasonCode::ReceiveMaximumExceeded),
            0x94 => Ok(ReasonCode::TopicAliasInvalid),
            0x95 => Ok(ReasonCode::PacketTooLarge),
            0x96 => Ok(ReasonCode::MessageRateTooHigh),
            0x97 => Ok(ReasonCode::QuotaExceeded),
            0x98 => Ok(ReasonCode::AdministrativeAction),
            0x99 => Ok(ReasonCode::PayloadFormatInvalid),
            0x9A => Ok(ReasonCode::RetainNotSupported),
            0x9B => Ok(ReasonCode::QosNotSupported),
            0x9C => Ok(ReasonCode::UseAnotherServer),
            0x9D => Ok(ReasonCode::ServerMoved),
            0x9E => Ok(ReasonCode::SharedSubscriptionsNotSupported),
            0x9F => Ok(ReasonCode::ConnectionRateExceeded),
            0xA0 => Ok(ReasonCode::MaximumConnectTime),
            0xA1 => Ok(ReasonCode::SubscriptionIdentifiersNotSupported),
            0xA2 => Ok(ReasonCode::WildcardSubscriptionsNotSupported),
            unknown => Err(UnknownReasonCode(unknown)),
        }
    }
}

#[derive(Debug, Format)]
pub(crate) struct Disconnect {
    reason_code: ReasonCode,
}

impl Disconnect {
    pub(crate) const TYPE: u8 = 14;
}

#[derive(Debug, Format)]
pub(crate) enum DecodeDisconnectError {
    UnknownReasonCode(UnknownReasonCode),
}

impl TryDecode<'_> for Disconnect {
    type Error = DecodeDisconnectError;

    fn try_decode(_flags: u8, variable_header_and_payload: &[u8]) -> Result<Self, Self::Error> {
        // Variable header
        // Disconnect reason code
        let reason_code = variable_header_and_payload[0];
        let reason_code =
            ReasonCode::try_from(reason_code).map_err(DecodeDisconnectError::UnknownReasonCode)?;

        //TODO decode more when needed
        // Properties
        Ok(Disconnect { reason_code })
    }
}
