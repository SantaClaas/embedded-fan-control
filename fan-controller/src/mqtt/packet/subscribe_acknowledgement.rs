use crate::mqtt::variable_byte_integer;
use defmt::Format;

#[derive(Debug, Clone, Format)]
pub(crate) enum SubscribeAcknowledgementError {
    InvalidPropertiesLength(variable_byte_integer::DecodeError),
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

        // Payload
        // Reason code for each subscribed topic in the same order
        let reason_codes = &buffer[offset..];

        Ok(SubscribeAcknowledgement {
            packet_identifier,
            reason_codes,
        })
    }
}
