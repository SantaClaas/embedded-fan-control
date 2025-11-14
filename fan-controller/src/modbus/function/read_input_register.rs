use crate::modbus::function::{self, Function};

pub(crate) struct ReadInputRegister {
    pub(crate) address: u16,
    pub(crate) number_of_registers: u16,
}

impl ReadInputRegister {
    pub(crate) fn new(address: u16, number_of_registers: u16) -> Self {
        Self {
            address,
            number_of_registers,
        }
    }
}

impl Function for ReadInputRegister {
    const CODE: u8 = function::code::READ_INPUT_REGISTER;
}
