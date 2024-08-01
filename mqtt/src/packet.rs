use std::convert::TryFrom;

pub(super) fn create_connect(client_identifier: &str, username: &str, password: &[u8]) -> Vec<u8> {
    let identifier_length = client_identifier.len() as u16;

    // Length is length of bytes not characters
    //TODO validate user name is less than u16::MAX
    let username_length = username.len() as u16;
    //TODO validate password is less than u16::MAX
    let password_length = password.len() as u16;

    // Remaining length can only be a byte
    //TODO use variable byte integer
    //TODO check length is not exceeding variable byte integer max

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
    packet.extend_from_slice(username.as_bytes());

    // Password is any binary data
    let length_bytes = password_length.to_be_bytes();
    packet.push(length_bytes[0]);
    packet.push(length_bytes[1]);
    // Password
    packet.extend_from_slice(password);

    packet
}

//TODO use NonZero<T>
pub(super) fn create_publish(topic_name: &str, payload: &[u8]) -> Vec<u8> {
    // Length is length of bytes not characters
    let topic_name_length = topic_name.len();
    let payload_length = payload.len();

    // Remaining length is variable byte integer
    // length of topic name length + topic name length + properties length
    let variable_header_length = size_of::<u16>() + topic_name_length + size_of::<u8>();
    //TODO ensure u32 is in less than variable byte integer max
    let remaining_length = variable_header_length + payload_length;
    let remaining_length_bytes = crate::variable_byte_integer::encode(remaining_length as u32);
    let fixed_header_length = 1 + remaining_length_bytes.len();
    let packet_length = fixed_header_length + remaining_length;
    let mut packet = Vec::with_capacity(packet_length);

    // Fixed header
    // Packet type PUBLISH (3) and flags
    packet.push(3 << 4);
    packet.extend_from_slice(&remaining_length_bytes);

    // Variable header
    // Topic name length
    let length_bytes: [u8; 2] = (topic_name_length as u16).to_be_bytes();
    packet.push(length_bytes[0]);
    packet.push(length_bytes[1]);
    // Topic name
    packet.extend_from_slice(topic_name.as_bytes());
    // Property length
    // No properties supported for now so set to 0
    packet.push(0);

    // Payload
    // No need to set length as it can be calculated
    packet.extend_from_slice(payload);

    packet
}

pub(super) enum QualityOfService {
    /// At most once delivery or 0
    AtMostOnceDelivery,
    /// At least once delivery or 1
    AtLeastOnceDelivery,
    /// Exactly once delivery or 2
    ExactlyOnceDelivery,
}

impl QualityOfService {
    const fn to_byte(&self) -> u8 {
        match self {
            QualityOfService::AtMostOnceDelivery => 0,
            QualityOfService::AtLeastOnceDelivery => 1,
            QualityOfService::ExactlyOnceDelivery => 2,
        }
    }
}

/// Retain handling option of the subscription options
/// Specifies whether retained messages are sent when the subscription is established.
/// Does not affect sending of retained messages after subscribe. If there are no retained messages
/// matching a topic filter, then all options act the same.
pub(super) enum RetainHandling {
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

pub(super) struct SubscriptionOptions(u8);

impl SubscriptionOptions {
    pub(super) const fn new(
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

pub(super) struct Subscription<'a> {
    topic_filter: &'a str,
    options: SubscriptionOptions,
}

impl<'a> Subscription<'a> {
    pub(super) const fn new(topic_filter: &'a str, options: SubscriptionOptions) -> Self {
        Self {
            options,
            topic_filter,
        }
    }

