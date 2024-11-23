use core::convert::Infallible;

pub(crate) trait Encode {
    fn encode(&self, buffer: &mut [u8], offset: &mut usize);
}

pub(crate) trait TryEncode {
    type Error;
    fn try_encode(&self, buffer: &mut [u8], offset: &mut usize) -> Result<(), Self::Error>;
}

impl<T: Encode> TryEncode for T {
    type Error = Infallible;

    fn try_encode(&self, buffer: &mut [u8], offset: &mut usize) -> Result<(), Self::Error> {
        self.encode(buffer, offset);
        Ok(())
    }
}

pub(crate) trait Decode<'a> {
    fn decode(flags: u8, variable_header_and_payload: &'a [u8]) -> Self
    where
        Self: Sized;
}

pub(crate) trait TryDecode<'a> {
    type Error;
    fn try_decode(flags: u8, variable_header_and_payload: &'a [u8]) -> Result<Self, Self::Error>
    where
        Self: Sized;
}

impl<'a, T: Decode<'a>> TryDecode<'a> for T {
    type Error = Infallible;

    fn try_decode(flags: u8, variable_header_and_payload: &'a [u8]) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        let value = T::decode(flags, variable_header_and_payload);
        Ok(value)
    }
}
