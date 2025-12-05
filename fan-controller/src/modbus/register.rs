mod address {
    use core::ops::Deref;

    #[derive(Debug, Clone, Copy)]
    pub(crate) struct Address(u16);

    impl Address {
        pub(crate) const fn new(value: u16) -> Self {
            Self(value)
        }
    }

    impl From<Address> for u16 {
        fn from(value: Address) -> Self {
            value.0
        }
    }

    impl From<u16> for Address {
        fn from(value: u16) -> Self {
            Self(value)
        }
    }

    impl Deref for Address {
        type Target = u16;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }
}

pub(crate) use address::Address;
