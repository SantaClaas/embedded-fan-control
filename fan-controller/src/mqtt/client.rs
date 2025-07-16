use crate::{
    configuration,
    mqtt::{
        packet::{
            self,
            connect::Connect,
            connect_acknowledgement::{ConnectAcknowledgement, ConnectReasonCode},
            disconnect::Disconnect,
            ping_response::PingResponse,
            publish::Publish,
            subscribe_acknowledgement::SubscribeAcknowledgement,
        },
        task::{self, ConnectError, SendError},
        TryDecode, TryEncode,
    },
};
use core::{
    cell::RefCell,
    fmt::Debug,
    future::{poll_fn, Future},
    num::NonZeroU16,
    task::Poll,
};
use defmt::{error, info, warn, Format};
use embassy_net::tcp::{TcpReader, TcpSocket, TcpWriter};
use embassy_sync::{
    blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex, signal::Signal,
    waitqueue::AtomicWaker,
};

pub(crate) struct ClientBuilder<'socket> {
    socket: TcpSocket<'socket>,
}

/// The client state
enum State {
    Disconnected,
    Connected,
    ConnectionLost,
}

// impl<'socket> ClientBuilder<'socket> {
//     pub(crate) fn new(socket: TcpSocket<'socket>) -> Self {
//         Self { socket }
//     }

//     pub(crate) fn build(self) -> Client<'socket> {
//         self
//     }
// }

/// Manages acknowledgements for MQTT topic subscriptions.
/// Maybe it should be renamed to `SubscriptionManager`
struct Acknowledgements {
    //TODO yes static "global" state is bad, but I am still learning how to use wakers and polling
    // with futures so this will be refactored when I made it work
    /// Contains the status of the subscribe packets send out. The packet identifier represents the
    /// index in the array
    acknowledgements: Mutex<CriticalSectionRawMutex, [bool; 2]>,
    /// The waker needs to be woken to complete the subscribe acknowledgement future.
    /// The embassy documentation does not explain when to use [`AtomicWaker`] but I am assuming
    /// it is useful for cases like this where I need to mutate a static.
    waker: AtomicWaker,
}

impl Acknowledgements {
    const fn new() -> Self {
        Self {
            acknowledgements: Mutex::new([false; 2]),
            waker: AtomicWaker::new(),
        }
    }

    async fn handle_subscribe_acknowledgement<'f>(
        &self,
        acknowledgement: &'f SubscribeAcknowledgement<'f>,
    ) {
        info!("Received subscribe acknowledgement");
        let mut acknowledgements = self.acknowledgements.lock().await;
        info!("Locked ACKNOWLEDGEMENTS");
        // Validate server sends a valid packet identifier or we get bamboozled and panic
        let Some(value) = acknowledgements.get_mut(acknowledgement.packet_identifier as usize)
        else {
            warn!(
                "Received subscribe acknowledgement for out of bounds packet identifier ({})",
                acknowledgement.packet_identifier
            );
            return;
        };

        info!(
            "Received acknowledgement {} Reason codes: {:#04x}",
            acknowledgement.packet_identifier, acknowledgement.reason_codes
        );
    }

    async fn wait_for_acknowledgement(&self, packet_identifier: NonZeroU16) {
        info!("Waiting for subscribe acknowledgements");

        //TODO make this into a future?
        //TODO if this function gets called multiple times it might never be woken up because there
        // is only one waker and the other call of this function will lock the mutex. To solve this
        // we could use structs from [embassy-sync::waitqueue] and/or a blocking mutex to remove the
        // try_lock which is used because the lock function is async and we can not easily await here
        poll_fn(|context| match self.acknowledgements.try_lock() {
            Ok(mut guard) => {
                let packet = guard.get_mut(packet_identifier.get() as usize).unwrap();
                if *packet {
                    Poll::Ready(())
                } else {
                    // Waker needs to be overwritten on each poll.
                    // Read the Rust async book on wakers for more details
                    let waker = context.waker();
                    self.waker.register(waker);
                    Poll::Pending
                }
            }
            Err(_error) => Poll::Pending,
        })
        .await;

        info!("Subscribe acknowledgement received")
    }
}

//TODO can I implement the wait for acknowledgement into a future. Maybe with signal?
struct AcknowledgementFuture {
    waker: AtomicWaker,
}

impl Future for AcknowledgementFuture {
    type Output = ();

    fn poll(
        self: core::pin::Pin<&mut Self>,
        context: &mut core::task::Context<'_>,
    ) -> Poll<Self::Output> {
        todo!()
    }
}

