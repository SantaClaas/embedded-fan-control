//! The Modbus relay module on the controller's second bus.
//!
//! One contact and nothing else: no speed, nothing it measures. What makes it a second bus rather
//! than another address on the fans' is its framing — it answers 8N1 and only 8N1, while the fans
//! run 8E1, and its parity is not configurable at all. `docs/relay.md` records where that was
//! established, along with what it needs for power and the greeting it sends whenever it boots.

use embassy_rp::uart::{self, DataBits, Parity, StopBits};

/// Its line settings, which are the factory defaults and cannot be brought closer to the fans':
/// the baud rate is settable, the parity is not
pub(crate) const BAUD_RATE: u32 = 9_600;

pub(crate) fn get_configuration() -> uart::Config {
    let mut configuration: uart::Config = uart::Config::default();
    configuration.baudrate = BAUD_RATE;
    configuration.data_bits = DataBits::DataBits8;
    configuration.parity = Parity::ParityNone;
    configuration.stop_bits = StopBits::STOP1;
    configuration
}

pub(crate) mod address {
    use crate::modbus;

    /// The address the module ships with, and the one every frame in its manual uses. Left alone
    /// deliberately: it is the only device on this bus, so there is nothing to collide with, and
    /// re-addressing it writes a permanent change to its flash
    pub(crate) const RELAY: modbus::device::Address = modbus::device::Address::new(0xFF);
}

pub(super) mod coil {
    use crate::modbus;

    /// Relay 1. The board is a one relay variant of an eight relay design, so its manual lists
    /// 0x0000 … 0x0007 and only the first of them exists here
    pub(crate) const RELAY: modbus::register::Address = modbus::register::Address::new(0x0000_u16);

    /// How many coils a read has to ask for.
    ///
    /// Not one, although one is all there is. The manual prints only the eight wide read — the
    /// full width of the design this board is a variant of — and the module answers that frame and
    /// stays silent at any other, so asking for what exists gets nothing back. Bit 0 of the byte
    /// that comes back is this relay; the rest belong to relays the board does not have. See
    /// `docs/relay.md`
    pub(crate) const COUNT: u16 = 8;

    /// Which bit of the byte a coil read answers with is the relay
    pub(crate) const RELAY_BIT: u8 = 0b0000_0001;
}
