pub(crate) struct PingRequest;

impl PingRequest {
    pub(crate) const TYPE: u8 = 12;

    pub(crate) fn write(self, buffer: &mut [u8], offset: &mut usize) {
        buffer[*offset] = Self::TYPE << 4;
        *offset += 1;
        // This might not be 0 if we reuse the buffer
        buffer[*offset] = 0;
    }
}
