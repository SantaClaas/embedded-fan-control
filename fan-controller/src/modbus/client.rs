use defmt::{error, info, warn};
use embassy_rp::{
    Peripheral,
    gpio::{Level, Output, Pin},
    interrupt::typelevel::Binding,
    uart::{self, BufferedInterruptHandler, BufferedUart, RxPin, TxPin},
};
use embassy_time::{Duration, with_timeout};
use embedded_io_async::{Read, ReadExactError, Write};

use crate::{
    configuration,
    modbus::function::{
        ReadCoil, ReadHoldingRegister, ReadInputRegisters, WriteHoldingRegister, WriteSingleCoil,
        code, read_input_register,
    },
};

/// How sending a request to a device failed
#[derive(Debug, Clone, Copy, defmt::Format)]
pub(crate) enum SendFailure {
    /// Writing the request timed out
    Timeout,
    /// The UART failed while writing the request
    Uart,
}

/// How receiving a run of bytes failed. Which part of the frame was being received is named by the
/// caller, because it is the one that knows which read this was
#[derive(Debug, Clone, Copy, defmt::Format)]
pub(crate) enum ReceiveFailure {
    /// Waiting for the bytes timed out. This is what a silent device looks like
    Timeout,
    /// The UART failed while reading
    Uart,
    /// The line went silent partway through
    Incomplete,
}

/// The parts of a response that every function reads the same way. The header comes first because
/// it decides how long the rest of the frame is, and an exception frame is what arrives in place
/// of an answer when the device refuses
#[derive(Debug, Clone, Copy, defmt::Format)]
pub(crate) enum Part {
    /// The device address and the function code
    Header,
    /// The exception code and the checksum that follow an exception header
    Exception,
}

/// The exception codes a device can refuse a write with, whether it was to a register or to a
/// coil. They are the standard modbus ones, described the way the fan specification describes
/// them — the relay documents no exception codes at all.
/// See MODBUS Parameter RadiCal im Spiralgehäuse V1.00, section 1.3.3
#[derive(Debug, Clone, Copy, defmt::Format)]
pub(crate) enum WriteException {
    /// The address is one the device does not have. On the fan that is anything outside the
    /// D000 ... D614 range it accepts
    RegisterOutOfRange,
    /// The register could not be written, because the electronics are defective or because this
    /// password level has no write permission for it
    WriteRefused,
    /// A code the specification does not list for this function. `0x03` lands here: it means the
    /// answer would be too long, which only a read can ask for
    Unexpected(u8),
}

impl From<u8> for WriteException {
    fn from(code: u8) -> Self {
        match code {
            0x02 => Self::RegisterOutOfRange,
            0x04 => Self::WriteRefused,
            other => Self::Unexpected(other),
        }
    }
}

/// The exception codes a device can refuse a read with, whether it was of registers or of coils.
/// They are the standard modbus ones, described the way the fan specification describes them —
/// the relay documents no exception codes at all.
/// See MODBUS Parameter RadiCal im Spiralgehäuse V1.00, section 1.3.1
#[derive(Debug, Clone, Copy, defmt::Format)]
pub(crate) enum ReadException {
    /// The address is one the device does not have. On the fan that is anything outside the
    /// D000 ... D614 range it accepts
    RegisterOutOfRange,
    /// The answer would exceed the 80 byte maximum telegram length, which means more than 37 or
    /// zero registers were asked for
    ResponseTooLong,
    /// The register could not be read because the electronics are defective
    ReadRefused,
    /// A code the specification does not list for this function
    Unexpected(u8),
}

impl From<u8> for ReadException {
    fn from(code: u8) -> Self {
        match code {
            0x02 => Self::RegisterOutOfRange,
            0x03 => Self::ResponseTooLong,
            0x04 => Self::ReadRefused,
            other => Self::Unexpected(other),
        }
    }
}

/// What the device is answering with, which its header is what decides
enum Answer {
    /// The answer to the function that was requested. Its body follows the header
    Requested,
    /// The fan refused. Holds the raw exception code, because the specification lists different
    /// codes for each function, so only the caller can name it
    Exception(u8),
}

