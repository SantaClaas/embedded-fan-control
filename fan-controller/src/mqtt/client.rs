use crate::mqtt::packet::{FromPublish, Packet};

use super::{
    connect, connect_acknowledgement::ConnectReasonCode, packet, Connect, ConnectErrorReasonCode,
};
use core::marker::PhantomData;
use embassy_net::tcp::{self, TcpReader, TcpSocket, TcpWriter};
use embassy_time::{with_timeout, Duration, TimeoutError};

pub(crate) struct MqttClient<'a, S> {
    _state: PhantomData<S>,
    socket: TcpSocket<'a>,
    // No experience how big this buffer should be
    send_buffer: [u8; 256],
    receive_buffer: [u8; 1024],
    timeout: Duration,
}
pub(crate) struct NotConnected;
pub(crate) struct Connected;

#[derive(Debug)]
pub(crate) enum ConnectError {
    WriteConnectError(connect::WriteError),
    TcpWriteError(tcp::Error),
    TcpFlushError(tcp::Error),
    ReadError(ReadError),
    DecodePacketError(packet::ReadError),
    /// Received invalid packet type. The server had to respond with a Connect Acknowledgement (CONNACK) packet but send another one instead
    InvalidPacket(u8),
    /// Received a connect acknowledgement with an error code
    ErrorCode(ConnectErrorReasonCode),
}

#[derive(Debug)]
pub(crate) enum ReadError {
    TcpReadError(tcp::Error),
    Timeout(TimeoutError),
}

impl<'a, S> MqttClient<'a, S> {
    async fn read(&mut self) -> Result<&[u8], ReadError> {
        let read = self.socket.read(&mut self.receive_buffer);
        let bytes_read = with_timeout(self.timeout, read)
            .await
            .map_err(ReadError::Timeout)?
            .map_err(ReadError::TcpReadError)?;

        Ok(&self.receive_buffer[..bytes_read])
    }
}

impl<'a, T> MqttClient<'a, NotConnected> where T : FromPublish {
    pub(crate) const fn new(socket: TcpSocket<'a>) -> Self {
        Self {
            socket,
            _state: PhantomData,
            send_buffer: [0; 256],
            receive_buffer: [0; 1024],
            timeout: Duration::from_secs(5),
        }
    }

    /// The server must not send any data before send
    pub(crate) async fn connect(
        mut self,
        username: &str,
        password: &[u8],
        keep_alive: Duration,
    ) -> Result<MqttClient<'a, Connected>, (Self, ConnectError)> {
        // Send MQTT connect packet
        let packet = Connect {
            client_identifier: "testfan",
            username,
            password,
            keep_alive_seconds: keep_alive.as_secs() as u16,
        };

        let mut offset = 0;
        if let Err(error) = packet.write(&mut self.send_buffer, &mut offset) {
            return Err((self, ConnectError::WriteConnectError(error)));
        };

        if let Err(error) = self.socket.write(&self.send_buffer[..offset]).await {
            return Err((self, ConnectError::TcpWriteError(error)));
        }

        if let Err(error) = self.socket.flush().await {
            return Err((self, ConnectError::TcpFlushError(error)));
        }

        // (if connect is accepted) The Server MUST send a CONNACK with a 0x00 (Success) Reason Code before sending any
        // Packet other than AUTH [MQTT-3.2.0-1].
        // Wait for connect acknowledgement
        let result = self.read().await;
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(error) => return Err((self, ConnectError::ReadError(error))),
        };

        let packet = match Packet::<T>::read(bytes) {
            Ok(packet) => packet,
            Err(error) => return Err((self, ConnectError::DecodePacketError(error))),
        };

        let Packet::ConnectAcknowledgement(acknowledgement) = packet else {
            let r#type = packet.get_type();
            return Err((self, ConnectError::InvalidPacket(r#type)));
        };

        if let ConnectReasonCode::ErrorCode(error_code) = acknowledgement.connect_reason_code {
            return Err((self, ConnectError::ErrorCode(error_code)));
        }

        Ok(MqttClient {
            socket: self.socket,
            _state: PhantomData,
            send_buffer: self.send_buffer,
            receive_buffer: self.receive_buffer,
            timeout: self.timeout,
        })
    }
}

impl<'a> MqttClient<'a, Connected> {
    pub(crate) fn split(self: &mut Self) -> (MqttReceiver<'_>, MqttSender<'_>) {
        let (reader, writer) = self.socket.split();
        (
            MqttReceiver {
                reader,
                receive_buffer: self.receive_buffer,
                timeout: self.timeout,
            },
            MqttSender {
                writer,
                send_buffer: self.send_buffer,
            },
        )
    }
}

pub(crate) struct MqttSender<'a> {
    send_buffer: [u8; 256],
    writer: TcpWriter<'a>,
}

#[derive(Debug)]
pub(crate) enum ReceiveError {
    ReadError(ReadError),
    DecodePacketError(packet::ReadError),
}

pub(crate) struct MqttReceiver<'a> {
    receive_buffer: [u8; 1024],
    reader: TcpReader<'a>,
    timeout: Duration,
}

impl<'a> MqttReceiver<'a> {
    async fn read(&mut self) -> Result<&[u8], ReadError> {
        let read = self.reader.read(&mut self.receive_buffer);
        let bytes_read = with_timeout(self.timeout, read)
            .await
            .map_err(ReadError::Timeout)?
            .map_err(ReadError::TcpReadError)?;

        Ok(&self.receive_buffer[..bytes_read])
    }

