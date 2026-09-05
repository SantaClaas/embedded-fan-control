use crate::modbus;

/// Reads a run of coils, of which only the first is asked about here.
///
/// `COUNT` is how many coils the request asks for, not how many are wanted. The relay module is
/// why the distinction exists: it is a one relay variant of an eight relay design, and its manual
/// prints only the eight wide read. It answers that frame and stays silent at any other, so the
/// count is the device's to dictate rather than the caller's to minimise — `docs/relay.md` records
/// where that was established
pub(crate) struct ReadCoils<const COUNT: u16>([u8; 8]);

impl<const COUNT: u16> ReadCoils<COUNT> {
    pub(crate) fn new(
        device_address: modbus::device::Address,
        first_coil: modbus::register::Address,
    ) -> Self {
        let first_coil = first_coil.to_be_bytes();
        let count = COUNT.to_be_bytes();
        let mut data = [
            *device_address,
            modbus::function::code::READ_COILS,
            first_coil[0],
            first_coil[1],
            count[0],
            count[1],
            // CRC set in next step
            0,
            0,
        ];

        let checksum = modbus::CRC.checksum(&data[..6]).to_be_bytes();
        data[6] = checksum[1];
        data[7] = checksum[0];
        Self(data)
    }

    pub(crate) fn device_address(&self) -> modbus::device::Address {
        self.0[0].into()
    }
}

impl<const COUNT: u16> AsRef<[u8]> for ReadCoils<COUNT> {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}
