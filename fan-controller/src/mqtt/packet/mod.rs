use crate::mqtt::variable_byte_integer;
use defmt::Format;

pub(crate) mod connect;
pub(crate) mod connect_acknowledgement;
pub(crate) mod disconnect;
pub(crate) mod ping_request;
pub(crate) mod ping_response;
pub(crate) mod publish;
pub(crate) mod subscribe;
pub(crate) mod subscribe_acknowledgement;

#[derive(Clone, Format, Debug)]
pub(crate) enum GetPartsError {
    EmptyBuffer,
    InvalidRemainingLength(variable_byte_integer::DecodeError),
    MissingBytes(usize),
}
pub(crate) struct PacketParts<'a> {
    pub(crate) r#type: u8,
    pub(crate) flags: u8,
    pub(crate) variable_header_and_payload: &'a [u8],
}

pub(crate) fn get_parts(buffer: &'_ [u8]) -> Result<PacketParts<'_>, GetPartsError> {
    if buffer.is_empty() {
        return Err(GetPartsError::EmptyBuffer);
    }

    let packet_type = buffer[0] >> 4;
    let flags = buffer[0] & 0b0000_1111;
    let mut offset = 1;
    let remaining_length = variable_byte_integer::decode(buffer, &mut offset)
        .map_err(GetPartsError::InvalidRemainingLength)?;

    if buffer.len() < offset + remaining_length {
        let missing_bytes = buffer.len() - offset - remaining_length;
        return Err(GetPartsError::MissingBytes(missing_bytes));
    }

    let variable_header_and_payload = &buffer[offset..offset + remaining_length];

    Ok(PacketParts {
        r#type: packet_type,
        flags,
        variable_header_and_payload,
    })
}