    pub(crate) async fn receive(&mut self) -> Result<Packet, ReceiveError> {
        let result = self.read().await;
        let bytes = match result {
            Ok(bytes) => bytes,
            Err(error) => return Err(ReceiveError::ReadError(error)),
        };

        Packet::read(bytes).map_err(ReceiveError::DecodePacketError)
    }
}

mod runner {
    use crate::mqtt::connect::Connect;
    use crate::mqtt::packet::Packet;
    use crate::mqtt::{connect, packet, ConnectErrorReasonCode};
    use embassy_net::tcp;
    use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};
    use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
    use embassy_sync::pubsub::{PubSubChannel, Publisher, Subscriber};
    use embassy_time::{with_timeout, Duration, TimeoutError};

    type ReceiveResult<'a> = Result<Packet<'a>, ReceiveError>;
    struct State<'a> {
        channel: PubSubChannel<CriticalSectionRawMutex, ReceiveResult<'a>, 8, 1, 1>,
    }

    #[derive(Clone)]
    enum ReceiveError {
        ReadError(ReadError),
        DecodePacketError(packet::ReadError),
    }

    struct MqttRunner<'a> {
        tcp_receiver: TcpReader<'a>,
        /// Publisher to send received packets to the client that waits for them
        publisher: Publisher<'a, CriticalSectionRawMutex, ReceiveResult<'a>, 8, 1, 1>,
        receive_buffer: [u8; 1024],
        timeout: Duration,
    }

    impl<'a> MqttRunner<'a> {
        pub(self) async fn read(&mut self) -> Result<&[u8], ReadError> {
            let read = self.tcp_receiver.read(&mut self.receive_buffer);
            let bytes_read = with_timeout(self.timeout, read)
                .await
                .map_err(ReadError::Timeout)?
                .map_err(ReadError::TcpReadError)?;

            Ok(&self.receive_buffer[..bytes_read])
        }

        pub(crate) async fn run(mut self) -> ! {
            loop {
                // let result: Result<&[u8], ReceiveError> = self.read().await.map_err(ReceiveError::ReadError);
                // let bytes = match result {
                //     Ok(bytes) => bytes,
                //     Err(error) => {
                //         self.publisher.publish(Err(error)).await;
                //         continue;
                //     }
                // };

                let read = self.tcp_receiver.read(&mut self.receive_buffer);
                let bytes_read = with_timeout(self.timeout, read)
                    .await.unwrap().unwrap();
                let bytes = &self.receive_buffer[..bytes_read];

                let result = Packet::read(bytes);

                match result {
                    Ok(packet) => self.publisher.publish(Ok(packet)).await,
                    Err(error) => {
                        self.publisher.publish(Err(ReceiveError::DecodePacketError(error))).await;
                        continue;
                    }
                }
            }
        }
    }

    #[derive(Debug, Clone)]
    pub(crate) enum ReadError {
        TcpReadError(tcp::Error),
        Timeout(TimeoutError),
    }

    #[derive(Debug)]
    pub(crate) enum ConnectError {
        WriteConnectError(connect::WriteError),
        TcpWriteError(tcp::Error),
        TcpFlushError(tcp::Error),
        ReadError(ReadError),
        DecodePacketError(super::ReadError),
        /// Received invalid packet type. The server had to respond with a Connect Acknowledgement (CONNACK) packet but send another one instead
        InvalidPacket(u8),
        /// Received a connect acknowledgement with an error code
        ErrorCode(ConnectErrorReasonCode),
    }

    struct MqttClient<'a> {
        subscriber: Subscriber<'a, CriticalSectionRawMutex, ReceiveResult<'a>, 8, 1, 1>,
        tcp_writer: TcpWriter<'a>,
        send_buffer: [u8; 256],
    }

    impl<'a> MqttClient<'a> {
        pub fn new(
            state: &'a State<'a>,
            mut socket: &'a mut TcpSocket<'a>,
        ) -> (MqttRunner<'a>, Self) {
            let (reader, writer) = socket.split();
            //TODO handle out of subscribers/publishers error
            let publisher = state.channel.publisher().unwrap();
            let subscriber = state.channel.subscriber().unwrap();

            (
                MqttRunner {
                    publisher,
                    tcp_receiver: reader,
                    receive_buffer: [0; 1024],
                    timeout: Duration::from_secs(5),
                },
                Self {
                    subscriber,
                    tcp_writer: writer,
                    send_buffer: [0; 256],
                },
            )
        }

        pub(crate) async fn connect(
            &mut self,
            username: &str,
            password: &[u8],
            keep_alive: Duration,
        ) -> Result<(), ConnectError> {
            // Send MQTT connect packet
            let packet = Connect {
                client_identifier: "testfan",
                username,
                password,
                keep_alive_seconds: keep_alive.as_secs() as u16,
            };

            let mut offset = 0;
            if let Err(error) = packet.write(&mut self.send_buffer, &mut offset) {
                return Err((ConnectError::WriteConnectError(error)));
            };

            if let Err(error) = self.tcp_writer.write(&self.send_buffer[..offset]).await {
                return Err((ConnectError::TcpWriteError(error)));
            }

            if let Err(error) = self.tcp_writer.flush().await {
                return Err((ConnectError::TcpFlushError(error)));
            }

            defmt::todo!()
        }
    }
}
