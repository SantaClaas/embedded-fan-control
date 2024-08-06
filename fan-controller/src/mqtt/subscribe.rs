use defmt::Format;

use crate::mqtt::{QualityOfService, variable_byte_integer};

pub(crate) struct Options(u8);

/// Retain handling option of the subscription options
/// Specifies whether retained messages are sent when the subscription is established.
/// Does not affect sending of retained messages after subscribe. If there are no retained messages
/// matching a topic filter, then all options act the same.
pub(crate) enum RetainHandling {
    // Send retained messages at the time of subscribe (0)
    SendAtSubscribe,
    // Send retained messages only if the subscription does not currently exist (1)
    OnlyIfNotExists,
    // Do not send retained messages (2)
    DoNotSend,
}

impl RetainHandling {
    const fn to_byte(&self) -> u8 {
        match self {
            RetainHandling::SendAtSubscribe => 0,
            RetainHandling::OnlyIfNotExists => 1,
            RetainHandling::DoNotSend => 2,
        }
    }
}

impl Options {
    pub(crate) const fn new(
        maximum_quality_of_service: QualityOfService,
        is_no_local: bool,
        is_retain_as_published: bool,
        retain_handling: RetainHandling,
    ) -> Self {
        Self(
            // Bit 0 and 1 (no shift necessary)
            maximum_quality_of_service.to_byte()
                // Boolean values are stored in a byte as either 0x00 or 0x01
                // Meaning the first bit is on for true and off for false
                // << 2 shifts that bit to the 3rd position (0 indexed)
                // Bit 2
                | (is_no_local as u8) << 2
                // Bit 3
                | (is_retain_as_published as u8) << 3
                // Bit 4 and 5
                | (retain_handling.to_byte() << 4),
            // Bit 6 and 7 are reserved and must be set to 0
        )
    }
}

pub(crate) struct Subscription<'a> {
    pub(crate) topic_filter: &'a str,
    pub(crate) options: Options,
}

impl<'a> Subscription<'a> {
    pub(crate) fn length(&self) -> usize {
        self.topic_filter.len() + size_of::<u8>()
    }
}

#[derive(Debug, Format)]
pub(crate) enum WriteError {
    /// The buffer does not contain enough empty space to write the packet
    BufferTooSmall {
        required: usize,
        available: usize,
    },
    RemainingLengthError(variable_byte_integer::VariableByteIntegerEncodeError),
}

pub(crate) struct Subscribe<'a, const N: usize> {
    pub(crate) subscriptions: [Subscription<'a>; N],
    pub(crate) packet_identifier: u16,
}

impl<'a, const N: usize> Subscribe<'a, N> {
    pub(crate) const TYPE: u8 = 8;
    pub(crate) fn write(self, buffer: &mut [u8], offset: &mut usize) -> Result<(), WriteError> {
        let variable_header_length = size_of::<u16>() + size_of::<u8>();

        let payload_length = self.subscriptions.len() * size_of::<u16>()
            + self
                .subscriptions
                .iter()
                .map(|subscription| subscription.length())
                .sum::<usize>();

        let remaining_length = variable_header_length + payload_length;
        let required_length = size_of_val(&Self::TYPE) + remaining_length;
        if required_length > buffer.len() - *offset {
            return Err(WriteError::BufferTooSmall {
                required: required_length,
                available: buffer.len() - *offset,
            });
        }

        buffer[*offset] = Self::TYPE << 4;
        *offset += 1;

        variable_byte_integer::encode(remaining_length, buffer, offset)
            .map_err(WriteError::RemainingLengthError)?;

        // Variable header
        // Packet Identifier
        buffer[*offset] = (self.packet_identifier >> 8) as u8;
        *offset += 1;

        buffer[*offset] = self.packet_identifier as u8;
        *offset += 1;

        // Property length
        // No properties supported for now so set to 0
        buffer[*offset] = 0;
        *offset += 1;

        for subscription in self.subscriptions {
            // Topic name length
            let topic_name_length = subscription.topic_filter.len() as u16;
            buffer[*offset] = (topic_name_length >> 8) as u8;
            *offset += 1;
            buffer[*offset] = topic_name_length as u8;
            *offset += 1;

            // Topic name
            for byte in subscription.topic_filter.as_bytes() {
                buffer[*offset] = *byte;
                *offset += 1;
            }

            // Options
            buffer[*offset] = subscription.options.0;
            *offset += 1;
        }

        Ok(())
    }
}