/// A failure in the part of a transaction that is the same whatever function was used: sending the
/// request, reading the header, and reading the exception frame that can arrive instead of an
/// answer. Both [`WriteError`] and [`ReadError`] carry this, and nothing in it belongs to only one
/// of them
#[derive(Debug, Clone, Copy, defmt::Format)]
pub(crate) enum ExchangeError {
    /// Sending the request failed
    Request(SendFailure),
    /// Receiving this part of the response failed
    Response(Part, ReceiveFailure),
    /// Another device answered. Holds the address it claims to come from
    DeviceAddress(u8),
    /// The answer was to a different function than the request. Holds the function code it used
    FunctionCode(u8),
    /// The checksum of the exception frame does not match its contents. Only the exception frame
    /// can fail a checksum here, because the header is two bytes and carries none of its own
    ExceptionChecksum,
}

/// Writing a holding register failed. The echo is the part only this function reads, so it is the
/// only error that can name it
#[derive(Debug, Clone, Copy, defmt::Format)]
pub(crate) enum WriteError {
    /// Failed before the echo was reached
    Exchange(ExchangeError),
    /// The fan refused the write with a modbus exception instead of performing it
    Exception(WriteException),
    /// Receiving the echoed register, value and checksum failed
    Echo(ReceiveFailure),
    /// The checksum of the echo does not match its contents
    EchoChecksum,
    /// The echo does not repeat the request. Holds the whole frame that arrived, which can be
    /// compared against the request that was logged when it was sent
    EchoMismatch([u8; WRITE_RESPONSE_LENGTH]),
}

impl From<ExchangeError> for WriteError {
    fn from(error: ExchangeError) -> Self {
        Self::Exchange(error)
    }
}

/// Reading a holding register failed. The byte count and the register contents are the part only
/// this function reads, so it is the only error that can name them
#[derive(Debug, Clone, Copy, defmt::Format)]
pub(crate) enum ReadError {
    /// Failed before the register contents were reached
    Exchange(ExchangeError),
    /// The fan refused the read with a modbus exception instead of answering it
    Exception(ReadException),
    /// Receiving the byte count, the register contents and the checksum failed
    Contents(ReceiveFailure),
    /// The checksum of the answer does not match its contents
    ContentsChecksum,
    /// The answer announced a different number of data bytes than the registers that were asked
    /// for take up. Holds the byte count it announced
    ByteCount(u8),
}

impl From<ExchangeError> for ReadError {
    fn from(error: ExchangeError) -> Self {
        Self::Exchange(error)
    }
}

/// The device address and the function code, which every response starts with and which decide
/// how long the rest of the frame is
const HEADER_LENGTH: usize = 2;

/// The fan ignores the four least significant bits of a set point and always assumes them to be
/// zero. See MODBUS Parameter RadiCal im Spiralgehäuse V1.00, section 2.3
const IGNORED_SET_POINT_BITS: u16 = 0x000F;

/// A successful response to a write holding register or write single coil request echoes the
/// request back
const WRITE_RESPONSE_LENGTH: usize = 8;

/// A successful response to a read coil request: the header, the byte count, the one byte the
/// single coil that was asked for is packed into, and the checksum
const READ_COIL_RESPONSE_LENGTH: usize = 6;

/// How many data bytes a read of the single coil asked for has to announce. Modbus packs eight
/// coils into a byte, so asking for one still comes back as a whole byte
const READ_COIL_BYTE_COUNT: u8 = 1;

/// A successful response to a read holding register request: the header, the byte count, the
/// contents of the one register that was asked for, and the checksum
const READ_RESPONSE_LENGTH: usize = 7;

/// How many data bytes a read of the single register asked for has to announce
const READ_BYTE_COUNT: u8 = 2;

/// What a read response carries around its data bytes: the header, the byte count, and the
/// checksum
const READ_OVERHEAD_LENGTH: usize = HEADER_LENGTH + 1 + 2;

/// The longest response a read of input registers can produce, which is the buffer every one of
/// them is read into. An array cannot be sized from a const generic on stable, so the buffer is
/// sized for the longest run the fan will answer and only the part that was asked for is used
const MAX_INPUT_REGISTERS_RESPONSE_LENGTH: usize =
    READ_OVERHEAD_LENGTH + 2 * read_input_register::MAX_COUNT;

/// An exception response replaces the register and value of the echo with a single exception code
const EXCEPTION_RESPONSE_LENGTH: usize = 5;

