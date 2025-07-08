use crate::mqtt::variable_byte_integer;
use ::mqtt::QualityOfService;
use defmt::Format;

#[derive(Debug, Clone, Format)]
pub(crate) enum SubscribeAcknowledgementError {
    InvalidPropertiesLength(variable_byte_integer::DecodeError),
}

#[derive(Format)]
pub(crate) enum SubscribeErrorReasonCode {
    UnspecifiedError = 0x80,
    ImplementationSpecificError = 0x83,
    NotAuthorized = 0x87,
    TopicFilterInvalid = 0x8F,
    PacketIdentifierInUse = 0x91,
    QuotaExceeded = 0x97,
    SharedSubscriptionsNotSupported = 0x9E,
    SubscriptionIdentifiersNotSupported = 0xA1,
    WildcardSubscriptionsNotSupported = 0xA2,
}

#[derive(Format)]
pub(crate) enum SubscribeReasonCode {
    GrantedQualityOfService(QualityOfService),
    ErrorCode(SubscribeErrorReasonCode),
}

#[derive(Format)]
pub(crate) struct SubscribeAcknowledgement<'a> {
    pub(crate) packet_identifier: u16,
    /// Reason code for each subscribed topic in the same order
    pub(crate) reason_codes: &'a [u8],
}

impl<'a> SubscribeAcknowledgement<'a> {
    pub(crate) const TYPE: u8 = 9;
    //TODO convert to decode trait
    pub(crate) fn read(buffer: &'a [u8]) -> Result<Self, SubscribeAcknowledgementError> {
        // Variable header
        let packet_identifier: u16 = ((buffer[0] as u16) << 8) | buffer[1] as u16;

        let mut offset = 2;
        let properties_length = variable_byte_integer::decode(buffer, &mut offset)
            .map_err(SubscribeAcknowledgementError::InvalidPropertiesLength)?;

        //TODO stop ignoring properties
        //TODO check if topics are acknowledged

        offset += properties_length;

        // const DEFAULT: Option<SubscribeReasonCode> = None;
        // let mut reason_codes = [DEFAULT; 2];
        // Payload
        // Reason code for each subscribed topic in the same order
        let reason_codes = &buffer[offset..];
        // let mut index = 0;
        // while index < reason_codes.len() {
        //     let Some(code) = buffer.get(offset + index) else {
        //         break;
        //     };
        //
        //     reason_codes[index] = Some(match code {
        //         0x00 => SubscribeReasonCode::GrantedQualityOfService(
        //             QualityOfService::AtMostOnceDelivery,
        //         ),
        //         0x01 => SubscribeReasonCode::GrantedQualityOfService(
        //             QualityOfService::AtLeastOnceDelivery,
        //         ),
        //         0x02 => SubscribeReasonCode::GrantedQualityOfService(
        //             QualityOfService::ExactlyOnceDelivery,
        //         ),
        //         0x80 => SubscribeReasonCode::ErrorCode(SubscribeErrorReasonCode::UnspecifiedError),
        //         0x83 => SubscribeReasonCode::ErrorCode(
        //             SubscribeErrorReasonCode::ImplementationSpecificError,
        //         ),
        //         0x87 => SubscribeReasonCode::ErrorCode(SubscribeErrorReasonCode::NotAuthorized),
        //         0x8F => {
        //             SubscribeReasonCode::ErrorCode(SubscribeErrorReasonCode::TopicFilterInvalid)
        //         }
        //         0x91 => {
        //             SubscribeReasonCode::ErrorCode(SubscribeErrorReasonCode::PacketIdentifierInUse)
        //         }
        //         0x97 => SubscribeReasonCode::ErrorCode(SubscribeErrorReasonCode::QuotaExceeded),
        //         0x9E => SubscribeReasonCode::ErrorCode(
        //             SubscribeErrorReasonCode::SharedSubscriptionsNotSupported,
        //         ),
        //         0xA1 => SubscribeReasonCode::ErrorCode(
        //             SubscribeErrorReasonCode::SubscriptionIdentifiersNotSupported,
        //         ),
        //         0xA2 => SubscribeReasonCode::ErrorCode(
        //             SubscribeErrorReasonCode::WildcardSubscriptionsNotSupported,
        //         ),
        //         other => {
        //             //TODO handle invalid reason code
        //             warn!("Invalid reason code: {:?}", other);
        //             break;
        //         }
        //     });
        //
        //     index += 1;
        // }

        Ok(SubscribeAcknowledgement {
            packet_identifier,
            reason_codes,
        })
    }
}
