use crate::mqtt::variable_byte_integer;
use crate::mqtt::variable_byte_integer::VariableByteIntegerEncodeError;
use crate::mqtt::TryEncode;
use defmt::Format;

pub(crate) struct Connect<'a> {
    pub(crate) client_identifier: &'a str,
    pub(crate) username: &'a str,
    pub(crate) password: &'a [u8],
    pub(crate) keep_alive_seconds: u16,
}

impl<'a> Connect<'a> {
    pub(crate) const TYPE: u8 = 1;

    #[deprecated(note = "Use Encode trait")]
    pub(crate) fn encode(&self, buffer: &mut [u8], offset: &mut usize) -> Result<(), EncodeError> {
        let remaining_length = 11
            + size_of::<u16>()
            + self.client_identifier.len()
            + size_of::<u16>()
            + self.username.len()
            + size_of::<u16>()
            + self.password.len();

        let required_length = size_of_val(&Self::TYPE) + remaining_length;
        if required_length > buffer.len() - *offset {
            return Err(EncodeError::BufferTooSmall {
                required: required_length,
                available: buffer.len() - *offset,
            });
        }

        // Fixed header
        buffer[*offset] = Self::TYPE << 4;
        *offset += 1;

        variable_byte_integer::encode(remaining_length, buffer, offset)
            .map_err(EncodeError::WriteRemainingLengthError)?;

        // Variable header
        // Protocol name length
        buffer[*offset] = 0x00;
        *offset += 1;
        buffer[*offset] = 4;
        *offset += 1;

        // Protocol name
        buffer[*offset] = b'M';
        *offset += 1;
        buffer[*offset] = b'Q';
        *offset += 1;
        buffer[*offset] = b'T';
        *offset += 1;
        buffer[*offset] = b'T';
        *offset += 1;
        // Protocol version
        buffer[*offset] = 5;
        *offset += 1;

        // Connect Flags
        // USER_NAME_FLAG | PASSWORD_FLAG | CLEAN_START
        buffer[*offset] = 0b1100_0010;
        *offset += 1;

        // Keep alive
        buffer[*offset] = (self.keep_alive_seconds >> 8) as u8;
        *offset += 1;
        buffer[*offset] = self.keep_alive_seconds as u8;
        *offset += 1;
        // Property length 0 (no properties). Has to be set to 0 if there are no properties
        buffer[*offset] = 0;
        *offset += 1;

        // Payload
        // Client identifier
        let length = self.client_identifier.len();
        buffer[*offset] = (length >> 8) as u8;
        *offset += 1;
        buffer[*offset] = length as u8;
        *offset += 1;

        for byte in self.client_identifier.as_bytes() {
            buffer[*offset] = *byte;
            *offset += 1;
        }
        // Username
        let length = self.username.len();
        buffer[*offset] = (length >> 8) as u8;
        *offset += 1;
        buffer[*offset] = length as u8;
        *offset += 1;
        for byte in self.username.as_bytes() {
            buffer[*offset] = *byte;
            *offset += 1;
        }

        // Password
        let length = self.password.len();
        buffer[*offset] = (length >> 8) as u8;
        *offset += 1;
        buffer[*offset] = length as u8;
        *offset += 1;
        for byte in self.password {
            buffer[*offset] = *byte;
            *offset += 1;
        }

        Ok(())
    }
}

impl TryEncode for Connect<'_> {
    type Error = EncodeError;

    fn try_encode(&self, buffer: &mut [u8], offset: &mut usize) -> Result<(), Self::Error> {
        let remaining_length = 11
            + size_of::<u16>()
            + self.client_identifier.len()
            + size_of::<u16>()
            + self.username.len()
            + size_of::<u16>()
            + self.password.len();

        let required_length = size_of_val(&Self::TYPE) + remaining_length;
        if required_length > buffer.len() - *offset {
            return Err(EncodeError::BufferTooSmall {
                required: required_length,
                available: buffer.len() - *offset,
            });
        }

        // Fixed header
        buffer[*offset] = Self::TYPE << 4;
        *offset += 1;

        variable_byte_integer::encode(remaining_length, buffer, offset)
            .map_err(EncodeError::WriteRemainingLengthError)?;

        // Variable header
        // Protocol name length
        buffer[*offset] = 0x00;
        *offset += 1;
        buffer[*offset] = 4;
        *offset += 1;

        // Protocol name
        buffer[*offset] = b'M';
        *offset += 1;
        buffer[*offset] = b'Q';
        *offset += 1;
        buffer[*offset] = b'T';
        *offset += 1;
        buffer[*offset] = b'T';
        *offset += 1;
        // Protocol version
        buffer[*offset] = 5;
        *offset += 1;

        // Connect Flags
        // USER_NAME_FLAG | PASSWORD_FLAG | CLEAN_START
        buffer[*offset] = 0b1100_0010;
        *offset += 1;

        // Keep alive
        buffer[*offset] = (self.keep_alive_seconds >> 8) as u8;
        *offset += 1;
        buffer[*offset] = self.keep_alive_seconds as u8;
        *offset += 1;
        // Property length 0 (no properties). Has to be set to 0 if there are no properties
        buffer[*offset] = 0;
        *offset += 1;

        // Payload
        // Client identifier
        let length = self.client_identifier.len();
        buffer[*offset] = (length >> 8) as u8;
        *offset += 1;
        buffer[*offset] = length as u8;
        *offset += 1;

        for byte in self.client_identifier.as_bytes() {
            buffer[*offset] = *byte;
            *offset += 1;
        }
        // Username
        let length = self.username.len();
        buffer[*offset] = (length >> 8) as u8;
        *offset += 1;
        buffer[*offset] = length as u8;
        *offset += 1;
        for byte in self.username.as_bytes() {
            buffer[*offset] = *byte;
            *offset += 1;
        }

        // Password
        let length = self.password.len();
        buffer[*offset] = (length >> 8) as u8;
        *offset += 1;
        buffer[*offset] = length as u8;
        *offset += 1;
        for byte in self.password {
            buffer[*offset] = *byte;
            *offset += 1;
        }

        Ok(())
    }
}

#[derive(Debug, Format)]
pub(crate) enum EncodeError {
    /// Client identifier + user name + password together are larger than [VariableByteInteger::MAX]
    DataTooLarge,
    /// The buffer does not contain enough empty space to write the packet
    BufferTooSmall {
        required: usize,
        available: usize,
    },
    WriteRemainingLengthError(VariableByteIntegerEncodeError),
}
