//! ebm-pabst [RadiCal centrifugal fans in scroll housings for residential ventilation](https://www.ebmpapst.com/us/en/campaigns/product-campaigns/centrifugal-fans/radical-with-scroll-housing.htmlhttps://www.ebmpapst.com/us/en/campaigns/product-campaigns/centrifugal-fans/radical-with-scroll-housing.html)
//! specific configuration and constants

use embassy_rp::uart;
use embassy_rp::uart::{DataBits, Parity, StopBits};

const BAUDRATE: u32 = 19200;
const DATA_BITS: DataBits = DataBits::DataBits8;
const PARITY: Parity = Parity::ParityEven;
const STOP_BITS: StopBits = StopBits::STOP1;


pub(crate) fn get_configuration() -> uart::Config {
    // I wish I could make this constant time but default isn't, there is no new and struct is non-exhaustive 😅
    let mut configuration: uart::Config = uart::Config::default();
    configuration.baudrate = BAUDRATE;
    configuration.data_bits = DATA_BITS;
    configuration.parity = PARITY;
    configuration.stop_bits = STOP_BITS;
    // Setting inverts should be a no-op as they should be false by default
    configuration
}
// pub const CONFIGURATION: uart::Config = {
//     let mut configuration: uart::Config =
//         uart::Config {
//             baudrate: 19200,
//             data_bits: DataBits::DataBits8,
//             parity: Parity::ParityEven,
//             stop_bits: StopBits::STOP1,
//             invert_cts: false,
//             invert_rts: false,
//             invert_rx: false,
//             invert_tx: false,
//         };
//     configuration
// };
//     uart::Config {
//     baudrate: 19200,
//     data_bits: DataBits::DataBits8,
//     parity: Parity::ParityEven,
//     stop_bits: StopBits::STOP1,
//     invert_cts: false,
//     invert_rts: false,
//     invert_rx: false,
//     invert_tx: false,
// };