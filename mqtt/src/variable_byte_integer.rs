/// Encodes an integer as variable byte integer according to the MQTT specification
fn encode_variable_byte_integer(mut value: u32) -> Vec<u8> {
    // Each byte can hold 7 bits, so how many 7 bit "bytes" do we need for 32 bits?
    // 32 / 7
    let length: usize = match value {
        0..=127 => 1,
        128..=16_383 => 2,
        16_384..=2_097_151 => 3,
        2_097_152..=268_435_455 => 4,
        _ => 5,
    };
    let mut output = Vec::with_capacity(length);
    loop {
        // 128 = 0b1000_0000

        // x = 128 = 0b1000_0000
        // encoded_byte = 128 % 0b1000_0000 = 0
        let mut encoded_byte = (value % 0b1000_0000);

        // x = 128 / 128 = 1
        value = value / 0b1000_0000;
        // If there is more data to encode, set the top bit of this byte
        if value > 0 {
            encoded_byte |= 0b1000_0000;
        }

        output.push(encoded_byte as u8);

        if value == 0 {
            break;
        }
    }

    output
}

const fn length<const N: u32>() -> usize {
    if N < 128 {
        return 1;
    }

    if N < 16_384 {
        return 2;
    }

    if N < 2_097_152 {
        return 3;
    }

    if N < 268_435_456 {
        return 4;
    }

    panic!("Variable byte integer can only hold up to 268_435_455 which is 4 bytes");
}

const fn encode<const N: usize>(mut value: u32) -> [u8; N] {
    let mut output = [0; N];
    let mut index = 0;

    loop {
        let mut encoded_byte = (value % 0b1000_0000);
        value = value / 0b1000_0000;
        if value > 0 {
            encoded_byte |= 0b1000_0000;
        }
        output[index] = encoded_byte as u8;
        index += 1;
        if value == 0 {
            break;
        }
    }
    output
}

trait VariableByteInteger {
    const LENGTH: usize;
    const MAX: u32 = 268_435_455;
}

enum VariableByteIntegerVariant {
    OneByte([u8; 1]),
    TwoBytes([u8; 2]),
    ThreeBytes([u8; 3]),
    FourBytes([u8; 4]),
}

enum VariableByteIntegerError {
    // The integer is too large to be encoded.
    TooLarge,
}

impl TryFrom<u32> for VariableByteIntegerVariant {
    type Error = VariableByteIntegerError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        let length = match value {
            0..=127 => 1,
            128..=16_383 => 2,
            16_384..=2_097_151 => 3,
            2_097_152..=268_435_455 => 4,
            _ => return Err(VariableByteIntegerError::TooLarge),
        };
        todo!()
    }
}

#[test]
fn can_encode_variable_byte_integer() {
    // Random examples from https://www.emqx.com/en/blog/mqtt-5-0-control-packets-01-connect-connack
    assert_eq!(encode_variable_byte_integer(47), [0x2f]);
    assert_eq!(encode_variable_byte_integer(19), [0x13]);

    // Range one byte
    assert_eq!(encode_variable_byte_integer(0), [0b0000_0000]);
    assert_eq!(encode_variable_byte_integer(1), [0b0000_0001]);
    assert_eq!(encode_variable_byte_integer(127), [0b0111_1111]);
    // Range two bytes
    assert_eq!(
        encode_variable_byte_integer(128),
        [0b1000_0000, 0b0000_0001]
    );
    assert_eq!(
        encode_variable_byte_integer(16_383),
        [0b1111_1111, 0b0111_1111]
    );

    // Range three bytes
    assert_eq!(
        encode_variable_byte_integer(16_384),
        [0b1000_0000, 0b1000_0000, 0b0000_0001]
    );
    assert_eq!(
        encode_variable_byte_integer(2_097_151),
        [0b1111_1111, 0b1111_1111, 0b0111_1111]
    );
    // Range four bytes
    assert_eq!(
        encode_variable_byte_integer(2_097_152),
        [0b1000_0000, 0b1000_0000, 0b1000_0000, 0b0000_0001]
    );
    assert_eq!(
        encode_variable_byte_integer(268_435_455),
        [0b1111_1111, 0b1111_1111, 0b1111_1111, 0b0111_1111]
    );
}

#[derive(Debug)]
enum DecodeVariableByteIntegerError {
    MalformedVariableByteIntegerError,
    UnexpectedEndOfInput,
}

fn decode_variable_byte_integer(
    bytes: impl IntoIterator<Item = u8> + std::fmt::Debug,
) -> Result<u32, DecodeVariableByteIntegerError> {
    let mut multiplier = 1;
    let mut value: u32 = 0;

    let mut bytes = bytes.into_iter();
    loop {
        let encoded_byte = bytes
            .next()
            .ok_or(DecodeVariableByteIntegerError::UnexpectedEndOfInput)?;

        value += (encoded_byte & 127) as u32 * multiplier;

        if multiplier > 128 * 128 * 128 {
            return Err(DecodeVariableByteIntegerError::MalformedVariableByteIntegerError);
        }

        multiplier *= 128;

        // The last byte has the most significant bit set to 0 indicating that there are no more bytes to follow
        if (encoded_byte & 128) != 0 {
            continue;
        } else {
            break;
        }
    }

    Ok(value)
}

#[test]
fn can_decode_variable_byte_integer() {
    // Random examples from https://www.emqx.com/en/blog/mqtt-5-0-control-packets-01-connect-connack
    assert_eq!(decode_variable_byte_integer([0x2f]).unwrap(), 47);
    assert_eq!(decode_variable_byte_integer([0x13]).unwrap(), 19);

    // Range one byte
    assert_eq!(decode_variable_byte_integer([0b0000_0000]).unwrap(), 0);
    assert_eq!(decode_variable_byte_integer([0b0000_0001]).unwrap(), 1);
    assert_eq!(decode_variable_byte_integer([0b0111_1111]).unwrap(), 127);

    // Range two bytes
    assert_eq!(
        decode_variable_byte_integer([0b1000_0000, 0b0000_0001]).unwrap(),
        128
    );
    assert_eq!(
        decode_variable_byte_integer([0b1111_1111, 0b0111_1111]).unwrap(),
        16_383
    );

    // Range three bytes
    assert_eq!(
        decode_variable_byte_integer([0b1000_0000, 0b1000_0000, 0b0000_0001]).unwrap(),
        16_384
    );
    assert_eq!(
        decode_variable_byte_integer([0b1111_1111, 0b1111_1111, 0b0111_1111]).unwrap(),
        2_097_151
    );

    // Range four bytes
    assert_eq!(
        decode_variable_byte_integer([0b1000_0000, 0b1000_0000, 0b1000_0000, 0b0000_0001]).unwrap(),
        2_097_152
    );
    assert_eq!(
        decode_variable_byte_integer([0b1111_1111, 0b1111_1111, 0b1111_1111, 0b0111_1111]).unwrap(),
        268_435_455
    );
}
