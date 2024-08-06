use crate::mqtt::variable_byte_integer;
use crate::mqtt::variable_byte_integer::VariableByteIntegerEncodeError;

pub(crate) struct Publish<'a> {
    pub(crate) topic_name: &'a str,
    pub(crate) payload: &'a [u8],
}

pub(crate) enum WriteError {
    VariableByteIntegerError(VariableByteIntegerEncodeError),
}

impl<'a> Publish<'a> {
    const TYPE: u8 = 3;

    pub(crate) fn write(&self, buffer: &mut [u8], offset: &mut usize) -> Result<(), WriteError> {
        // Fixed header
        //TODO set flags
        buffer[*offset] = Self::TYPE << 4;
        *offset += 1;

        // Remaining length
        let topic_name_length = self.topic_name.len();
        let variable_header_length = size_of::<u16>() + topic_name_length + size_of::<u8>();
        let remaining_length = variable_header_length + self.payload.len();
        variable_byte_integer::encode(remaining_length, buffer, offset)
            .map_err(WriteError::VariableByteIntegerError)?;

        // Variable header
        // Topic name length
        buffer[*offset] = (topic_name_length >> 8) as u8;
        *offset += 1;
        buffer[*offset] = topic_name_length as u8;
        *offset += 1;
        // Topic name
        for byte in self.topic_name.as_bytes() {
            buffer[*offset] = *byte;
            *offset += 1;
        }

        // Property length
        // No properties supported for now so set to 0
        buffer[*offset] = 0;
        *offset += 1;

        // Payload
        // No need to set length as it will be calculated
        for byte in self.payload {
            buffer[*offset] = *byte;
            *offset += 1;
        }

        Ok(())
    }
}
