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
    use crate::mqtt::subscribe::{Subscribe, Subscription};
    use crate::mqtt::subscribe_acknowledgement::SubscribeAcknowledgement;
    use crate::mqtt::{connect, packet, subscribe, ConnectErrorReasonCode};
    use defmt::{warn, Format};
    use embassy_net::tcp;
    use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};
    use embassy_sync::blocking_mutex::raw::CriticalSectionRawMutex;
    use embassy_sync::pubsub::{PubSubChannel, Publisher, Subscriber, WaitResult};
    use embassy_time::{with_deadline, with_timeout, Duration, Instant, TimeoutError};

    type ReceiveResult<T> = Result<Packet<T>, ReceiveError>;
    struct State<T>
    where
        T: FromPublish + Clone,
    {
        channel: PubSubChannel<CriticalSectionRawMutex, ReceiveResult<T>, 8, 1, 1>,
    }

    #[derive(Clone, Debug, Format)]
    enum ReceiveError {
        ReadError(ReadError),
        DecodePacketError(packet::ReadError),
    }

    struct MqttRunner<'a, T>
    where
        T: FromPublish + Clone,
    {
        tcp_receiver: TcpReader<'a>,
        /// Publisher to send received packets to the client that waits for them
        publisher: Publisher<'a, CriticalSectionRawMutex, ReceiveResult<T>, 8, 1, 1>,
        receive_buffer: [u8; 1024],
        timeout: Duration,
    }

    impl<'a, T> MqttRunner<'a, T>
    where
        T: FromPublish + Clone,
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
                let result: Result<&[u8], ReceiveError> =
                    self.read().await.map_err(ReceiveError::ReadError);
                let bytes = match result {
                    Ok(bytes) => bytes,
                    Err(error) => {
                        self.publisher.publish(Err(error)).await;
                        continue;
                    }
                };

                let result = Packet::read(bytes);

                match result {
                    Ok(packet) => self.publisher.publish(Ok(packet)).await,
                    Err(error) => {
                        self.publisher
                            .publish(Err(ReceiveError::DecodePacketError(error)))
                            .await;
                        continue;
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
        T: FromPublish + Clone,
    {
        subscriber: Subscriber<'a, CriticalSectionRawMutex, ReceiveResult<T>, 8, 1, 1>,
        tcp_writer: TcpWriter<'a>,
        send_buffer: [u8; 256],
        timeout: Duration,
    }

    impl<'a, T> MqttClient<'a, T>
    where
        T: FromPublish + Clone,
    {
        pub fn new(
            state: &'a State<T>,
            mut socket: &'a mut TcpSocket<'a>,
        ) -> (MqttRunner<'a, T>, Self)
        where
            T: FromPublish + Clone,
        {
            let (reader, writer) = socket.split();
            //TODO handle out of subscribers/publishers error
            let publisher = state.channel.publisher().unwrap();
            let subscriber = state.channel.subscriber().unwrap();
            let timeout = Duration::from_secs(5);

            (
                MqttRunner {
                    publisher,
                    tcp_receiver: reader,
                    receive_buffer: [0; 1024],
                    timeout: timeout.clone(),
                },
                Self {
                    subscriber,
                    tcp_writer: writer,
                    send_buffer: [0; 256],
                    timeout,
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

            // Send
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

            // Receive

            // Wait for connect acknowledgement
            // Discard all messages before the connect acknowledgement
            // The server has to send a connect acknowledgement before sending any other packet
            // TCP should ensure the order of packets (afaik), so they should not arrive out of order
            // Using a deadline because the loop could be run multiple times
            let deadline = Instant::now() + self.timeout;
            loop {
                let result = with_deadline(deadline, self.subscriber.next_message()).await;

                let result = match result {
                    Ok(result) => result,
                    Err(error) => return Err(ConnectError::Timeout(error)),
                };

                match result {
                    WaitResult::Lagged(missed_messages) => {
                        warn!(
                            "Missed {} messages while waiting for connect acknowledgement: ",
                            missed_messages
                        );
                    }
                    WaitResult::Message(result) => {
                        let Packet::ConnectAcknowledgement(ConnectAcknowledgement {
                            is_session_present: _,
                            connect_reason_code,
                        }) = result.map_err(ConnectError::ReceiveError)?
                        else {
                            continue;
                        };

                        if let ConnectReasonCode::ErrorCode(error_code) = connect_reason_code {
                            return Err(ConnectError::ErrorCode(error_code));
                        };

                        return Ok(());
                    }
                }
            }
        }

        pub(crate) async fn subscribe<'s>(
            &mut self,
            subscriptions: &[Subscription<'s>],
        ) -> Result<(), SubscribeError> {
            //TODO add packet identifier management when there can be more than one subscribe in flight
            // That requires a non-mutable reference on self though. Which means putting the tcp writer behind a channel or mutex(?)
            let packet = Subscribe {
                subscriptions,
                packet_identifier: 42,
            };

            // Send
            let mut offset = 0;
            if let Err(error) = packet.write(&mut self.send_buffer, &mut offset) {
                return Err((SubscribeError::WriteSubscribeError(error)));
            };

            if let Err(error) = self.tcp_writer.write(&self.send_buffer[..offset]).await {
                return Err((SubscribeError::TcpWriteError(error)));
            }

            if let Err(error) = self.tcp_writer.flush().await {
                return Err((SubscribeError::TcpFlushError(error)));
            }

            // Receive
            let deadline = Instant::now() + self.timeout;
            loop {
                let result = with_deadline(deadline, self.subscriber.next_message()).await;

                let result = match result {
                    Ok(result) => result,
                    Err(error) => return Err(SubscribeError::Timeout(error)),
                };

                match result {
                    WaitResult::Lagged(missed_messages) => {
                        warn!(
                            "Missed {} messages while waiting for subscribe acknowledgement: ",
                            missed_messages
                        );
                    }
                    WaitResult::Message(result) => {
                        // Ignoring all errors because we don't know if they were as a result of
                        // sending the subscribe packet
                        let packet = match result {
                            Err(error) => {
                                warn!(
                                    "Error while waiting for subscribe acknowledgement: {:?}",
                                    error
                                );
                                continue;
                            }
                            Ok(packet) => packet,
                        };

                        // Ignore all messages not intended for us
                        let Packet::SubscribeAcknowledgement(SubscribeAcknowledgement {
                            //TODO how to pass arbitrary amount of subscribe topics back?
                                                             }) = packet else {
                            continue;
                        };
                    }
                }
            }

            defmt::todo!()
        }
    }
}
