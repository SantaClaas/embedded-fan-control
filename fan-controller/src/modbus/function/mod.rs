pub(super) mod code;
pub(crate) mod read_input_register;

pub(crate) trait Function {
    const CODE: u8;
}
