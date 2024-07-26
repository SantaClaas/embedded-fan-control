use serialport::{DataBits, Parity, SerialPort, StopBits};
use std::time::Duration;

// This might change depending on your system. Could put this in an environment variable
const PORT_NAME: &str = "/dev/cu.usbserial-2150";

fn open_serial_port() -> serialport::Result<Box<dyn SerialPort>> {
    serialport::new(PORT_NAME, 19_200)
        .timeout(Duration::from_secs(120))
        .data_bits(DataBits::Eight)
        .stop_bits(StopBits::One)
        .parity(Parity::Even)
        .open()
}

#[tokio::main]
async fn main() -> serialport::Result<()> {
    println!("Starting debug listener");
    // Arrange
    let mut port = open_serial_port()?;

    let mut count = 0u64;

    loop {
        let mut buffer = [0u8; 32];
        // We expect this length but this is a test, and it could not be guaranteed

        // let to_read = port.bytes_to_read()?;
        // println!("bytes_to_read: {:?}", to_read);
        // if to_read == 0 {
        //     tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        //     continue;
        // }
        // let mut buffer = Vec::with_capacity(to_read.try_into().unwrap());
        buffer;
        let bytes_read = port.read(&mut buffer)?;
        // let mut buffer = Vec::with_capacity(7);
        // let bytes_read = port.read_to_end(&mut buffer)?;
        println!(
            "message {count} - bytes_read: ({:?}) {:?} {:#b} {:#b} ",
            bytes_read, buffer, buffer[0], buffer[1]
        );
        // let inverse: Vec<u8> = buffer.iter().map(|byte| byte.to_be()).collect::<Vec<u8>>();
        // println!("message {count} - inverse: {:?}", inverse);
        count += 1;

        // tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
