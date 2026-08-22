use defmt::{error, info, warn};
use embassy_rp::{
    Peripheral,
    gpio::{Level, Output, Pin},
    interrupt::typelevel::Binding,
    uart::{self, BufferedInterruptHandler, BufferedUart, RxPin, TxPin},
};
use embassy_time::{Duration, block_for, with_timeout};
use embedded_io_async::{Read, ReadExactError, Write};

use crate::{
    configuration,
    modbus::function::{WriteHoldingRegister, code},
};

/// Which part of the response was being read when something went wrong. The parts are read one
/// after the other because the header decides how long the rest of the frame is
#[derive(Debug, Clone, Copy, defmt::Format)]
pub(crate) enum Part {
    /// The device address and the function code
    Header,
    /// The exception code and the checksum that follow an exception header
    Exception,
    /// The register, the value, and the checksum echoed back after a successful write
    Echo,
}

/// The device answered, but not with the acknowledgement that was asked for
#[derive(Debug, Clone, Copy, defmt::Format)]
pub(crate) enum InvalidResponse {
    /// The line went silent in the middle of this part of the frame
    Incomplete(Part),
    /// Another device answered. Holds the address it claims to come from
    DeviceAddress(u8),
    /// The answer was not to a write holding register request. Holds the function code it used
    FunctionCode(u8),
    /// The checksum of this part's frame does not match its contents
    Checksum(Part),
    /// The frame does not echo the request. Holds the whole frame that arrived, which can be
    /// compared against the request that was logged when it was sent
    Echo([u8; RESPONSE_LENGTH]),
}

/// The exception codes the fan documents for write single register.
/// See MODBUS Parameter RadiCal im Spiralgehäuse V1.00, section 1.3.3
#[derive(Debug, Clone, Copy, defmt::Format)]
pub(crate) enum Exception {
    /// The register address is outside the D000 ... D614 range the fan accepts
    RegisterOutOfRange,
    /// The register could not be written, either because the electronics are defective or because
    /// this password level has no write permission for it
    WriteRefused,
    /// A code the specification does not list for this function
    Unknown(u8),
}

impl From<u8> for Exception {
    fn from(code: u8) -> Self {
        match code {
            0x02 => Self::RegisterOutOfRange,
            0x04 => Self::WriteRefused,
            other => Self::Unknown(other),
        }
    }
}

#[derive(Debug, Clone, Copy, defmt::Format)]
pub(crate) enum Error {
    /// Sending the request timed out
    RequestTimeout,
    /// The UART failed while sending the request
    RequestUart,
    /// Waiting for this part of the response timed out. This is what a silent fan looks like
    ResponseTimeout(Part),
    /// The UART failed while reading this part of the response
    ResponseUart(Part),
    /// The fan answered with a modbus exception instead of performing the write
    Exception(Exception),
    InvalidResponse(InvalidResponse),
}

/// How long the MAX845 keeps driving the line after the last byte was flushed, so the end of the
/// request is not cut off. Flushing only empties the FIFO, the last character can still be in the
/// shift register.
///
/// Two bounds to stay between, both at 19200 baud 8E1 which is 11 bits or 573 µs per character:
/// - at least one character, so the last one finishes leaving the shift register
/// - less than 3,5 characters (2,0 ms), because that is the minimum pause the fan waits before it
///   starts answering. Holding the driver longer means talking over the start of its response.
///   See MODBUS Parameter RadiCal im Spiralgehäuse V1.00, section 1.2.2
///
/// This was 5 ms, which reached into the fan's answer. Timings in microseconds are not accurate
/// and this is worth re-tuning on hardware, but stay inside the two bounds above
const BLOCK_FOR: Duration = Duration::from_micros(800);

/// The device address and the function code, which every response starts with and which decide
/// how long the rest of the frame is
const HEADER_LENGTH: usize = 2;

/// The fan ignores the four least significant bits of a set point and always assumes them to be
/// zero. See MODBUS Parameter RadiCal im Spiralgehäuse V1.00, section 2.3
const IGNORED_SET_POINT_BITS: u16 = 0x000F;

/// A successful response to a write holding register request echoes the request back
const RESPONSE_LENGTH: usize = 8;

/// An exception response replaces the register and value of the echo with a single exception code
const EXCEPTION_RESPONSE_LENGTH: usize = 5;

