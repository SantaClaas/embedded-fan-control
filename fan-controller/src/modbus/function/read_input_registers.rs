use crate::modbus;

/// The most registers one request may ask for. The fan answers with at most 80 bytes and refuses
/// anything longer with exception `0x03`.
/// See MODBUS Parameter RadiCal im Spiralgehäuse V1.00, section 1.3.2
pub(crate) const MAX_COUNT: usize = 37;

/// Reads a run of input registers, which hold the values the fan measures about itself and which
/// cannot be written. Unlike [`super::ReadHoldingRegister`] this asks for a range rather than a
/// single register, because the values worth polling sit next to each other and a range costs the
/// same round trip as one register would.
///
/// `COUNT` is part of the type so the request and the array it is answered with cannot disagree
/// about how many registers are in flight.
/// See MODBUS Parameter RadiCal im Spiralgehäuse V1.00, section 1.3.2
pub(crate) struct ReadInputRegisters<const COUNT: usize>([u8; 8]);

impl<const COUNT: usize> ReadInputRegisters<COUNT> {
    pub(crate) fn new(
        device_address: modbus::device::Address,
        start_address: modbus::register::Address,
    ) -> Self {
        // The fan reports both of these as exception `0x03`, which says only that the answer would
        // be the wrong length. Catching them here names the actual mistake, at compile time
        const {
            assert!(COUNT >= 1, "a request for zero registers is refused by the fan");
            assert!(
                COUNT <= MAX_COUNT,
                "more registers than fit in the fan's 80 byte answer"
            );
        }

        let start_address = start_address.to_be_bytes();
        let count = (COUNT as u16).to_be_bytes();
        let mut data = [
            *device_address,
            modbus::function::code::READ_INPUT_REGISTERS,
            start_address[0],
            start_address[1],
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

impl<const COUNT: usize> AsRef<[u8]> for ReadInputRegisters<COUNT> {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}
