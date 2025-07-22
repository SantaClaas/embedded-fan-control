use core::ops::Range;

use defmt::Format;

use crate::mqtt::variable_byte_integer;

#[derive(Clone, Format, Debug)]
pub(crate) enum PacketError {
    EmptyBuffer,
    InvalidRemainingLength(variable_byte_integer::DecodeError),
    MissingBytes(usize),
}

trait Packetter {
    fn test() -> () {
        todo!()
    }
}

enum Packett {}

struct Packet<const L: usize> {
    buffer: [u8; L],
    variable_header_and_payload: Range<usize>,
}

impl<const L: usize> Packet<L> {
    fn r#type(&self) -> u8 {
        self.buffer[0] >> 4
    }

    fn flags(&self) -> u8 {
        self.buffer[0] & 0b0000_1111
    }

    fn decode(self) -> Result<Packett, ()> {
        todo!()
    }
}

impl<const L: usize> TryFrom<[u8; L]> for Packet<L> {
    type Error = PacketError;

    fn try_from(buffer: [u8; L]) -> Result<Self, Self::Error> {
        if buffer.is_empty() {
            return Err(PacketError::EmptyBuffer);
        }

        // Skip fixed header
        let mut offset = 1;
        let remaining_length = variable_byte_integer::decode(&buffer, &mut offset)
            .map_err(PacketError::InvalidRemainingLength)?;

        if buffer.len() < offset + remaining_length {
            let missing_bytes = buffer.len() - offset - remaining_length;
            return Err(PacketError::MissingBytes(missing_bytes));
        }

        Ok(Self {
            buffer,
            variable_header_and_payload: offset..offset + remaining_length,
        })
    }
}
