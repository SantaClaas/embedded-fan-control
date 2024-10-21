use core::num::NonZeroU16;

use defmt::{info, Format};

use crate::mqtt::TryEncode;
use crate::mqtt::{variable_byte_integer, QualityOfService};

#[derive(Debug)]
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

#[derive(Debug)]
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
pub(crate) enum EncodeError {
    /// The buffer does not contain enough empty space to write the packet
    BufferTooSmall {
        required: usize,
        available: usize,
    },
    RemainingLengthError(variable_byte_integer::VariableByteIntegerEncodeError),
}

#[derive(Debug)]
pub(crate) struct Subscribe<'a> {
    pub(crate) subscriptions: &'a [Subscription<'a>],
    /// Packet identifier of 0 is invalid even though it is only used by client to keep track of packets
    pub(crate) packet_identifier: NonZeroU16,
}

impl<'a> Subscribe<'a> {
    pub(crate) const TYPE: u8 = 8;
}

impl TryEncode for Subscribe<'_> {
    type Error = EncodeError;

    // https://www.emqx.com/en/blog/mqtt-5-0-control-packets-03-subscribe-unsubscribe
    fn encode(&self, buffer: &mut [u8], offset: &mut usize) -> Result<(), Self::Error> {
        // 82 0a 05 be 00 00 04 64 65 6d 6f 02
        // let test_packet = &[
        //     0x82, 0x0a, 0x05, 0xbe, 0x00, 0x00, 0x04, 0x64, 0x65, 0x6d, 0x6f, 0x02,
        // ];
        // for byte in test_packet {
        //     buffer[*offset] = *byte;
        //     *offset += 1;
        // }

        // return Ok(());

        // Calculate lengths to bail early if buffer is too small
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
            return Err(EncodeError::BufferTooSmall {
                required: required_length,
                available: buffer.len() - *offset,
            });
        }

        // Need to set type and fixed/reserved bit in first byte
        buffer[*offset] = Self::TYPE << 4 | 0b0000_0010;
        *offset += 1;

        variable_byte_integer::encode(remaining_length, buffer, offset)
            .map_err(EncodeError::RemainingLengthError)?;

        // Variable header
        // Packet Identifier
        buffer[*offset] = (Into::<u16>::into(self.packet_identifier) >> 8) as u8;
        *offset += 1;

        //TODO support identifiers greater than u8::MAX
        let identifier: u16 = self.packet_identifier.into();
        buffer[*offset] = identifier as u8;
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
