use core::str::Utf8Error;

use crate::mqtt::TryEncode;
use crate::mqtt::variable_byte_integer::VariableByteIntegerEncodeError;
use crate::mqtt::{TryDecode, variable_byte_integer};
use defmt::{Debug2Format, Format, Formatter, write};

#[derive(Format, Clone)]
pub(crate) struct Publish<'a> {
    pub(crate) topic_name: &'a str,
    pub(crate) payload: &'a [u8],
}

#[derive(Debug, Format)]
pub(crate) enum EncodeError {
    EmptyBuffer,
    /// The buffer does not contain enough empty space to write the packet
    BufferTooSmall {
        required: usize,
        available: usize,
    },
    VariableByteIntegerError(VariableByteIntegerEncodeError),
}

#[derive(Debug, Clone)]
pub(crate) enum ReadError {
    /// The quality of service level is not supported and can't be ignored.
    UnsupportedQualityOfServiceLevel(u8),
    VariableByteIntegerError(variable_byte_integer::DecodeError),
    ZeroLengthTopicName,
    InvalidTopicName(Utf8Error),
}

impl Format for ReadError {
    fn format(&self, fmt: Formatter) {
        match self {
            ReadError::UnsupportedQualityOfServiceLevel(error) => {
                write!(fmt, "Unsupported quality of service level: {}", error)
            }
            ReadError::VariableByteIntegerError(error) => {
                write!(fmt, "Variable byte integer error: {}", error)
            }
            ReadError::ZeroLengthTopicName => {
                write!(fmt, "Zero length topic name")
            }
            ReadError::InvalidTopicName(error) => {
                write!(fmt, "Invalid topic name: {:?}", Debug2Format(error))
            }
        }
    }
}

impl<'a> Publish<'a> {
    pub(crate) const TYPE: u8 = 3;
}

impl TryEncode for Publish<'_> {
    type Error = EncodeError;

    fn try_encode(&self, buffer: &mut [u8], offset: &mut usize) -> Result<(), Self::Error> {
        if buffer.is_empty() {
            return Err(EncodeError::EmptyBuffer);
        }

        // Fixed header
        //TODO set flags
        buffer[*offset] = Self::TYPE << 4;
        *offset += 1;

        // Remaining length
        let topic_name_length = self.topic_name.len();
        let variable_header_length = size_of::<u16>() + topic_name_length + size_of::<u8>();
        let remaining_length = variable_header_length + self.payload.len();

        let length_length = variable_byte_integer::encode(remaining_length, buffer, offset)
            .map_err(EncodeError::VariableByteIntegerError)?;

        let required_length = size_of_val(&Self::TYPE) + length_length + remaining_length;

        if required_length > buffer.len() - *offset {
            return Err(EncodeError::BufferTooSmall {
                required: required_length,
                available: buffer.len() - *offset,
            });
        }

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

        assert_eq!(required_length, buffer[..*offset].len());

        Ok(())
    }
}

impl<'a> TryDecode<'a> for Publish<'a> {
    type Error = ReadError;

    fn try_decode(flags: u8, variable_header_and_payload: &'a [u8]) -> Result<Self, Self::Error> {
        // let is_re_delivery = (flags & 0b0000_1000) != 0;
        let quality_of_service_level = (flags & 0b0000_0110) >> 1;
        if quality_of_service_level > 0 {
            // This changes the layout of the packet and adds a packet identifier
            // which we don't expect right now
            return Err(ReadError::UnsupportedQualityOfServiceLevel(
                quality_of_service_level,
            ));
        }

        // Ignore retain flag as it should only matter for packets send to the server
        // let is_retain = (flags & 0b0000_0001) != 0;

        let mut offset = 0;
        // Variable header
        let topic_length = ((variable_header_and_payload[offset] as u16) << 8)
            | variable_header_and_payload[offset + 1] as u16;

        offset += 2;
        if topic_length == 0 {
            return Err(ReadError::ZeroLengthTopicName);
        }

        let topic_name = core::str::from_utf8(
            &variable_header_and_payload[offset..offset + topic_length as usize],
        )
        .map_err(ReadError::InvalidTopicName)?;
        //TODO validate topic name does not contain MQTT wildcard characters
        offset += topic_length as usize;

        // Properties
        let properties_length =
            variable_byte_integer::decode(variable_header_and_payload, &mut offset)
                .map_err(ReadError::VariableByteIntegerError)?;

        // Ignore properties for now
        offset += properties_length;

        // Payload
        //TODO validate there is enough space left in the buffer
        let payload = &variable_header_and_payload[offset..];
        Ok(Publish {
            topic_name,
            payload,
        })
    }
}
