use
{
	ws_stream_tungstenite :: { *                                         } ,
	futures               :: { AsyncReadExt, io::{ BufReader, copy_buf } } ,
	std                   :: { env, net::SocketAddr, io                  } ,
	tokio                 :: { net::{ TcpListener, TcpStream }           } ,
	async_tungstenite     :: { accept_async, tokio::TokioAdapter     } ,
};

use crate::frame::*;

mod error;
mod frame;
mod multiplexor;
mod rw;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

    let addr: SocketAddr = "127.0.0.1:3212".to_string().parse().unwrap();
	println!( "server task listening at: {}", &addr );

	let socket = TcpListener::bind(&addr).await.unwrap();
    
    loop
	{
		tokio::spawn( handle_conn( socket.accept().await ) );
	}
/*
    let (c_tx, c_rx) = mpsc::channel();
    let c_r = Reader::new(c_rx);
    let c_w = Writer::new(c_tx);

    let (s_tx, s_rx) = mpsc::channel();
    let s_r = Reader::new(s_rx);
    let s_w = Writer::new(s_tx);

    tokio::spawn(async move {
        let mut mux = Multiplexor::new(c_r, s_w);
        let (mut writer, mut reader) = mux.offer("Common").await.unwrap();

        AsyncWriteExt::write_all(&mut writer, &[1, 2, 3, 6])
            .await
            .unwrap();

        drop(writer);

        tokio::time::sleep(Duration::from_secs(100)).await;
    });

    let mut mux = Multiplexor::new(s_r, c_w);

    loop {
        let (name, mut writer, mut reader) = mux.listen().await.unwrap();
        println!("incoming stream: {}", name);

        tokio::spawn(async move {
            let mut buf = [1; 1];
            loop {
                AsyncReadExt::read_exact(&mut reader, &mut buf)
                    .await
                    .unwrap();
                println!("Server in: {:?}", buf);
            }
        });
    }

    tokio::time::sleep(Duration::from_secs(100)).await;
*/
    Ok(())
}
/*
pub struct Reader<T> {
    output: Receiver<Vec<T>>,
    output_buf: Vec<T>,
}

impl<T: Clone + Copy> Reader<T> {
    pub fn new(output: Receiver<Vec<T>>) -> Self {
        Self {
            output,
            output_buf: Vec::new(),
        }
    }

    fn read_wrap(&mut self, buf: &mut [T]) -> std::io::Result<usize> {
        if !self.output_buf.is_empty() {
            let drain_size = if self.output_buf.len() > buf.len() {
                buf.len()
            } else {
                self.output_buf.len()
            };

            let chunk: Vec<_> = self.output_buf.drain(0..drain_size).collect();
            buf[0..chunk.len()].copy_from_slice(&chunk);

            return Ok(chunk.len());
        }

        let res = self.output.recv();
        match res {
            Ok(received) => {
                if received.len() <= buf.len() {
                    buf[0..received.len()].copy_from_slice(&received);

                    Ok(received.len()) //
                } else {
                    buf.copy_from_slice(&received[..buf.len()]);
                    self.output_buf.extend(&received[buf.len()..]);

                    Ok(buf.len()) //
                }
            }
            Err(e) => Err(Error::new(ErrorKind::Other, e)),
        }
    }
}

impl Read for Reader<u8> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_wrap(buf)
    }
}

pub struct Writer<T> {
    input: Sender<Vec<T>>,
}

impl<T: Clone> Writer<T> {
    pub fn new(input: Sender<Vec<T>>) -> Self {
        Self { input }
    }
}

impl Write for Writer<u8> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.input.send(buf.to_vec()).unwrap();
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
*/
async fn handle_conn( stream: Result< (TcpStream, SocketAddr), io::Error> )
{

	// If the TCP stream fails, we stop processing this connection
	//
	let (tcp_stream, peer_addr) = match stream
	{
		Ok( tuple ) => tuple,

		Err(e) =>
		{
			println!( "Failed TCP incoming connection: {}", e );
			return;
		}
	};
       
	let s = accept_async( TokioAdapter::new(tcp_stream) ).await;

	// If the Ws handshake fails, we stop processing this connection
	//
	let socket = match s
	{
		Ok(ws) => ws,

		Err(e) =>
		{
			println!( "Failed WebSocket HandShake: {}", e );
			return;
		}
	};


	println!( "Incoming connection from: {}", peer_addr );

	let ws_stream = WsStream::new( socket );
	let (reader, mut writer) = ws_stream.split();

	// BufReader allows our AsyncRead to work with a bigger buffer than the default 8k.
	// This improves performance quite a bit.
	//
	if let Err(e) = copy_buf( BufReader::with_capacity( 64_000, reader ), &mut writer ).await
	{
		println!( "{:?}", e.kind() )
	}
}