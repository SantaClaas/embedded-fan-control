use crate::mqtt::packet::{FromPublish, Packet};

use super::{
    connect, connect_acknowledgement::ConnectReasonCode, packet, Connect, ConnectErrorReasonCode,
};
use core::marker::PhantomData;
use embassy_net::tcp::{self, TcpReader, TcpSocket, TcpWriter};
use embassy_time::{with_timeout, Duration, TimeoutError};

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

pub(crate) struct MqttClient<'a, S> {
    _state: PhantomData<S>,
    socket: TcpSocket<'a>,
    // No experience how big this buffer should be
    send_buffer: [u8; 256],
    receive_buffer: [u8; 1024],
    timeout: Duration,
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

impl<'a> MqttClient<'a, NotConnected> {
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
    pub(crate) async fn connect<T>(
        mut self,
        username: &str,
        password: &[u8],
        keep_alive: Duration,
    ) -> Result<MqttClient<'a, Connected>, (Self, ConnectError)>
    where
        T: FromPublish,
    {
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

        Ok(MqttClient::<Connected> {
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

    pub(crate) async fn receive<T>(&mut self) -> Result<Packet<T>, ReceiveError>
    where
        T: FromPublish,
    {
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
    use crate::mqtt::connect_acknowledgement::{ConnectAcknowledgement, ConnectReasonCode};
    use crate::mqtt::packet::{FromPublish, Packet};
    use crate::mqtt::{connect, packet, subscribe, ConnectErrorReasonCode};
    use defmt::{warn, Format};
    use embassy_futures::select::{select, Either};
    use embassy_net::tcp;
    use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};
    use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
    use embassy_sync::channel::{Channel, Receiver, Sender};
    use embassy_time::{with_deadline, with_timeout, Duration, Instant, TimeoutError};
    use crate::mqtt::client::Connected;

    mod message {
        use crate::mqtt::connect_acknowledgement::ConnectReasonCode;
        use crate::mqtt::packet::{FromPublish, Packet};

        pub(super) enum Outgoing<'a> {
            Connect {
                client_identifier: &'a str,
                username: &'a str,
                password: &'a [u8],
            },
        }

        pub(super) enum Incoming<T>
        where
            T: FromPublish,
        {
            ConnectAcknowledgement(ConnectReasonCode),
            Publish(T),
        }
    }

    enum Response {
        ConnectAcknowledgement {
            connect_reason_code: ConnectReasonCode,
        },
    }

    struct State<'ch, T>
    where
        T: FromPublish,
    {
        incoming: Channel<CriticalSectionRawMutex, message::Incoming<T>, 8>,
        outgoing: Channel<CriticalSectionRawMutex, message::Outgoing<'ch>, 8>,
    }

    #[derive(Clone, Debug, Format)]
    enum ReceiveError {
        ReadError(ReadError),
        DecodePacketError(packet::ReadError),
    }

    struct MqttRunner<'a, T>
    where
        T: FromPublish,
    {
        tcp_receiver: TcpReader<'a>,
        /// Subscribe to messages to be sent from the client (like an actor handle)
        outgoing: Receiver<'a, CriticalSectionRawMutex, message::Outgoing<'a>, 8>,
        incoming: Sender<'a, CriticalSectionRawMutex, message::Incoming<T>, 8>,
        receive_buffer: [u8; 1024],
        send_buffer: [u8; 256],
        timeout: Duration,
        tcp_writer: TcpWriter<'a>,
    }

    impl<'a, T> MqttRunner<'a, T>
    where
        T: FromPublish,
    {
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
                let result = select(
                    self.outgoing.receive(),
                    self.tcp_receiver.read(&mut self.receive_buffer),
                )
                .await;
                match result {
                    Either::First(message) => {
                        match message {
                            message::Outgoing::Connect {
                                client_identifier,
                                username,
                                password,
                            } => {
                                let packet = Connect {
                                    client_identifier,
                                    username,
                                    password,
                                    //TODO validate seconds < u16::MAX
                                    keep_alive_seconds: self.timeout.as_secs() as u16,
                                };

                                let mut offset = 0;
                                if let Err(error) = packet.write(&mut self.send_buffer, &mut offset)
                                {
                                    //TODO handle error
                                    warn!("Error encoding connect packet: {:?}", error);
                                    continue;
                                };

                                if let Err(error) =
                                    self.tcp_writer.write(&self.send_buffer[..offset]).await
                                {
                                    //TODO handle error
                                    warn!("Error writing connect packet: {:?}", error);
                                    continue;
                                }

                                if let Err(error) = self.tcp_writer.flush().await {
                                    //TODO handle error
                                    warn!("Error flushing connect packet: {:?}", error);
                                    continue;
                                }
                            }
                        }
                    }
                    Either::Second(result) => {
                        let bytes_read = match result {
                            Ok(bytes_read) => bytes_read,
                            Err(error) => {
                                //TODO handle error
                                warn!("Error reading from TCP: {:?}", error);
                                continue;
                            }
                        };

                        let result = Packet::<T>::read(&self.receive_buffer[..bytes_read]);
                        let packet = match result {
                            Ok(packet) => packet,
                            Err(error) => {
                                //TODO handle error
                                warn!("Error decoding packet: {:?}", error);
                                continue;
                            }
                        };

                        match packet {
                            Packet::ConnectAcknowledgement(ConnectAcknowledgement {
                                connect_reason_code,
                                is_session_present: _,
                            }) => {
                                let message =
                                    message::Incoming::ConnectAcknowledgement(connect_reason_code);
                                self.incoming.send(message).await;
                                continue;
                            }

                            //TODO
                            Packet::SubscribeAcknowledgement(_) => {}
                            Packet::Publish(_) => {}
                        }
                    }
                }
            }
        }
    }

