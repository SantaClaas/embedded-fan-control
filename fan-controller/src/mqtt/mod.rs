//! Module containing all the MQTT things to enable the fan controller to be integrated with Home
//! Assistant

use defmt::Format;

use crate::mqtt::variable_byte_integer::DecodeError;

pub(crate) mod client;
pub(crate) mod packet;
pub(crate) mod task;
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

#[derive(Format)]
pub(super) enum QualityOfService {
    /// At most once delivery or 0
    AtMostOnceDelivery = 0x00,
    /// At least once delivery or 1
    AtLeastOnceDelivery = 0x01,
    /// Exactly once delivery or 2
    ExactlyOnceDelivery = 0x02,
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
