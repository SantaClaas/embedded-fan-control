/// Reads the state of a run of coils. The relay module answers only the eight wide form its
/// manual prints, whatever it has — see `docs/relay.md`
pub const READ_COILS: u8 = 0x01;

pub const READ_HOLDING_REGISTERS: u8 = 0x03;

pub const READ_INPUT_REGISTERS: u8 = 0x04;

pub const WRITE_SINGLE_REGISTER: u8 = 0x06;

/// Closes or opens one coil. Like [`WRITE_SINGLE_REGISTER`] it is confirmed by the device sending
/// the request back byte for byte
pub const WRITE_SINGLE_COIL: u8 = 0x05;

/// A device reports an error by responding with the function code of the request and this bit set
pub const EXCEPTION_MASK: u8 = 0x80;
