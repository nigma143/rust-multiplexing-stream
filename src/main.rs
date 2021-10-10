use {
    async_tungstenite::{accept_async, tokio::TokioAdapter},
    futures::io::copy_buf,
    std::{env, net::SocketAddr},
    tokio::net::{TcpListener, TcpStream},
    ws_stream_tungstenite::*,
};

use std::{path::Path, sync::Arc, time::Duration};

use async_tungstenite::tungstenite::Error;
use rustls_pemfile::{certs, rsa_private_keys};
use std::io::BufReader;
use tokio::{
    io::{self, split, AsyncReadExt, AsyncWriteExt, DuplexStream},
    sync::{mpsc, Mutex},
};
use tokio_rustls::rustls::{self, Certificate, PrivateKey};
use tokio_rustls::TlsAcceptor;
use tokio_util::compat::{FuturesAsyncReadCompatExt, FuturesAsyncWriteCompatExt};

use crate::{frame::*, multiplexor::Multiplexor};

mod error;
mod frame;
mod multiplexor;
mod pipe;

fn load_certs(path: &Path) -> io::Result<Vec<Certificate>> {
    certs(&mut BufReader::new(std::fs::File::open(path)?))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid cert"))
        .map(|mut certs| certs.drain(..).map(Certificate).collect())
}

fn load_keys(path: &Path) -> io::Result<Vec<PrivateKey>> {
    rsa_private_keys(&mut BufReader::new(std::fs::File::open(path)?))
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "invalid key"))
        .map(|mut keys| keys.drain(..).map(PrivateKey).collect())
}

#[tokio::main()]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let addr: SocketAddr = "0.0.0.0:5001".to_string().parse().unwrap();
    println!("server task listening at: {}", &addr);

    let socket = TcpListener::bind(&addr).await.unwrap();

    loop {
        tokio::spawn(handle_conn(socket.accept().await));
    }
    Ok(())
}

async fn handle_conn(stream: Result<(TcpStream, SocketAddr), io::Error>) {
    let (tcp_stream, peer_addr) = match stream {
        Ok(tuple) => tuple,
        Err(e) => {
            println!("Failed TCP incoming connection: {}", e);
            return;
        }
    };

    let ws_stream = accept_async(TokioAdapter::new(tcp_stream)).await;
    let ws_stream = match ws_stream {
        Ok(ws_stream) => ws_stream,
        Err(e) => {
            println!("Failed WebSocket HandShake: {}", e);
            return;
        }
    };
    
    println!("Incoming WS connection from: {}", peer_addr);
    let ws_stream = WsStream::new(ws_stream);

    let (reader, writer) = futures::AsyncReadExt::split(ws_stream);

    let mut mux = Multiplexor::new(reader.compat(), writer.compat_write());

    let certs = load_certs(&Path::new("data/cert.pem".into())).unwrap();
    let mut keys = load_keys(&Path::new("data/key.pem")).unwrap();

    let config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, keys.remove(0))
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))
        .unwrap();
    let tls_acceptor = TlsAcceptor::from(Arc::new(config));

    let listener = TcpListener::bind("0.0.0.0:8080").await.unwrap();

    while let Ok((socket, _)) = listener.accept().await {
        let tls_acceptor = tls_acceptor.clone();
        let mut outcoming = mux.outcoming();

        let fut = async move {
            let (mut mux_reader, mut mux_writer) = outcoming.offer("ig-common").await.unwrap();

            let socket = tls_acceptor.accept(socket).await.unwrap();
            let (mut tcp_reader, mut tcp_writer) = split(socket);

            let client_to_server = async {
                let mut buf = vec![0; 2 * 1024].into_boxed_slice();
                loop {
                    let n = tcp_reader.read(&mut buf).await?;
                    if n == 0 {
                        break;
                    }
                    mux_writer.write_all(&buf[0..n]).await?;
                }
                mux_writer.shutdown().await
            };

            let server_to_client = async {
                let mut buf = vec![0; 2 * 1024].into_boxed_slice();
                loop {
                    let n = mux_reader.read(&mut buf).await?;
                    if n == 0 {
                        break;
                    }
                    tcp_writer.write_all(&buf[0..n]).await?;
                }
                tcp_writer.shutdown().await
            };

            tokio::try_join!(client_to_server, server_to_client);
        };

        tokio::spawn(fut);
    }

    tokio::time::sleep(Duration::from_secs(100)).await;
    // BufReader allows our AsyncRead to work with a bigger buffer than the default 8k.
    // This improves performance quite a bit.
    //
    /*if let Err(e) = copy_buf(BufReader::with_capacity(64_000, reader), &mut writer).await {
        println!("{:?}", e.kind())
    }*/
}
