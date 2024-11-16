use core::convert::Infallible;

use crate::mqtt::Decode;

pub(crate) struct PingResponse;

impl PingResponse {
    pub(crate) const TYPE: u8 = 13;
}

impl Decode<'_> for PingResponse {
    /// Returns a ping response as ping response don't have a variable header or payload
    fn decode(_flags: u8, _variable_header_and_payload: &[u8]) -> Self
    where
        Self: Sized,
    {
        Self
    }
}