/// How long to wait for another byte while clearing the line after a failed transaction.
/// Modbus separates frames by 3.5 characters of silence which is about 2 ms at 19200 baud 8E1
const DISCARD_TIMEOUT: Duration = Duration::from_millis(5);

/// Modbus transmits the checksum low byte first, unlike the rest of the frame
fn is_checksum_valid(frame: &[u8]) -> bool {
    let (data, checksum) = frame.split_at(frame.len() - 2);
    *checksum == super::CRC.checksum(data).to_le_bytes()
}

/// Modbus messages are sent through UART to MAX845 to control fans.
/// The pin is used to enable the DE pin to switch between reading and writing
pub(crate) struct Client<'a, UART: uart::Instance, PIN: Pin> {
    uart: BufferedUart<'a, UART>,
    driver_enable: Output<'a, PIN>,
}

impl<'a, UART: uart::Instance, PIN: Pin> Client<'a, UART, PIN> {
    pub(crate) fn new(
        uart: impl Peripheral<P = UART> + 'a,
        tx: impl Peripheral<P = impl TxPin<UART>> + 'a,
        rx: impl Peripheral<P = impl RxPin<UART>> + 'a,
        irq: impl Binding<UART::Interrupt, BufferedInterruptHandler<UART>>,
        driver_enable: impl Peripheral<P = PIN> + 'a,
        tx_buffer: &'a mut [u8],
        rx_buffer: &'a mut [u8],
        configuration: uart::Config,
    ) -> Self {
        let uart = BufferedUart::new(uart, irq, tx, rx, tx_buffer, rx_buffer, configuration);
        let driver_enable = Output::new(driver_enable, Level::Low);

        Self {
            uart,
            driver_enable,
        }
    }

    pub(crate) async fn send_3(&mut self, message: &WriteHoldingRegister) -> Result<(), Error> {
        // For debugging
        let fan_identifier = match *message.device_address() {
            2 => "[Fan 1]",
            3 => "[Fan 2]",
            _other => "Unknown (oops)",
        };

        let result = self.transact(message, fan_identifier).await;

        // A failed transaction can leave part of a frame in the receive buffer. Dropping it keeps
        // the next transaction from reading those leftovers as its own response. Both fans share
        // this UART, so leftovers from one would otherwise be read as an answer from the other.
        if result.is_err() {
            self.discard_incoming(fan_identifier).await;
        }

        result
    }

