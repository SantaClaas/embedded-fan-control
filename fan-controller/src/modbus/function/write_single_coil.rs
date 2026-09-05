use crate::modbus;

/// Closes or opens one coil.
///
/// The same eight byte shape as [`super::WriteHoldingRegister`], with the value carrying no
/// quantity: `0xFF00` closes the contact and `0x0000` opens it, and nothing else is allowed. The
/// device confirms by sending the request back byte for byte
pub(crate) struct WriteSingleCoil([u8; 8]);

/// The only two values a coil write may carry. Anything else is a malformed request rather than
/// an intermediate state — a coil is closed or it is open
const CLOSED: u16 = 0xFF00;
const OPEN: u16 = 0x0000;

impl WriteSingleCoil {
    pub(crate) fn new(
        device_address: modbus::device::Address,
        coil_address: modbus::register::Address,
        is_closed: bool,
    ) -> Self {
        let coil_address = coil_address.to_be_bytes();
        let value = if is_closed { CLOSED } else { OPEN };
        let mut data = [
            *device_address,
            modbus::function::code::WRITE_SINGLE_COIL,
            coil_address[0],
            coil_address[1],
            (value >> 8) as u8,
            value as u8,
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

impl AsRef<[u8]> for WriteSingleCoil {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}