    #[derive(Debug, Clone, Format)]
    pub(crate) enum ReadError {
        TcpReadError(tcp::Error),
        Timeout(TimeoutError),
    }

    #[derive(Debug)]
    pub(crate) enum ConnectError {
        WriteConnectError(connect::WriteError),
        TcpWriteError(tcp::Error),
        TcpFlushError(tcp::Error),
        ReceiveError(ReceiveError),
        /// Received a connect acknowledgement with an error code
        ErrorCode(ConnectErrorReasonCode),
        Timeout(TimeoutError),
    }

    pub(crate) enum SubscribeError {
        WriteSubscribeError(subscribe::WriteError),
        TcpWriteError(tcp::Error),
        TcpFlushError(tcp::Error),
        Timeout(TimeoutError),
    }

    struct MqttClient<'a, T>
    where
        T: FromPublish,
    {
        timeout: Duration,
        /// Publisher for sending messages to the runner to execute
        outgoing: Sender<'a, CriticalSectionRawMutex, message::Outgoing<'a>, 8>,
        incoming: Receiver<'a, CriticalSectionRawMutex, message::Incoming<T>, 8>,
    }

    impl<'a, T> MqttClient<'a, T>
    where
        T: FromPublish,
    {
        pub fn new(
            state: &'a State<'a, T>,
            mut socket: &'a mut TcpSocket<'a>,
        ) -> (MqttRunner<'a, T>, Self) {
            let (reader, writer) = socket.split();
            //TODO handle out of subscribers/publishers error
            let send_incoming = state.incoming.sender();
            let receive_incoming = state.incoming.receiver();
            let send_outgoing = state.outgoing.sender();
            let receive_outgoing = state.outgoing.receiver();

            let timeout = Duration::from_secs(5);

            (
                MqttRunner {
                    incoming: send_incoming,
                    outgoing: receive_outgoing,
                    tcp_receiver: reader,
                    receive_buffer: [0; 1024],
                    send_buffer: [0; 256],
                    timeout: timeout.clone(),
                    tcp_writer: writer,
                },
                Self {
                    timeout,
                    incoming: receive_incoming,
                    outgoing: send_outgoing,
                },
            )
        }

        pub async fn connect(
            &self,
            client_identifier: &'a str,
            username: &'a str,
            password: &'a [u8],
        ) -> Result<(), ConnectError> {
            let message = message::Outgoing::Connect {
                client_identifier,
                username,
                password,
            };

            self.outgoing.send(message).await;

            // Wait for connect acknowledgement
            // Discard all messages before the connect acknowledgement
            // The server has to send a connect acknowledgement before sending any other packet
            // TCP should ensure the order of packets (afaik), so they should not arrive out of order
            // Using a deadline because the loop could be run multiple times
            let deadline = Instant::now() + self.timeout;
            loop {
                let result = with_deadline(deadline, self.incoming.receive()).await;
                let message = match result {
                    Ok(message) => message,
                    Err(error) => {
                        //TODO handle error
                        warn!("Timed out receiving connect acknowledgement: {:?}", error);
                        return Err(ConnectError::Timeout(error));
                    }
                };
                match message {
                    message::Incoming::ConnectAcknowledgement(reason_code) => return match reason_code {
                        ConnectReasonCode::Success => Ok(()),
                        ConnectReasonCode::ErrorCode(error_code) => Err(ConnectError::ErrorCode(error_code)),
                    },
                    _other => continue,
                };
            }
        }
    }
}
