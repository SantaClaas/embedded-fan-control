//! ebm-pabst [RadiCal centrifugal fans in scroll housings for residential ventilation](https://www.ebmpapst.com/us/en/campaigns/product-campaigns/centrifugal-fans/radical-with-scroll-housing.html)
//! specific configuration and constants

use embassy_rp::uart;
use embassy_rp::uart::{DataBits, Parity, StopBits};


pub(crate) fn get_configuration() -> uart::Config {
    // I wish I could make this constant time but default isn't, there is no new and struct is non-exhaustive 😅
    let mut configuration: uart::Config = uart::Config::default();
    configuration.baudrate = 19_200;
    configuration.data_bits = DataBits::DataBits8;
    configuration.parity = Parity::ParityEven;
    configuration.stop_bits = StopBits::STOP1;
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