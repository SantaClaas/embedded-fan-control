// Allowed until the routine that drives the bypass is the caller
#![allow(dead_code)]

use crate::modbus;

/// Drives a single coil, which is how the bypass relay is opened and closed.
/// The frame is the same shape as [`super::WriteHoldingRegister`] and the response is the same
/// echo of the request, so the client reads both of them the same way.
/// See Alssay single-way Modbus relay module LC-Modbus-1R-D7, section 3, instructions 1 and 2
pub(crate) struct WriteSingleCoil([u8; 8]);

/// What the two positions of a coil are spelled as. Modbus does not use zero and one here, and a
/// device is free to refuse anything else
mod value {
    pub(super) const ON: u16 = 0xFF00;
    pub(super) const OFF: u16 = 0x0000;
}

impl WriteSingleCoil {
    pub(crate) fn new(
        device_address: modbus::device::Address,
        coil_address: modbus::coil::Address,
        is_on: bool,
    ) -> Self {
        let coil_address = coil_address.to_be_bytes();
        let value = if is_on { value::ON } else { value::OFF };
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

        // They come out reversed (or is us using to_be_bytes reversed?)
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
