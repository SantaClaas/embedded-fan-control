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
    pub(crate) async fn connect(reader: TcpReader<'socket>, writer: TcpWriter<'socket>) -> Self {
        // let (reader, writer) = socket.split();

        Self { reader, writer }
    }
}
