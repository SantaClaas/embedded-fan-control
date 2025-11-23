pub(super) mod code;
pub(crate) mod read_input_register;
pub(crate) mod write_holding_register;

pub(crate) use write_holding_register::WriteHoldingRegister;
pub(crate) trait Function {
    const CODE: u8;
}
