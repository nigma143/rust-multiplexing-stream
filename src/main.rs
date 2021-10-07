use {
    async_tungstenite::{accept_async, tokio::TokioAdapter},
    futures::io::{copy_buf, BufReader},
    std::{env, net::SocketAddr},
    tokio::net::{TcpListener, TcpStream},
    ws_stream_tungstenite::*,
};

use std::time::Duration;

use tokio::{io::{self, AsyncReadExt, AsyncWriteExt}, sync::mpsc};
use tokio_util::compat::{FuturesAsyncReadCompatExt, FuturesAsyncWriteCompatExt};

use crate::{
    frame::*,
    multiplexor::Multiplexor,
};

mod error;
mod frame;
mod multiplexor;
mod pipe;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "127.0.0.1:5001".to_string().parse().unwrap();
    println!("server task listening at: {}", &addr);

    let socket = TcpListener::bind(&addr).await.unwrap();

    loop {
        tokio::spawn(handle_conn(socket.accept().await));
    }

    /*let (pipe_reader1, pipe_writer2) = pipe::new_pipe(64);
    let (pipe_reader2,pipe_writer1) = pipe::new_pipe(64);

    tokio::spawn(async move {
        //tokio::time::sleep(Duration::from_secs(2)).await;
        let mut mux = Multiplexor::new(pipe_reader1, pipe_writer1);
        let (name, mut reader, mut writer) = mux.listen().await.unwrap();
        println!("incoming stream: {}", name);

        tokio::spawn(async move {
            let mut buf = [1; 1];
            loop {
                let writer2 = &mut writer;
                AsyncReadExt::read_exact(&mut reader, &mut buf)
                    .await
                    .unwrap();
                println!("Server in: {:?}", buf);
            }
        })
        .await
        .unwrap();
    });
    tokio::time::sleep(Duration::from_secs(1)).await;

    let mut mux = Multiplexor::new(pipe_reader2, pipe_writer2);

    loop {
        let (mut reader, mut writer) = mux.offer("Common").await.unwrap();
        println!("gffdgd");
        AsyncWriteExt::write_all(&mut writer, &[1, 2, 3, 6])
            .await
            .unwrap();
        //drop(writer);

        tokio::time::sleep(Duration::from_secs(100)).await;

        let readerw = &mut reader;

        let writer23 = &mut writer;
    }

    tokio::time::sleep(Duration::from_secs(100)).await;*/

    Ok(())
}

async fn handle_conn(stream: Result<(TcpStream, SocketAddr), io::Error>) {
    // If the TCP stream fails, we stop processing this connection
    //
    let (tcp_stream, peer_addr) = match stream {
        Ok(tuple) => tuple,

        Err(e) => {
            println!("Failed TCP incoming connection: {}", e);
            return;
        }
    };

    let s = accept_async(TokioAdapter::new(tcp_stream)).await;

    // If the Ws handshake fails, we stop processing this connection
    //
    let socket = match s {
        Ok(ws) => ws,

        Err(e) => {
            println!("Failed WebSocket HandShake: {}", e);
            return;
        }
    };

    println!("Incoming connection from: {}", peer_addr);
    
    let ws_stream = WsStream::new(socket);
    let (reader, mut writer) = futures::AsyncReadExt::split(ws_stream);

    let mut mux = Multiplexor::new(reader.compat(), writer.compat_write());

    let (mut reader, mut writer) = mux.offer("ig-common").await.unwrap();
    println!("gffdgd");

    let listener = TcpListener::bind("127.0.0.1:8080").await.unwrap();

    let (mut socket, _) = listener.accept().await.unwrap();
    println!("connected");
    let (mut c_reader, mut c_writer) = socket.split();

    let mut client_to_server = async {
        io::copy(&mut c_reader, &mut writer).await
    };

    let mut server_to_client = async {
        io::copy(&mut reader, &mut c_writer).await
    };

    tokio::try_join!(client_to_server, server_to_client).unwrap();

    tokio::time::sleep(Duration::from_secs(100)).await;
    // BufReader allows our AsyncRead to work with a bigger buffer than the default 8k.
    // This improves performance quite a bit.
    //
    /*if let Err(e) = copy_buf(BufReader::with_capacity(64_000, reader), &mut writer).await {
        println!("{:?}", e.kind())
    }*/
}
