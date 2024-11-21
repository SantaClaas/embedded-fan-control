// use core::{
//     error,
//     str::{self, FromStr},
// };

// use defmt::info;
// use embassy_rp::{
//     flash::{self, Async},
//     peripherals::{DMA_CH0, FLASH},
// };
// use embedded_storage::nor_flash::NorFlash;
// use heapless::{String, Vec};
// use thiserror::Error;

// const ADDRESS_OFFSET: u32 = 0x100000;
// const FLASH_SIZE: usize = 2 * 1024 * 1024;

// // Workaround for alignment requirements. (from embassy ekv example)
// #[repr(C, align(4))]
// struct AlignedBuffer<const N: usize>([u8; N]);

// mod layout {
//     pub(super) struct Block {
//         pub(super) capacity: usize,
//         pub(super) length: usize,
//         pub(super) start: usize,
//     }

//     /// WPA2 SSID max length is 32 bytes
//     pub(super) mod wifi {
//         /// The wifi ssid and password can not be longer than 64 bytes so we can encode their length in a byte
//         const LENGTH_BYTE: usize = size_of::<u8>();

//         pub mod ssid {
//             use super::LENGTH_BYTE;
//             pub const MAX_LENGTH: usize = 32;
//             pub const START: usize = 0;
//             pub const AREA_LENGTH: usize = LENGTH_BYTE + MAX_LENGTH;
//         }

//         pub mod password {
//             use super::LENGTH_BYTE;
//             pub const MAX_LENGTH: usize = 64;
//             pub const START: usize = super::ssid::AREA_LENGTH;
//             pub const AREA_LENGTH: usize = LENGTH_BYTE + MAX_LENGTH;
//         }
//     }
// }

// /// Abstraction over the flash storage to make common operations easeir. Based on the embassy flash example.
// pub(crate) struct Storage<'a> {
//     inner: embassy_rp::flash::Flash<'a, FLASH, Async, FLASH_SIZE>,
// }

// pub(crate) enum Error {
//     FlashError(flash::Error),
//     Utf8Error(str::Utf8Error),
// }
// impl From<flash::Error> for Error {
//     fn from(error: flash::Error) -> Self {
//         Self::FlashError(error)
//     }
// }

// impl From<str::Utf8Error> for Error {
//     fn from(error: str::Utf8Error) -> Self {
//         Self::Utf8Error(error)
//     }
// }

// impl<'a> Storage<'a> {
//     pub(crate) fn new(flash: FLASH, dma_ch0: DMA_CH0) -> Self {
//         Self {
//             inner: embassy_rp::flash::Flash::<_, Async, FLASH_SIZE>::new(flash, dma_ch0),
//         }
//     }

//     pub(crate) async fn read(&mut self) {
//         let mut buffer = [0u8; 8];
//         //TODO what is the difference between read and background_read???
//         // the example uses background_read so I am too but why????
//         defmt::unwrap!(self.inner.blocking_read(ADDRESS_OFFSET, &mut buffer));
//         info!("Read from flash: {:x}", buffer);
//     }

//     pub(crate) fn write(&mut self) {
//         let bytes = [0xcu8, 0x1, 0xa, 0xa, 0x5];
//         defmt::unwrap!(self.inner.write(ADDRESS_OFFSET, &bytes));
//     }

//     pub(crate) async fn read_wifi_ssid(
//         &mut self,
//     ) -> Result<String<{ layout::wifi::ssid::MAX_LENGTH }>, Error> {
//         let mut buffer = AlignedBuffer([0; layout::wifi::ssid::AREA_LENGTH]);
//         self.inner.read(ADDRESS_OFFSET, &mut buffer.0[..]).await?;
//         let actual_length = buffer.0[0] as usize;
//         let ssid = &buffer.0[1..actual_length];
//         // let vector = Vec::<layout::wifi::ssid::MAX_LENGTH>::from;
//         // String::from_utf8(vec)
//         let ssid = str::from_utf8(ssid)?;
//         let ssid = String::from_str(ssid)
//             .expect("SSID should not have been read into a buffer that is longer than 32 bytes");
//         Ok(ssid)
//     }
// }
//-------------------------------------------------------------------------------------
/////! Code from ekv example. I don't fully understand it
use crate::storage;
use defmt::*;
use ekv::flash::{self, PageID};
use ekv::{config, Database, FormatError, ReadError};
use embassy_executor::Spawner;
use embassy_rp::flash::Flash;
use embassy_rp::peripherals::FLASH;
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use embassy_time::Duration;
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};

use {defmt_rtt as _, panic_probe as _};

const FLASH_SIZE: usize = 2 * 1024 * 1024;
//TODO what is this
extern "C" {
    // Flash storage used for configuration
    static __config_start: u32;
}

//TODO what is this
// Workaround for alignment requirements.
#[repr(C, align(4))]
struct AlignedBuf<const N: usize>([u8; N]);

struct DbFlash<T: NorFlash + ReadNorFlash> {
    start: usize,
    flash: T,
}

impl<T: NorFlash + ReadNorFlash> flash::Flash for DbFlash<T> {
    type Error = T::Error;

    fn page_count(&self) -> usize {
        config::MAX_PAGE_COUNT
    }