    const fn length(&self) -> usize {
        self.topic_filter.len() + size_of::<u8>()
    }
}

pub(super) fn create_subscribe(packet_identifier: u16, subscriptions: &[Subscription]) -> Vec<u8> {
    // Packet identifier + property length
    let variable_header_length = size_of::<u16>() + size_of::<u8>();
    // Payload contains a list of pairs of subscription options and topic name
    // Maybe size_of_val would be accurate too?
    let payload_length =
        // Length bytes for each subscription
        subscriptions.len() * size_of::<u16>() +
        subscriptions
        .iter()
        .map(|subscription| subscription.length())
        .sum::<usize>();
    let remaining_length = variable_header_length + payload_length;
    let remaining_length_bytes = crate::variable_byte_integer::encode(remaining_length as u32);
    let fixed_header_length = 1 + remaining_length_bytes.len();
    let packet_length = fixed_header_length + remaining_length;
    let mut packet = Vec::with_capacity(packet_length);

    // Subscribe packet type (8) and bits must be set to 0010
    packet.push(0b1000_0010);
    // Remaining length
    packet.extend_from_slice(&remaining_length_bytes);

    // Variable header
    // Packet Identifier
    let packet_identifier_bytes = packet_identifier.to_be_bytes();
    packet.extend_from_slice(&packet_identifier_bytes);

    // Property length
    // No properties supported for now so set to 0
    packet.push(0);

    for subscription in subscriptions {
        let topic_name_length_bytes = (subscription.topic_filter.len() as u16).to_be_bytes();
        packet.extend_from_slice(&topic_name_length_bytes);
        packet.extend_from_slice(&subscription.topic_filter.as_bytes());
        let options: u8 = subscription.options.0;
        packet.push(options);
    }

    packet
}

pub enum ConnectReasonCode {
    Success = 0x00,
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

pub struct UnkownConnectReasonCode(u8);
impl TryFrom<u8> for ConnectReasonCode {
    type Error = UnkownConnectReasonCode;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(ConnectReasonCode::Success),
            0x80 => Ok(ConnectReasonCode::UnspecifiedError),
            0x81 => Ok(ConnectReasonCode::MalformedPacket),
            0x82 => Ok(ConnectReasonCode::ProtocolError),
            0x83 => Ok(ConnectReasonCode::ImplementationSpecificError),
            0x84 => Ok(ConnectReasonCode::UnsupportedProtocolVersion),
            0x85 => Ok(ConnectReasonCode::ClientIdentifierNotValid),
            0x86 => Ok(ConnectReasonCode::BadUserNameOrPassword),
            0x87 => Ok(ConnectReasonCode::NotAuthorized),
            0x88 => Ok(ConnectReasonCode::ServerUnavailable),
            0x89 => Ok(ConnectReasonCode::ServerBusy),
            0x8A => Ok(ConnectReasonCode::Banned),
            0x8C => Ok(ConnectReasonCode::BadAuthenticationMethod),
            0x90 => Ok(ConnectReasonCode::TopicNameInvalid),
            0x95 => Ok(ConnectReasonCode::PacketTooLarge),
            0x97 => Ok(ConnectReasonCode::QuotaExceeded),
            0x99 => Ok(ConnectReasonCode::PayloadFormatInvalid),
            0x9A => Ok(ConnectReasonCode::RetainNotSupported),
            0x9B => Ok(ConnectReasonCode::QosNotSupported),
            0x9C => Ok(ConnectReasonCode::UseAnotherServer),
            0x9D => Ok(ConnectReasonCode::ServerMoved),
            0x9F => Ok(ConnectReasonCode::ConnectionRateExceeded),
            unknown => Err(UnkownConnectReasonCode(unknown)),
        }
    }
}

enum ConnectAcknowledgementProperty {}

#[derive(Debug)]
pub(super) enum PacketType {
    Connect = 1,
    ConnectAcknowledgement = 2,
    Publish = 3,
    PublishAcknowledgement = 4,
    PublishReceived = 5,
    PublishRelease = 6,
    PublishComplete = 7,
    Subscribe = 8,
    SubscribeAcknowledgement = 9,
    Unsubscribe = 10,
    UnsubscribeAcknowledgement = 11,
    PingRequest = 12,
    PingResponse = 13,
    Disconnect = 14,
    Authentication = 15,
}

#[derive(Debug)]
pub(super) struct UnknownPacketTypeError(u8);
impl TryFrom<u8> for PacketType {
    type Error = UnknownPacketTypeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value >> 4 {
            1 => Ok(PacketType::Connect),
            2 => Ok(PacketType::ConnectAcknowledgement),
            3 => Ok(PacketType::Publish),
            4 => Ok(PacketType::PublishAcknowledgement),
            5 => Ok(PacketType::PublishReceived),
            6 => Ok(PacketType::PublishRelease),
            7 => Ok(PacketType::PublishComplete),
            8 => Ok(PacketType::Subscribe),
            9 => Ok(PacketType::SubscribeAcknowledgement),
            10 => Ok(PacketType::Unsubscribe),
            11 => Ok(PacketType::UnsubscribeAcknowledgement),
            12 => Ok(PacketType::PingRequest),
            13 => Ok(PacketType::PingResponse),
            14 => Ok(PacketType::Disconnect),
            15 => Ok(PacketType::Authentication),
            unknown => Err(UnknownPacketTypeError(unknown)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        create_connect, create_publish, create_subscribe, QualityOfService, RetainHandling,
        Subscription, SubscriptionOptions,
    };

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

        #[rustfmt::skip]
        assert_eq!(
            connect_packet[13..],
            // Rust formatting is doing some wild stuff here for some reason
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

    #[test]
    fn can_create_options() {
        let subscription_options = SubscriptionOptions::new(
            QualityOfService::ExactlyOnceDelivery,
            true,
            true,
            RetainHandling::DoNotSend,
        );

        // Assert bit 0 and 1 are set for QualityOfService
        let expected: u8 = 0b0000_0010;
        let actual = subscription_options.0 & 0b0000_0010;
        assert_eq!(
            actual, expected,
            "Expected bit 0 and 1 to be set for Quality of service {expected:#010b} but was {actual:#010b}"
        );

        // Assert bit 2 is set
        let expected: u8 = 0b0000_0100;
        let actual = subscription_options.0 & 0b0000_0100;
        assert_eq!(
            actual, expected,
            "Expected bit 2 to be set for No Local but was {actual:#010b}"
        );

        // Assert bit 3 is set
        let expected: u8 = 0b0000_1000;
        let actual = subscription_options.0 & 0b0000_1000;
        assert_eq!(
            actual, expected,
            "Expected bit 3 to be set for Retain as Published but was {actual:#010b}"
        );

        // Assert bit 4 and 5 is set
        let expected: u8 = 0b0001_000;
        let actual = subscription_options.0 & 0b0000_1000;
        assert_eq!(
            actual, expected,
            "Expected bit 4 and 5 to be set for Retain Handling but was {actual:#010b}"
        );

        // Assert bit 6 and 7 are 0
        let expected: u8 = 0b0000_0000;
        let actual = subscription_options.0 & 0b1100_0000;
        assert_eq!(actual, expected);
    }

    #[test]
    fn can_correctly_set_retain_handling() {
        let subscription_options = SubscriptionOptions::new(
            QualityOfService::AtMostOnceDelivery,
            false,
            false,
            RetainHandling::SendAtSubscribe,
        );

        assert_eq!(subscription_options.0, 0);

        let subscription_options = SubscriptionOptions::new(
            QualityOfService::AtMostOnceDelivery,
            false,
            false,
            RetainHandling::DoNotSend,
        );

        // Assert bit 4 is set
        let expected: u8 = 0b0010_0000;
        // ------------------------------------------7654_3210
        let actual = subscription_options.0 & 0b0010_0000;
        assert_eq!(
            actual, expected,
            "Expected bit 5 to be set for Retain Handling but was {:#010b}",
            subscription_options.0
        );

        let subscription_options = SubscriptionOptions::new(
            QualityOfService::AtMostOnceDelivery,
            false,
            false,
            RetainHandling::OnlyIfNotExists,
        );

        let expected: u8 = 0b0001_0000;
        let actual = subscription_options.0 & 0b0001_0000;
        assert_eq!(
            actual, expected,
            "Expected bit 4 to be set for Retain Handling but was {:#010b}",
            subscription_options.0
        );
    }

    #[test]
    fn can_create_subscribe_packet() {
        // Arrange
        let subscription = Subscription::new(
            "demo",
            SubscriptionOptions::new(
                QualityOfService::ExactlyOnceDelivery,
                false,
                false,
                RetainHandling::SendAtSubscribe,
            ),
        );

        // Act
        let subscribe_packet = create_subscribe(1470, &[subscription]);

        // Assert
        // Check fixed header
        assert_eq!(subscribe_packet[..2], [(8 << 4 | 2), 0x0a]);

        // Check variable header
        assert_eq!(
            subscribe_packet[2..5],
            [0x05, 0xbe, 0x00],
            "Actual: {:#04x?}",
            &subscribe_packet[2..5]
        );

        // Check payload
        // Topic length
        assert_eq!(subscribe_packet[5..7], [0x00, 0x04]);
        // Topic filter
        assert_eq!(&subscribe_packet[7..11], b"demo");
        // Options
        let expected = 0b0000_0010;
        let actual = subscribe_packet[11];
        assert_eq!(
            expected, actual,
            "Expected {expected:#010b} actual {actual:#010b}",
        );
    }
}
