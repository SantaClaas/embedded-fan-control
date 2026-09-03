#![allow(dead_code, reason = "called by the bypass routine, added next")]

use crate::modbus;

/// Reads a single coil. Modbus answers a coil read with a run of bits packed into bytes, so asking
/// for exactly one keeps the answer to a single data byte with the position in its lowest bit, the
/// same way [`super::ReadHoldingRegister`] asks for exactly one register.
/// See Alssay single-way Modbus relay module LC-Modbus-1R-D7, section 3, instruction 8
pub(crate) struct ReadCoil([u8; 8]);

impl ReadCoil {
    /// How many coils to read. The relay has eight and answers for all of them in one byte, but
    /// only the first is wired to anything, and a shorter answer is one less thing to decode
    const COUNT: u16 = 1;

    pub(crate) fn new(
        device_address: modbus::device::Address,
        coil_address: modbus::coil::Address,
    ) -> Self {
        let coil_address = coil_address.to_be_bytes();
        let count = Self::COUNT.to_be_bytes();
        let mut data = [
            *device_address,
            modbus::function::code::READ_COILS,
            coil_address[0],
            coil_address[1],
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

impl AsRef<[u8]> for ReadCoil {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}