    async fn transact(
        &mut self,
        message: &WriteHoldingRegister,
        fan_identifier: &str,
    ) -> Result<(), Error> {
        // Write then read
        // Set pin setting DE (driver enable) to on (high) on the MAX845 to send data
        self.driver_enable.set_high();

        let request = message.as_ref();
        info!("{} Sending message to fan: {:?}", fan_identifier, request);
        // As ref because &[u8; 8] is not the same as &[u8]
        with_timeout(configuration::FAN_TIMEOUT, self.uart.write_all(request))
            .await
            .map_err(|_timeout| Error::RequestTimeout)?
            .map_err(|_error| Error::RequestUart)?;

        info!("{} Request written", fan_identifier);

        // Before closing we need to flush the buffer to ensure that all data is written
        // This requires blocking or we get a WouldBlock error. I don't understand why (TODO)
        let result = self.uart.blocking_flush();
        if let Err(_error) = result {
            error!("{} UART flush error", fan_identifier);
        }

        // In addition to flushing we need to wait for some time before turning off data in on the
        // MAX845 because we might be too fast and cut off the last byte or more. (This happened)
        // I saw someone using 120 microseconds (https://youtu.be/i46jdhvRej4?t=886).
        // See [BLOCK_FOR] for how long to wait and why.
        // Timer::after(Duration::from_micros(1_000)).await;
        // Using an await timer breaks this. Probably because it yields to the scheduler
        block_for(BLOCK_FOR);

        // Close sending data to enable receiving data
        self.driver_enable.set_low();

        // Read
        // The response is either an echo of the request or a shorter exception frame, so the
        // address and function code are read first to find out which one is arriving. Reading
        // exactly as many bytes as the frame holds leaves nothing behind for the next transaction.
        let mut response = [0u8; RESPONSE_LENGTH];
        info!("{} Waiting for response from fan", fan_identifier);
        self.read_exact(&mut response[..HEADER_LENGTH], Part::Header)
            .await?;

        if response[0] != request[0] {
            warn!(
                "{} Response came from device address {:?} instead of {:?}",
                fan_identifier, response[0], request[0]
            );
            return Err(Error::InvalidResponse(InvalidResponse::DeviceAddress(
                response[0],
            )));
        }

        if response[1] == code::WRITE_SINGLE_REGISTER | code::EXCEPTION_MASK {
            self.read_exact(
                &mut response[HEADER_LENGTH..EXCEPTION_RESPONSE_LENGTH],
                Part::Exception,
            )
            .await?;

            let frame = &response[..EXCEPTION_RESPONSE_LENGTH];
            if !is_checksum_valid(frame) {
                warn!(
                    "{} Exception response failed checksum: {:?}",
                    fan_identifier, frame
                );
                return Err(Error::InvalidResponse(InvalidResponse::Checksum(
                    Part::Exception,
                )));
            }

            let exception = Exception::from(response[2]);
            error!(
                "{} Fan rejected the write with modbus exception {:?}",
                fan_identifier, exception
            );
            return Err(Error::Exception(exception));
        }

        if response[1] != code::WRITE_SINGLE_REGISTER {
            warn!(
                "{} Response used function code {:?} instead of {:?}",
                fan_identifier,
                response[1],
                code::WRITE_SINGLE_REGISTER
            );
            return Err(Error::InvalidResponse(InvalidResponse::FunctionCode(
                response[1],
            )));
        }

        self.read_exact(&mut response[HEADER_LENGTH..], Part::Echo)
            .await?;

        if !is_checksum_valid(&response) {
            warn!(
                "{} Response failed checksum: {:?}",
                fan_identifier, response
            );
            return Err(Error::InvalidResponse(InvalidResponse::Checksum(
                Part::Echo,
            )));
        }

        // The echo repeats the register and the value that were written. The register has to match
        // exactly, but the fan ignores the four least significant bits of a set point, and the
        // specification does not say whether it echoes back the bits it received or the value it
        // stored. Masking those bits on both sides accepts either without accepting a real
        // mismatch, and the checksum above still catches a corrupted frame
        let echoed_register = u16::from_be_bytes([response[2], response[3]]);
        let requested_register = u16::from_be_bytes([request[2], request[3]]);
        let echoed_value = u16::from_be_bytes([response[4], response[5]]);
        let requested_value = u16::from_be_bytes([request[4], request[5]]);

        if echoed_register != requested_register
            || echoed_value & !IGNORED_SET_POINT_BITS != requested_value & !IGNORED_SET_POINT_BITS
        {
            warn!(
                "{} Response {:?} does not echo the request {:?}",
                fan_identifier, response, request
            );
            return Err(Error::InvalidResponse(InvalidResponse::Echo(response)));
        }

        info!(
            "{} Fan acknowledged the write: {:?}",
            fan_identifier, response
        );

        Ok(())
    }

    /// Fills the whole buffer or fails. Reading exactly the frame length avoids the short reads
    /// [`Read::read`] would allow, which would leave the rest of the frame for the next caller.
    /// The part is carried into the error so a failure says which read did not complete
    async fn read_exact(&mut self, buffer: &mut [u8], part: Part) -> Result<(), Error> {
        match with_timeout(configuration::FAN_TIMEOUT, self.uart.read_exact(buffer)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(ReadExactError::UnexpectedEof)) => {
                Err(Error::InvalidResponse(InvalidResponse::Incomplete(part)))
            }
            Ok(Err(ReadExactError::Other(_error))) => Err(Error::ResponseUart(part)),
            Err(_timeout) => Err(Error::ResponseTimeout(part)),
        }
    }

    /// Reads until the line has been silent for [`DISCARD_TIMEOUT`] to drop a partial or
    /// unexpected frame before the next transaction starts
    async fn discard_incoming(&mut self, fan_identifier: &str) {
        let mut discarded = [0u8; RESPONSE_LENGTH];
        while let Ok(result) = with_timeout(DISCARD_TIMEOUT, self.uart.read(&mut discarded)).await {
            match result {
                Ok(0) => break,
                Ok(count) => info!(
                    "{} Discarded {:?} unexpected bytes: {:?}",
                    fan_identifier,
                    count,
                    &discarded[..count]
                ),
                Err(_error) => {
                    error!("{} UART error while clearing the line", fan_identifier);
                    break;
                }
            }
        }
    }
}
