#![no_std]
#[cfg_attr(feature = "defmt", derive(defmt::Format))]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
#[repr(u8)]
pub enum QualityOfService {
    /// At most once delivery or 0
    AtMostOnceDelivery = 0x00,
    /// At least once delivery or 1
    AtLeastOnceDelivery = 0x01,
    /// Exactly once delivery or 2
    ExactlyOnceDelivery = 0x02,
}

impl QualityOfService {
    pub const fn to_byte(&self) -> u8 {
        match self {
            QualityOfService::AtMostOnceDelivery => 0,
            QualityOfService::AtLeastOnceDelivery => 1,
            QualityOfService::ExactlyOnceDelivery => 2,
        }
    }
}
