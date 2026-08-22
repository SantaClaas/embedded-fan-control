use crate::modbus;

/// Reads a single holding register. Modbus can read a range in one request, but the fan controller
/// only ever wants one register at a time and asking for exactly one keeps the response a fixed
/// length. See MODBUS Parameter RadiCal im Spiralgehäuse V1.00, section 1.3.1
pub(crate) struct ReadHoldingRegister([u8; 8]);

impl ReadHoldingRegister {
    /// How many registers to read, which the request carries as a count rather than a range
    const COUNT: u16 = 1;

    pub(crate) fn new(
        device_address: modbus::device::Address,
        register_address: modbus::register::Address,
    ) -> Self {
        let register_address = register_address.to_be_bytes();
        let count = Self::COUNT.to_be_bytes();
        let mut data = [
            *device_address,
            modbus::function::code::READ_HOLDING_REGISTERS,
            register_address[0],
            register_address[1],
            count[0],
            count[1],
            // CRC set in next step
            0,
            0,
        ];

        let checksum = modbus::CRC.checksum(&data[..6]).to_be_bytes();

        // They come out reversed (or is us using to_be_bytes reversed?)
        data[6] = checksum[1];
        data[7] = checksum[0];
        Self(data)
    }

    pub(crate) fn device_address(&self) -> modbus::device::Address {
        self.0[0].into()
    }
}

impl AsRef<[u8]> for ReadHoldingRegister {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}