    async fn erase(&mut self, page_id: PageID) -> Result<(), <DbFlash<T> as flash::Flash>::Error> {
        self.flash.erase(
            (self.start + page_id.index() * config::PAGE_SIZE) as u32,
            (self.start + page_id.index() * config::PAGE_SIZE + config::PAGE_SIZE) as u32,
        )
    }

    async fn read(
        &mut self,
        page_id: PageID,
        offset: usize,
        data: &mut [u8],
    ) -> Result<(), <DbFlash<T> as flash::Flash>::Error> {
        let address = self.start + page_id.index() * config::PAGE_SIZE + offset;
        let mut buf = AlignedBuf([0; config::PAGE_SIZE]);
        self.flash.read(address as u32, &mut buf.0[..data.len()])?;
        data.copy_from_slice(&buf.0[..data.len()]);
        Ok(())
    }

    async fn write(
        &mut self,
        page_id: PageID,
        offset: usize,
        data: &[u8],
    ) -> Result<(), <DbFlash<T> as flash::Flash>::Error> {
        let address = self.start + page_id.index() * config::PAGE_SIZE + offset;
        let mut buf = AlignedBuf([0; config::PAGE_SIZE]);
        buf.0[..data.len()].copy_from_slice(data);
        self.flash.write(address as u32, &buf.0[..data.len()])
    }
}

use core::num::NonZero;
use core::str::Utf8Error;

/// Implementers can be read from storage
trait Read<const SIZE: usize> {
    const KEY: &[u8];

    fn create(buffer: [u8; SIZE], bytes_read: usize) -> Self;
}

pub struct Ssid {
    buffer: [u8; 32],
    length: usize,
}

impl Ssid {
    pub(crate) fn try_into_string(&self) -> Result<&str, Utf8Error> {
        core::str::from_utf8(&self.buffer[..self.length])
    }
}

impl Read<32> for Ssid {
    const KEY: &'static [u8] = b"WIFI SSID";

    fn create(buffer: [u8; 32], bytes_read: usize) -> Self {
        Self {
            buffer,
            length: bytes_read,
        }
    }
}

pub(crate) struct WifiPassword {
    buffer: [u8; 64],
    length: usize,
}

impl WifiPassword {
    pub(crate) fn try_into_string(&self) -> Result<&str, Utf8Error> {
        core::str::from_utf8(&self.buffer[..self.length])
    }
}

impl Read<64> for WifiPassword {
    const KEY: &'static [u8] = b"WIFI PASSWORD";

    fn create(buffer: [u8; 64], bytes_read: usize) -> Self {
        Self {
            buffer,
            length: bytes_read,
        }
    }
}

pub(crate) struct Storage<'a> {
    database:
        Database<DbFlash<Flash<'a, FLASH, embassy_rp::flash::Blocking, 2097152>>, NoopRawMutex>,
}

impl<'a> Storage<'a> {
    pub(crate) async fn create(
        flash: FLASH,
    ) -> Result<Self, FormatError<embassy_rp::flash::Error>> {
        let flash: DbFlash<Flash<_, _, FLASH_SIZE>> = DbFlash {
            flash: Flash::new_blocking(flash),
            //TODO find out what this does 😬
            start: unsafe { &__config_start as *const u32 as usize },
        };

        let database: Database<
            DbFlash<Flash<'_, FLASH, embassy_rp::flash::Blocking, 2097152>>,
            NoopRawMutex,
        > = Database::<_, NoopRawMutex>::new(flash, ekv::Config::default());

        if database.mount().await.is_err() {
            info!("FOrmatting");
            database.format().await?;
        }

        Ok(Self { database })
    }

    pub(crate) async fn get<T: Read<SIZE>, const SIZE: usize>(
        &self,
    ) -> Result<T, ReadError<embassy_rp::flash::Error>> {
        let mut value = [0u8; SIZE];
        let bytes_read = self
            .database
            .read_transaction()
            .await
            .read(T::KEY, &mut value)
            .await?;

        Ok(T::create(value, bytes_read))
        // If we got at least an SSID we try to connect to the wifi

        // let mut write = database.write_transaction().await;
        // let result = write.write(b"WIFI SSID", b"test").await;
        // if result.is_err() {
        //     error!("Error writing");
        //     return;
        // }

        // let result = write.commit().await;
        // if result.is_err() {
        //     error!("Error committing");
        //     return;
        // }

        // info!("Committed");

        // // SSID length is max 32 bytes
        // let mut buffer = [0; 32];
        // let ssid = {
        //     let reader = database.read_transaction().await;
        //     reader
        //         .read(b"WIFI SSID", &mut buffer)
        //         .await
        //         .map(|bytes_read| &buffer[..bytes_read])
        //         .ok()
        // };

        // if let Some(ssid) = ssid {
        //     info!("Read SSID: {}", core::str::from_utf8(ssid).unwrap());
        // }

        // /// WPA2 Password is max length 64bytes
        // let mut buffer = [0u8; 64];
        // let password = {
        //     let reader = database.read_transaction().await;
        //     reader
        //         .read(b"WIFI PASSWORD", &mut buffer)
        //         .await
        //         .map(|bytes_read| &buffer[..bytes_read])
        //         .ok()
        // };

        // if let Some(password) = password {
        //     info!("Read password: {}", core::str::from_utf8(password).unwrap());
        // } else {
        //     info!("Could not read password");
        // }

        // info!("Ready");
    }
}
