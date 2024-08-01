use std::num::{NonZero, NonZeroUsize};

/// Encodes an integer as variable byte integer according to the MQTT specification
pub(super) fn encode(mut value: u32) -> Vec<u8> {
    //TODO Try to accept any integer type
    //TODO Error if value is too large

    // Each byte can hold 7 bits, so how many 7 bit "bytes" do we need for 32 bits?
    // 32 / 7
    let length: usize = match value {
        // Short circuit when it fits in one byte
        0..=127 => return vec![value as u8],
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
        let mut encoded_byte = value % 0b1000_0000;

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

const fn encode_const<const N: usize>(mut value: u32) -> [u8; N] {
    let mut output = [0; N];
    let mut index = 0;

    loop {
        let mut encoded_byte = value % 0b1000_0000;
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

enum VariableByteIntegerError {
    /// The integer is larger than 268,435,455  ([VariableByteInteger::MAX]).
    TooLarge,
}

/// The variable byte integer is an integer that can be encoded in 1-4 bytes.
/// The first bit of every byte is a continuation bit, if it is set, the next byte is also part of the integer.
/// Internally it is represented as a 4 byte array because variable size byte arrays with known size at compile time is hard.
/// And we can't use vectors because they are not const and no std.
/// So this really just saves on data send on the wire.
pub(super) struct VariableByteInteger([u8; 4]);

/// Helps to mark an integer as a non zero integer when you know at compile time that it is not zero
macro_rules! non_zero_usize {
    (0) => {
        compile_error!("0 is not a valid NonZeroUsize")
    };
    ($value:expr) => {{
        const VALUE: NonZeroUsize = {
            let Some(value) = NonZeroUsize::new($value) else {
                // Using compile_error!() would always cause a compile error.
                // So panic at const time it is
                // Formatting goes strange here
            panic!(stringify!($value is not a valid NonZeroUsize));
            };

            value
        };

        VALUE
    }};
}

impl VariableByteInteger {
    const MAX: u32 = 268_435_455;

    const fn encode(mut value: u32) -> Result<Self, VariableByteIntegerError> {
        if value > Self::MAX {
            return Err(VariableByteIntegerError::TooLarge);
        }

        // Short circuit if it fits in one byte (0-127)
        if value < 128 {
            return Ok(Self([value as u8, 0, 0, 0]));
        }

        let mut output = [0; 4];
        let mut index = 0;

        loop {
            let mut encoded_byte = value % 0b1000_0000;
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
        Ok(Self(output))
    }

    /// Returns the length of the encoded integer in bytes which is either 1, 2, 3 or 4.
    /// This helps to determine how many bytes are needed on the wire to represent this integer.
    const fn length(&self) -> NonZero<usize> {
        if (self.0[2] & 0b1000_0000) != 0 {
            return non_zero_usize!(4);
        }

        if (self.0[1] & 0b1000_0000) != 0 {
            return non_zero_usize!(3);
        }

        if (self.0[0] & 0b1000_0000) != 0 {
            return non_zero_usize!(2);
        }

        return non_zero_usize!(1);
    }
}

#[test]
fn can_encode_variable_byte_integer() {
    // Random examples from https://www.emqx.com/en/blog/mqtt-5-0-control-packets-01-connect-connack
    assert_eq!(encode_const(47), [0x2f]);
    assert_eq!(encode_const(19), [0x13]);

    // Range one byte
    assert_eq!(encode_const(0), [0b0000_0000]);
    assert_eq!(encode_const(1), [0b0000_0001]);
    assert_eq!(encode_const(127), [0b0111_1111]);
    // Range two bytes
    assert_eq!(encode_const(128), [0b1000_0000, 0b0000_0001]);
    assert_eq!(encode_const(16_383), [0b1111_1111, 0b0111_1111]);

    // Range three bytes
    assert_eq!(
        encode_const(16_384),
        [0b1000_0000, 0b1000_0000, 0b0000_0001]
    );
    assert_eq!(
        encode_const(2_097_151),
        [0b1111_1111, 0b1111_1111, 0b0111_1111]
    );
    // Range four bytes
    assert_eq!(
        encode_const(2_097_152),
        [0b1000_0000, 0b1000_0000, 0b1000_0000, 0b0000_0001]
    );
    assert_eq!(
        encode_const(268_435_455),
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
