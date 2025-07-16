use defmt::Format;

#[derive(Debug, Format)]
pub(super) enum VariableByteIntegerEncodeError {
    /// The integer is larger than 268,435,455  ([VariableByteInteger::MAX]).
    TooLarge,
    /// The buffer is too small to write the integer
    EndOfBuffer,
}
const MAX: usize = 268_435_455;
/// Returns bytes written on success
pub(super) fn encode(
    value: usize,
    buffer: &mut [u8],
    offset: &mut usize,
) -> Result<usize, VariableByteIntegerEncodeError> {
    // This checks if the length is also too large for u32
    let length = match value {
        0..=127 => 1,
        128..=16_383 => 2,
        16_384..=2_097_151 => 3,
        2_097_152..=MAX => 4,
        _ => return Err(VariableByteIntegerEncodeError::TooLarge),
    };

    if buffer.len() <= *offset + length {
        return Err(VariableByteIntegerEncodeError::EndOfBuffer);
    }

    // Short circuit if it fits in one byte (0-127)
    if value < 128 {
        buffer[*offset] = value as u8;
        *offset += 1;
        return Ok(1);
    }

    let mut value = value as u32;
    loop {
        let mut encoded_byte = value % 0b1000_0000;
        value /= 0b1000_0000;
        if value > 0 {
            encoded_byte |= 0b1000_0000;
        }
        buffer[*offset] = encoded_byte as u8;
        *offset += 1;
        if value == 0 {
            break;
        }
    }

    Ok(length)
}

#[derive(Debug, Format, Clone)]
pub(super) enum DecodeError {
    MalformedVariableByteIntegerError,
    /// The buffer does not contain enough bytes to read the remaining length
    EndOfBuffer,
    /// The last byte indicates that the next byte is part of the variable byte integer but there was no next byte
    UnexpectedEndOfBuffer,

    InvalidLength,
}

pub(super) fn decode(buffer: &[u8], offset: &mut usize) -> Result<usize, DecodeError> {
    if buffer.len() <= *offset {
        return Err(DecodeError::EndOfBuffer);
    }

    // Shortcut if the length is 1 (bit 7 is 0)
    if (buffer[*offset] & 0b1000_0000) == 0 {
        let value = buffer[*offset] as usize;
        *offset += 1;
        return Ok(value);
    }

    let mut multiplier = 1;
    let mut value = 0;
    let mut length: u8 = 0;

    loop {
        let encoded_byte = buffer[*offset];
        *offset += 1;
        length += 1;

        value += (encoded_byte & 127) as usize * multiplier;

        if multiplier > 128 * 128 * 128 {
            return Err(DecodeError::MalformedVariableByteIntegerError);
        }

        multiplier *= 128;

        // The last byte has the most significant bit set to 0 indicating that there are no more bytes to follow
        if (encoded_byte & 128) == 0 {
            break;
        }

        // The current byte indicates the next byte is part of the integer, but we would be past 4 bytes
        if length > 4 {
            return Err(DecodeError::InvalidLength);
        }

        if buffer.len() <= *offset {
            return Err(DecodeError::UnexpectedEndOfBuffer);
        }
    }

    Ok(value)
}
