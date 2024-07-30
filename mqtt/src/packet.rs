
pub(super) fn create_connect(client_identifier: &str, username: &str, password: &[u8]) -> Vec<u8> {
    //TODO validate identifier length is between 1 and 23 bytes and contains only valid characters
    let identifier_length = client_identifier.len() as u16;

    // Length of str might not be the same as length of bytes afaik
    let username = username.as_bytes();
    //TODO validate user name is less than u16::MAX
    let username_length = username.len() as u16;
    //TODO validate password is less than u16::MAX
    let password_length = password.len() as u16;

    // Remaining length can only be a byte
    //TODO check length is not exceeding u8

    // 2 = length of length fields
    let remaining_length: u8 = (VARIABLE_HEADER.len() as u16
        + 2
        + identifier_length
        + 2
        + username_length
        + 2
        + password_length) as u8;

    // Fixed header
    let fixed_header = [
        // CONNECT package type (1) in the first four bits
        1 << 4,
        // Remaining length
        remaining_length,
    ];

    // Create variable header
    const VARIABLE_HEADER: [u8; 11] = [
        // Protocol name length
        0x00,
        0x04,
        // Protocol name
        b'M',
        b'Q',
        b'T',
        b'T',
        // Protocol version
        5,
        // Connect Flags
        // USER_NAME_FLAG | PASSWORD_FLAG | CLEAN_START
        0b1100_0010,
        // Keep alive = 60 seconds
        0x00,
        60,
        // Property length 0 (no properties). Has to be set to 0 if there are no properties
        0,
    ];

    let mut packet = Vec::with_capacity(fixed_header.len() + remaining_length as usize);
    // packet.append(fixed_header)
    packet.extend_from_slice(&fixed_header);

    // Variable header
    packet.extend_from_slice(&VARIABLE_HEADER);

    // Payload

    // Client identifier
    let length_bytes = identifier_length.to_be_bytes();
    // Client identifier length
    packet.push(length_bytes[0]);
    packet.push(length_bytes[1]);
    // Client identifier
    packet.extend_from_slice(client_identifier.as_bytes());

    // User name
    let length_bytes = username_length.to_be_bytes();
    packet.push(length_bytes[0]);
    packet.push(length_bytes[1]);

    // User name
    packet.extend_from_slice(username);

    // Password is any binary data
    let length_bytes = password_length.to_be_bytes();
    packet.push(length_bytes[0]);
    packet.push(length_bytes[1]);
    // Password
    packet.extend_from_slice(password);

    packet
}

pub(super) fn create_publish(topic_name: &str, payload: &[u8]) -> Vec<u8> {
    let mut packet = Vec::with_capacity(0);
    // Shadowing topic name to not confuse str and bytes length
    let topic_name = topic_name.as_bytes();
    let topic_name_length = topic_name.len() as u16;
    let payload_length = payload.len() as u16;

    // Fixed header
    // Packet type PUBLISH (3) and flags
    packet.push(3 << 4);
    //TODO ensure remaining length is less than max u8

    // 2 = name length
    // 1 = length property length bytes
    let remaining_length: u8 = (2 + topic_name_length + 1 + payload_length) as u8;
    packet.push(remaining_length);

    // Variable header
    // Topic name length
    let length_bytes = topic_name_length.to_be_bytes();
    packet.push(length_bytes[0]);
    packet.push(length_bytes[1]);

    // Topic name
    packet.extend_from_slice(topic_name);

    // Property length
    // No properties supported for now so set to 0
    packet.push(0);

    // Payload
    // No need to set length as it can be calculated
    packet.extend_from_slice(payload);

    packet
}

#[test]
fn can_create_publish_packet() {
    const PAYLOAD: [u8; 11] = *b"testpayload";
    // Act
    let publish_packet = create_publish("test", &PAYLOAD);

    // Assert
    // Fixed header
    assert_eq!(publish_packet[..2], [0b0011_0000, 18]);

    // Variable header
    assert_eq!(
        publish_packet[2..9],
        [
            // Topic name length
            0x00, 0x04, // Topic name
            b't', b'e', b's', b't', // Property length
            0x00,
        ]
    );

    // Payload
    assert_eq!(publish_packet[9..], PAYLOAD);
}

#[test]
fn can_create_connect_packet() {
    // Act
    let connect_packet = create_connect("mqttx_0x668d0d", "admin", b"public");
    // Assert
    // Fixed header
    assert_eq!(connect_packet[..2], [0b0001_0000, 42]);
    // Variable header
    assert_eq!(
        connect_packet[2..13],
        [
            // Protocol name length
            0x00,
            0x04,
            // Protocol name
            b'M',
            b'Q',
            b'T',
            b'T',
            // 0x4d, 0x51, 0x54, 0x54,
            // Protocol version
            5,
            // Connect Flags
            // USER_NAME_FLAG | PASSWORD_FLAG | CLEAN_START
            0b1100_0010,
            // 0xc2,
            // Keep alive 60 seconds
            0x00,
            60,
            // Property length 0 (no properties). Has to be set to 0 if there are no properties
            0,
        ]
    );

    // Payload
    assert_eq!(
        connect_packet[13..],
        // Rust formatting is doing some wild stuff here for some reason
        #[rustfmt::skip]
        [
            // Client identifier length
            0x00, 0x0e,
            // Client identifier
            b'm', b'q', b't', b't', b'x', b'_', b'0', b'x', b'6', b'6', b'8', b'd', b'0', b'd',
            // User name length
            0x00, 0x05,
            // User name
            0x61, 0x64, 0x6d, 0x69, 0x6e,
            // Password length
            0x00, 0x06,
            // Password
            b'p', b'u', b'b', b'l', b'i', b'c',
        ]
    );
}