pub(crate) struct Client<'socket> {
    reader: TcpReader<'socket>,
    writer: TcpWriter<'socket>,
    acknowledgements: Acknowledgements,
    ping_response: Signal<CriticalSectionRawMutex, PingResponse>,
    state: Signal<CriticalSectionRawMutex, State>,
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

    pub(crate) async fn connect(
        reader: TcpReader<'socket>,
        writer: TcpWriter<'socket>,
    ) -> Result<Self, ConnectError> {
        let mut client = Self {
            reader,
            writer,
            acknowledgements: Acknowledgements::new(),
            ping_response: Signal::new(),
            state: Signal::new(),
        };

        // Send
        let packet = Connect {
            client_identifier: env!("CARGO_PKG_NAME"),
            username: configuration::MQTT_BROKER_CREDENTIALS.username,
            password: configuration::MQTT_BROKER_CREDENTIALS.password,
            keep_alive_seconds: configuration::KEEP_ALIVE.as_secs() as u16,
        };

        client.send(packet).await.map_err(ConnectError::Send)?;

        // Receive
        // Wait for connect acknowledgement
        // Discard all messages before the connect acknowledgement
        // The server has to send a connect acknowledgement before sending any other packet
        let mut receive_buffer = [0; 1024];
        let bytes_read = client
            .reader
            .read(&mut receive_buffer)
            .await
            .map_err(ConnectError::Read)?;

        let parts =
            packet::get_parts(&receive_buffer[..bytes_read]).map_err(ConnectError::Parts)?;

        if parts.r#type != ConnectAcknowledgement::TYPE {
            warn!(
                "Expected connect acknowledgement packet, got: {:?}",
                parts.r#type
            );
            return Err(ConnectError::InvalidResponsePacketType(parts.r#type));
        }

        let acknowledgement =
            ConnectAcknowledgement::try_decode(parts.flags, parts.variable_header_and_payload)
                .map_err(ConnectError::DecodeAcknowledgement)?;

        info!("Connect acknowledgement read");
        if let ConnectReasonCode::ErrorCode(error_code) = acknowledgement.connect_reason_code {
            warn!("Connect error: {:?}", error_code);
            return Err(ConnectError::ErrorReasonCode(error_code));
        }

        info!("Connection complete");
        Ok(client)
    }

    async fn handle_ping_response(&self, ping_response: PingResponse) {
        info!("Received ping response");
        self.ping_response.signal(ping_response);
    }

    async fn listen(&mut self) {
        let mut buffer = [0; 1024];
        loop {
            info!("Waiting for packet");
            let result = self.reader.read(&mut buffer).await;
            let bytes_read = match result {
                // This indicates the TCP connection was closed. (See embassy-net documentation)
                Ok(0) => {
                    warn!("MQTT broker closed connection");
                    self.state.signal(State::ConnectionLost);
                    return;
                }
                Ok(bytes_read) => bytes_read,
                Err(error) => {
                    warn!("Error reading from MQTT broker: {:?}", error);
                    return;
                }
            };

            info!("Packet received");

            let parts = match packet::get_parts(&buffer[..bytes_read]) {
                Ok(parts) => parts,
                Err(error) => {
                    warn!("Error reading MQTT packet: {:?}", error);
                    return;
                }
            };

            info!("Handling packet");

            match parts.r#type {
                Publish::TYPE => {
                    info!("Received publish");
                    let publish = match Publish::try_decode(
                        parts.flags,
                        parts.variable_header_and_payload,
                    ) {
                        Ok(publish) => publish,
                        Err(error) => {
                            error!("Error reading publish: {:?}", error);
                            continue;
                        }
                    };

                    defmt::todo!("Handle publish");
                    info!("Handled publish");
                }
                SubscribeAcknowledgement::TYPE => {
                    let subscribe_acknowledgement =
                        match SubscribeAcknowledgement::read(parts.variable_header_and_payload) {
                            Ok(acknowledgement) => acknowledgement,
                            Err(error) => {
                                error!("Error reading subscribe acknowledgement: {:?}", error);
                                continue;
                            }
                        };

                    self.acknowledgements
                        .handle_subscribe_acknowledgement(&subscribe_acknowledgement)
                        .await;
                }
                PingResponse::TYPE => {
                    info!("Received ping response");
                    let ping_response = match PingResponse::try_decode(
                        parts.flags,
                        parts.variable_header_and_payload,
                    ) {
                        Ok(response) => response,
                        // Matching to get compiler error if this changes
                        Err(Infallible) => {
                            defmt::unreachable!("Ping response is always empty so decode should always succeed if the protocol did not change")
                        }
                    };

                    self.handle_ping_response(ping_response).await;
                }
                Disconnect::TYPE => {
                    info!("Received disconnect");

                    let disconnect =
                        Disconnect::try_decode(parts.flags, parts.variable_header_and_payload);
                    info!("Disconnect {:?}", disconnect);
                    //TODO disconnect TCP connection
                }
                other => info!("Unsupported packet type {}", other),
            }
        }
    }
}
