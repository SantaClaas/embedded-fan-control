mod address {
    use core::ops::Deref;

    #[derive(Debug, Clone, Copy)]
    pub(crate) struct Address(u8);

    impl Address {
        pub(crate) const fn new(value: u8) -> Self {
            Self(value)
        }
    }

    impl Into<u8> for Address {
        fn into(self) -> u8 {
            self.0
        }
    }

    impl From<u8> for Address {
        fn from(value: u8) -> Self {
            Self(value)
        }
    }

    impl Deref for Address {
        type Target = u8;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
}

pub(crate) use address::Address;