/// How long to wait for another byte while clearing the line after a failed transaction.
/// Modbus separates frames by 3.5 characters of silence which is about 2 ms at 19200 baud 8E1
const DISCARD_TIMEOUT: Duration = Duration::from_millis(5);

/// Which device on the bus an address belongs to, for the log. The fans are not the only thing on
/// it any more, so an address that is not one of them is a device this function has not been told
/// about rather than a mistake
fn device_identifier(device_address: u8) -> &'static str {
    match device_address {
        2 => "[Fan 1]",
        3 => "[Fan 2]",
        _other => "[Unknown device]",
    }
}

/// Modbus transmits the checksum low byte first, unlike the rest of the frame
fn is_checksum_valid(frame: &[u8]) -> bool {
    let (data, checksum) = frame.split_at(frame.len() - 2);
    *checksum == super::CRC.checksum(data).to_le_bytes()
}

/// Modbus messages are sent through UART to MAX485 to control fans.
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

    /// Writes a fan's set point and waits for the fan to acknowledge it
    pub(crate) async fn write_holding_register(
        &mut self,
        message: &WriteHoldingRegister,
    ) -> Result<(), WriteError> {
        let device_identifier = device_identifier(*message.device_address());

        let result = self
            .transact_write(message.as_ref(), IGNORED_SET_POINT_BITS, device_identifier)
            .await;
        self.clear_line_after(&result, device_identifier).await;

        result
    }

    /// Drives a single coil and waits for the device to acknowledge it, which is how the bypass
    /// relay is opened and closed
    pub(crate) async fn write_single_coil(
        &mut self,
        message: &WriteSingleCoil,
    ) -> Result<(), WriteError> {
        let device_identifier = device_identifier(*message.device_address());

        // Every bit of the echo has to match here, unlike a set point: a coil has two positions
        // and nothing about the value that carries them is ignored
        const IGNORED_VALUE_BITS: u16 = 0x0000;
        let result = self
            .transact_write(message.as_ref(), IGNORED_VALUE_BITS, device_identifier)
            .await;
        self.clear_line_after(&result, device_identifier).await;

        result
    }

    /// Reads back which position a coil is in, which is how the bypass relay is asked where it
    /// stands. It keeps its position while the controller resets, so this is worth asking on boot
    /// rather than assuming
    pub(crate) async fn read_coil(&mut self, message: &ReadCoil) -> Result<bool, ReadError> {
        let device_identifier = device_identifier(*message.device_address());

        let result = self.transact_read_coil(message, device_identifier).await;
        self.clear_line_after(&result, device_identifier).await;

        result
    }

    /// Reads a run of the fan's input registers, which is where it reports what it measures
    /// about itself. Read only, unlike the holding registers the set point lives in
    pub(crate) async fn read_input_registers<const COUNT: usize>(
        &mut self,
        message: &ReadInputRegisters<COUNT>,
    ) -> Result<[u16; COUNT], ReadError> {
        let device_identifier = device_identifier(*message.device_address());

        let result = self
            .transact_read_input_registers(message, device_identifier)
            .await;
        self.clear_line_after(&result, device_identifier).await;

        result
    }

    /// Reads back what a fan currently holds in one of its registers
    pub(crate) async fn read_holding_register(
        &mut self,
        message: &ReadHoldingRegister,
    ) -> Result<u16, ReadError> {
        let device_identifier = device_identifier(*message.device_address());

        let result = self.transact_read(message, device_identifier).await;
        self.clear_line_after(&result, device_identifier).await;

        result
    }

    /// A failed transaction can leave part of a frame in the receive buffer. Dropping it keeps the
    /// next transaction from reading those leftovers as its own response. Both fans share this
    /// UART, so leftovers from one would otherwise be read as an answer from the other.
    async fn clear_line_after<T, E>(&mut self, result: &Result<T, E>, device_identifier: &str) {
        if result.is_err() {
            self.discard_incoming(device_identifier).await;
        }
    }

    /// Writing a register and driving a coil are the same exchange: an eight byte request, and an
    /// echo of it back. They differ only in how much of the echoed value has to match, which is
    /// what `ignored_value_bits` names — a fan quietly drops the low bits of a set point, a coil
    /// has nothing to drop
    async fn transact_write(
        &mut self,
        request: &[u8],
        ignored_value_bits: u16,
        device_identifier: &str,
    ) -> Result<(), WriteError> {
        self.send_request(request, device_identifier).await?;

        // The response is either an echo of the request or a shorter exception frame, so the
        // address and function code are read first to find out which one is arriving. Reading
        // exactly as many bytes as the frame holds leaves nothing behind for the next transaction.
        let mut response = [0u8; WRITE_RESPONSE_LENGTH];
        if let Answer::Exception(code) = self
            .read_header(&mut response, request, device_identifier)
            .await?
        {
            return Err(WriteError::Exception(code.into()));
        }

        self.receive_exact(&mut response[HEADER_LENGTH..])
            .await
            .map_err(WriteError::Echo)?;

        if !is_checksum_valid(&response) {
            warn!(
                "{} Response failed checksum: {:?}",
                device_identifier, response
            );
            return Err(WriteError::EchoChecksum);
        }

        // The echo repeats the address and the value that were written. The address has to match
        // exactly, but the fan ignores the four least significant bits of a set point, and the
        // specification does not say whether it echoes back the bits it received or the value it
        // stored. Masking those bits on both sides accepts either without accepting a real
        // mismatch, and the checksum above still catches a corrupted frame
        let echoed_address = u16::from_be_bytes([response[2], response[3]]);
        let requested_address = u16::from_be_bytes([request[2], request[3]]);
        let echoed_value = u16::from_be_bytes([response[4], response[5]]);
        let requested_value = u16::from_be_bytes([request[4], request[5]]);

        if echoed_address != requested_address
            || echoed_value & !ignored_value_bits != requested_value & !ignored_value_bits
        {
            warn!(
                "{} Response {:?} does not echo the request {:?}",
                device_identifier, response, request
            );
            return Err(WriteError::EchoMismatch(response));
        }

        info!(
            "{} Device acknowledged the write: {:?}",
            device_identifier, response
        );

        Ok(())
    }

    /// Unlike the register reads, one coil comes back as one bit in one byte rather than as a
    /// value, so there is nothing to assemble out of the data bytes — only the lowest bit of the
    /// only byte to look at
    async fn transact_read_coil(
        &mut self,
        message: &ReadCoil,
        device_identifier: &str,
    ) -> Result<bool, ReadError> {
        let request = message.as_ref();
        self.send_request(request, device_identifier).await?;

        let mut response = [0u8; READ_COIL_RESPONSE_LENGTH];
        if let Answer::Exception(code) = self
            .read_header(&mut response, request, device_identifier)
            .await?
        {
            return Err(ReadError::Exception(code.into()));
        }

        self.receive_exact(&mut response[HEADER_LENGTH..])
            .await
            .map_err(ReadError::Contents)?;

        // Checked before the checksum for the same reason as in the register reads: a different
        // byte count means a frame of a different length was just read, so a failing checksum
        // would report something other than what actually went wrong
        if response[2] != READ_COIL_BYTE_COUNT {
            warn!(
                "{} Response announced {:?} data bytes instead of {:?}: {:?}",
                device_identifier, response[2], READ_COIL_BYTE_COUNT, response
            );
            return Err(ReadError::ByteCount(response[2]));
        }

        if !is_checksum_valid(&response) {
            warn!(
                "{} Response failed checksum: {:?}",
                device_identifier, response
            );
            return Err(ReadError::ContentsChecksum);
        }

        // Coils are packed into the byte from the lowest bit up and exactly one was asked for, so
        // it is the only bit that carries anything. The rest are padding and are ignored rather
        // than checked, because the specification does not say what a device puts there
        let is_on = response[3] & 0b0000_0001 != 0;
        info!(
            "{} Device answered the read with {:?}: {:?}",
            device_identifier, is_on, response
        );

        Ok(is_on)
    }

    async fn transact_read(
        &mut self,
        message: &ReadHoldingRegister,
        device_identifier: &str,
    ) -> Result<u16, ReadError> {
        let request = message.as_ref();
        self.send_request(request, device_identifier).await?;

        // Unlike the write, the answer does not repeat the request: it carries a byte count and
        // the register contents. Only one register was asked for, so its length is known in
        // advance and the byte count is a check rather than something to act on.
        let mut response = [0u8; READ_RESPONSE_LENGTH];
        if let Answer::Exception(code) = self
            .read_header(&mut response, request, device_identifier)
            .await?
        {
            return Err(ReadError::Exception(code.into()));
        }

        self.receive_exact(&mut response[HEADER_LENGTH..])
            .await
            .map_err(ReadError::Contents)?;

        // Checked before the checksum: a different byte count means the frame is a different
        // length than the one that was just read, so the checksum would fail for a reason that
        // does not name the actual problem
        if response[2] != READ_BYTE_COUNT {
            warn!(
                "{} Response announced {:?} data bytes instead of {:?}: {:?}",
                device_identifier, response[2], READ_BYTE_COUNT, response
            );
            return Err(ReadError::ByteCount(response[2]));
        }

        if !is_checksum_valid(&response) {
            warn!(
                "{} Response failed checksum: {:?}",
                device_identifier, response
            );
            return Err(ReadError::ContentsChecksum);
        }

        let value = u16::from_be_bytes([response[3], response[4]]);
        info!(
            "{} Fan answered the read with {:?}: {:?}",
            device_identifier, value, response
        );

        Ok(value)
    }

    async fn transact_read_input_registers<const COUNT: usize>(
        &mut self,
        message: &ReadInputRegisters<COUNT>,
        device_identifier: &str,
    ) -> Result<[u16; COUNT], ReadError> {
        let request = message.as_ref();
        self.send_request(request, device_identifier).await?;

        // Like the holding register read the answer carries a byte count and the contents rather
        // than repeating the request, so its length is known from the count that was asked for.
        // Only the part of the buffer this request can fill is read into, so the rest of a frame
        // is never left on the line for the next transaction to pick up
        let mut buffer = [0u8; MAX_INPUT_REGISTERS_RESPONSE_LENGTH];
        let response = &mut buffer[..READ_OVERHEAD_LENGTH + 2 * COUNT];

        if let Answer::Exception(code) = self
            .read_header(response, request, device_identifier)
            .await?
        {
            return Err(ReadError::Exception(code.into()));
        }

        self.receive_exact(&mut response[HEADER_LENGTH..])
            .await
            .map_err(ReadError::Contents)?;

        // Checked before the checksum for the same reason as in the single register read: a
        // different byte count means a different frame length was just read, so a failing checksum
        // would report something other than what actually went wrong.
        // The cast cannot truncate because `MAX_COUNT` registers are 74 data bytes
        let expected_byte_count = (2 * COUNT) as u8;
        if response[2] != expected_byte_count {
            warn!(
                "{} Response announced {:?} data bytes instead of {:?}: {:?}",
                device_identifier, response[2], expected_byte_count, response
            );
            return Err(ReadError::ByteCount(response[2]));
        }

        if !is_checksum_valid(response) {
            warn!(
                "{} Response failed checksum: {:?}",
                device_identifier, response
            );
            return Err(ReadError::ContentsChecksum);
        }

        let mut registers = [0u16; COUNT];
        for (index, register) in registers.iter_mut().enumerate() {
            // The data bytes start after the header and the byte count
            let offset = HEADER_LENGTH + 1 + 2 * index;
            *register = u16::from_be_bytes([response[offset], response[offset + 1]]);
        }

        info!(
            "{} Fan answered the read with {:?}: {:?}",
            device_identifier, registers, response
        );

        Ok(registers)
    }

    /// Drives the line, writes the request, and hands the line back to the fan
    async fn send_request(
        &mut self,
        request: &[u8],
        device_identifier: &str,
    ) -> Result<(), ExchangeError> {
        // Write then read
        // Set pin setting DE (driver enable) to on (high) on the MAX485 to send data
        self.driver_enable.set_high();

        info!(
            "{} Sending message to device: {:?}",
            device_identifier, request
        );
        // As ref because &[u8; 8] is not the same as &[u8]
        with_timeout(configuration::FAN_TIMEOUT, self.uart.write_all(request))
            .await
            .map_err(|_timeout| ExchangeError::Request(SendFailure::Timeout))?
            .map_err(|_error| ExchangeError::Request(SendFailure::Uart))?;

        info!("{} Request written", device_identifier);

        // Flushing only drains the software buffer, which empties as soon as the interrupt handler
        // has moved the frame into the hardware FIFO. At that point none of it has reached the wire
        let result = self.uart.blocking_flush();
        if let Err(_error) = result {
            error!("{} UART flush error", device_identifier);
        }

        // So wait for the transmitter itself to go idle. BUSY stays asserted until the FIFO has
        // drained and the last character has left the shift register, which is the only moment the
        // whole frame is actually on the line. Handing the line back before that cuts the frame off
        // mid-way and the fan drops it on the checksum, which looks exactly like a silent fan.
        // A fixed wait was here before and could not work: it has to cover the whole frame, not the
        // one character a plain Uart would have left over, and the frame length is not a constant.
        // Busy waiting rather than awaiting a timer, because yielding to the scheduler here means
        // the line is handed back late, and the fan starts answering 3,5 characters after the frame
        // ends. See MODBUS Parameter RadiCal im Spiralgehäuse V1.00, section 1.2.2
        while self.uart.busy() {}

        // Close sending data to enable receiving data
        self.driver_enable.set_low();

        Ok(())
    }

    /// Reads the device address and function code every response starts with, and the rest of the
    /// exception frame when the fan answered with one. On [`Answer::Requested`] the caller can
    /// read the rest of its own frame into the same buffer.
    ///
    /// A refusal comes back as the raw exception code rather than a named one, because the
    /// specification lists different codes for each function and this is shared between them.
    ///
    /// The buffer has to hold at least [`EXCEPTION_RESPONSE_LENGTH`] bytes
    async fn read_header(
        &mut self,
        response: &mut [u8],
        request: &[u8],
        device_identifier: &str,
    ) -> Result<Answer, ExchangeError> {
        info!("{} Waiting for response from device", device_identifier);
        self.receive_exact(&mut response[..HEADER_LENGTH])
            .await
            .map_err(|failure| ExchangeError::Response(Part::Header, failure))?;

        if response[0] != request[0] {
            warn!(
                "{} Response came from device address {:?} instead of {:?}",
                device_identifier, response[0], request[0]
            );
            return Err(ExchangeError::DeviceAddress(response[0]));
        }

        let function_code = request[1];
        if response[1] == function_code | code::EXCEPTION_MASK {
            self.receive_exact(&mut response[HEADER_LENGTH..EXCEPTION_RESPONSE_LENGTH])
                .await
                .map_err(|failure| ExchangeError::Response(Part::Exception, failure))?;

            let frame = &response[..EXCEPTION_RESPONSE_LENGTH];
            if !is_checksum_valid(frame) {
                warn!(
                    "{} Exception response failed checksum: {:?}",
                    device_identifier, frame
                );
                return Err(ExchangeError::ExceptionChecksum);
            }

            error!(
                "{} Device rejected function code {:?} with modbus exception code {:?}",
                device_identifier, function_code, response[2]
            );
            return Ok(Answer::Exception(response[2]));
        }

        if response[1] != function_code {
            warn!(
                "{} Response used function code {:?} instead of {:?}",
                device_identifier, response[1], function_code
            );
            return Err(ExchangeError::FunctionCode(response[1]));
        }

        Ok(Answer::Requested)
    }

    /// Fills the whole buffer or fails. Reading exactly the frame length avoids the short reads
    /// [`Read::read`] would allow, which would leave the rest of the frame for the next caller.
    /// Which part of the frame this was is added by the caller, which is the only one that knows
    async fn receive_exact(&mut self, buffer: &mut [u8]) -> Result<(), ReceiveFailure> {
        match with_timeout(configuration::FAN_TIMEOUT, self.uart.read_exact(buffer)).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(ReadExactError::UnexpectedEof)) => Err(ReceiveFailure::Incomplete),
            Ok(Err(ReadExactError::Other(_error))) => Err(ReceiveFailure::Uart),
            Err(_timeout) => Err(ReceiveFailure::Timeout),
        }
    }

    /// Reads until the line has been silent for [`DISCARD_TIMEOUT`] to drop a partial or
    /// unexpected frame before the next transaction starts
    async fn discard_incoming(&mut self, device_identifier: &str) {
        let mut discarded = [0u8; WRITE_RESPONSE_LENGTH];
        while let Ok(result) = with_timeout(DISCARD_TIMEOUT, self.uart.read(&mut discarded)).await {
            match result {
                Ok(0) => break,
                Ok(count) => info!(
                    "{} Discarded {:?} unexpected bytes: {:?}",
                    device_identifier,
                    count,
                    &discarded[..count]
                ),
                Err(_error) => {
                    error!("{} UART error while clearing the line", device_identifier);
                    break;
                }
            }
        }
    }
}
