use serialport::{DataBits, Parity, SerialPort, StopBits};
use std::time::Duration;

// This might change depending on your system. Could put this in an environment variable
const PORT_NAME: &str = "/dev/cu.usbserial-150";

fn open_serial_port() -> serialport::Result<Box<dyn SerialPort>> {
    serialport::new(PORT_NAME, 19_200)
        .timeout(Duration::from_secs(60 * 5))
        .data_bits(DataBits::Eight)
        .stop_bits(StopBits::One)
        .parity(Parity::Even)
        .open()
}

#[tokio::main]
async fn main() -> serialport::Result<()> {
    println!("Starting debug listener");

    // List available devices
    println!("\nAvailable ports:");
    for port in serialport::available_ports()? {
        println!("Port: {}", port.port_name)
    }

    // Arrange
    let mut port = open_serial_port()?;

    let mut count = 0u64;
    let is_echo = false;

    println!("\nListening on device {}", PORT_NAME);
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
        let bytes_read = port.read(&mut buffer)?;
        // let mut buffer = Vec::with_capacity(7);
        // let bytes_read = port.read_to_end(&mut buffer)?;
        println!(
            "message {count} - bytes_read: ({:?}) {:?} {:#b} {:#b} ",
            bytes_read,
            &buffer[..bytes_read],
            buffer[0],
            buffer[1]
        );

        if is_echo {
            // Wait as the fan would wait too
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            // Echo
            let result = port.write(&buffer[..bytes_read]);
            println!("Echoed message back {:?}", result);
            let result = port.flush();
            println!("Flushed message back {:?}", result);
        }
        // let inverse: Vec<u8> = buffer.iter().map(|byte| byte.to_be()).collect::<Vec<u8>>();
        // println!("message {count} - inverse: {:?}", inverse);
        count += 1;

        // tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    }
}
