use crate::mqtt::variable_byte_integer;
use crate::mqtt::variable_byte_integer::VariableByteIntegerDecodeError;
use defmt::Format;

#[derive(Debug, Clone)]
pub(crate) enum SubscribeAcknowledgementError {
    InvalidPropertiesLength(VariableByteIntegerDecodeError),
}

#[derive(Format, Clone)]
pub(crate) struct SubscribeAcknowledgement;
impl SubscribeAcknowledgement {
    pub(super) const TYPE: u8 = 9;
    pub(crate) fn read(buffer: &[u8]) -> Result<Self, SubscribeAcknowledgementError> {
        // Variable header
        let packet_identifier: u16 = ((buffer[0] as u16) << 8) | buffer[1] as u16;

        let mut offset = 2;
        let properties_length = variable_byte_integer::decode(buffer, &mut offset)
            .map_err(SubscribeAcknowledgementError::InvalidPropertiesLength)?;

        //TODO stop ignoring properties
        //TODO check if topics are acknowledged

        // offset += properties_length;
        //
        // // Payload
        // // Reason code for each subscribed topic in the same order
        // let mut topic_index = 0;
        //
        // loop {
        //     let reason_code = buffer[offset];
        //     offset += 1;
        //
        // }

        Ok(SubscribeAcknowledgement)
    }
}
