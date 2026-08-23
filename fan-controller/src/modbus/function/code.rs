pub const READ_HOLDING_REGISTERS: u8 = 0x03;

pub const READ_INPUT_REGISTERS: u8 = 0x04;

pub const WRITE_SINGLE_REGISTER: u8 = 0x06;

/// A device reports an error by responding with the function code of the request and this bit set
pub const EXCEPTION_MASK: u8 = 0x80;
