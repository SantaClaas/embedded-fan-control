use crate::modbus;

/// Originally an experiment at a simpler data type to represent a modbus message
pub(crate) struct WriteHoldingRegister([u8; 8]);

impl WriteHoldingRegister {
    pub(crate) fn new(device_address: u8, register_address: u16, value: u16) -> Self {
        let register_address = register_address.to_be_bytes();
        let mut data = [
            device_address,
            modbus::function::code::WRITE_SINGLE_REGISTER,
            register_address[0],
            register_address[1],
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
}
