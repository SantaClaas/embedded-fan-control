//! Module containing all the MQTT things to enable the fan controller to be integrated with Home
//! Assistant

use core::convert::Infallible;

use defmt::Format;

pub(crate) mod client;
pub(crate) mod packet;
pub(crate) mod task;
pub(crate) mod v2;
pub(crate) mod variable_byte_integer;

#[derive(Debug, Format, Clone)]
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

#[derive(Debug, Clone, Format)]
pub struct UnknownConnectErrorReasonCode(u8);

pub(crate) trait TryEncode {
    type Error;
    fn try_encode(&self, buffer: &mut [u8], offset: &mut usize) -> Result<(), Self::Error>;
}

impl<T: crate::task::Publish> TryEncode for T {
    type Error = publish::EncodeError;

    fn try_encode(&self, buffer: &mut [u8], offset: &mut usize) -> Result<(), Self::Error> {
        if buffer.is_empty() {
            return Err(publish::EncodeError::EmptyBuffer);
        }

        // Fixed header
        //TODO set flags
        buffer[*offset] = Self::TYPE << 4;
        *offset += 1;

        let topic_name = self.topic();
        // Remaining length
        let topic_name_length = topic_name.len();
        let variable_header_length = size_of::<u16>() + topic_name_length + size_of::<u8>();
        let payload = self.payload();
        let remaining_length = variable_header_length + payload.len();

        let length_length = variable_byte_integer::encode(remaining_length, buffer, offset)
            .map_err(publish::EncodeError::VariableByteIntegerError)?;

        let required_length = size_of_val(&Self::TYPE) + length_length + remaining_length;

        if required_length > buffer.len() - *offset {
            return Err(publish::EncodeError::BufferTooSmall {
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
        for byte in topic_name.as_bytes() {
            buffer[*offset] = *byte;
            *offset += 1;
        }

        // Property length
        // No properties supported for now so set to 0
        buffer[*offset] = 0;
        *offset += 1;

        // Payload
        // No need to set length as it will be calculated
        for byte in payload {
            buffer[*offset] = *byte;
            *offset += 1;
        }

        assert_eq!(required_length, buffer[..*offset].len());

        Ok(())
    }
}

pub(crate) trait Decode<'a> {
    fn decode(flags: u8, variable_header_and_payload: &'a [u8]) -> Self
    where
        Self: Sized;
}

pub(crate) trait TryDecode<'a> {
    type Error;
    fn try_decode(flags: u8, variable_header_and_payload: &'a [u8]) -> Result<Self, Self::Error>
    where
        Self: Sized;
}

impl<'a, T: Decode<'a>> TryDecode<'a> for T {
    type Error = Infallible;

    fn try_decode(flags: u8, variable_header_and_payload: &'a [u8]) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        let value = T::decode(flags, variable_header_and_payload);
        Ok(value)
    }
}

/// Helps to mark an integer as a non zero integer when you know at compile time that it is not zero
macro_rules! non_zero_u16 {
    (0) => {
        compile_error!("0 is not a valid NonZeroU16")
    };
    ($value:expr) => {{
        const VALUE: core::num::NonZeroU16 = {
            let Some(value) = core::num::NonZeroU16::new($value) else {
                // Using compile_error!() would always cause a compile error.
                // So panic at const time it is
                // Formatting goes strange here
            core::panic!(core::stringify!($value is not a valid NonZeroU16))
            };

            value
        };

        VALUE
    }};
}

pub(crate) use non_zero_u16;

use crate::mqtt::packet::publish;
