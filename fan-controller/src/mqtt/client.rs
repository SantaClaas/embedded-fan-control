use crate::{
    configuration,
    mqtt::{
        packet::connect::Connect,
        task::{self, SendError},
        TryEncode,
    },
};
use core::fmt::Debug;
use defmt::{info, Format};
use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};

pub(crate) struct ClientBuilder<'socket> {
    socket: TcpSocket<'socket>,
}

// impl<'socket> ClientBuilder<'socket> {
//     pub(crate) fn new(socket: TcpSocket<'socket>) -> Self {
//         Self { socket }
//     }

//     pub(crate) fn build(self) -> Client<'socket> {
//         self
//     }
// }

pub(crate) struct Client<'socket> {
    reader: TcpReader<'socket>,
    writer: TcpWriter<'socket>,
}

impl<'socket> Client<'socket> {
    async fn send<'a, T>(&mut self, packet: T) -> Result<(), SendError<<T as TryEncode>::Error>>
    where
        T: TryEncode<Error: Debug + Format>,
    {
        info!("Sending packet");
        let mut offset = 0;
        let mut send_buffer = [0; 1024];
        packet
            .try_encode(&mut send_buffer, &mut offset)
            .map_err(SendError::Encode)?;

        self.writer
            .write(&send_buffer[..offset])
            .await
            .map_err(SendError::Write)?;

        self.writer.flush().await.map_err(SendError::Flush)?;
        Ok(())
    }

    pub(crate) async fn connect(reader: TcpReader<'socket>, writer: TcpWriter<'socket>) -> Self {
        let packet = Connect {
            client_identifier: env!("CARGO_PKG_NAME"),
            username: configuration::MQTT_BROKER_CREDENTIALS.username,
            password: configuration::MQTT_BROKER_CREDENTIALS.password,
            keep_alive_seconds: configuration::KEEP_ALIVE.as_secs() as u16,
        };

        let client = Self { reader, writer };
        client
    }
}
