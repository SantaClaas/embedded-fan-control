use crate::mqtt::Encode;
use core::convert::Infallible;
use core::pin::Pin;

pub(crate) struct PingRequest;

impl PingRequest {
    pub(crate) const TYPE: u8 = 12;
}

impl Encode for PingRequest {
    fn encode(&self, buffer: &mut [u8], offset: &mut usize) {
        buffer[*offset] = Self::TYPE << 4;
        *offset += 1;
        // Setting it to 0, because this might not be 0 if we reuse the buffer
        buffer[*offset] = 0;
    }
}
