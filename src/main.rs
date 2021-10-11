use {
    async_tungstenite::{accept_async, tokio::TokioAdapter},
    std::net::SocketAddr,
    tokio::net::TcpListener,
    ws_stream_tungstenite::*,
};

use std::{path::Path, sync::Arc};

use rustls_pemfile::{certs, rsa_private_keys};
use std::io::BufReader;
use tokio::{
    io::{self, split, AsyncReadExt, AsyncWriteExt},
    sync::Mutex,
};
use tokio_rustls::rustls::{self, Certificate, PrivateKey};
use tokio_rustls::TlsAcceptor;
use tokio_util::compat::{FuturesAsyncReadCompatExt, FuturesAsyncWriteCompatExt};

use crate::multiplexor::Multiplexor;

mod error;
mod frame;
mod multiplexor;
mod pipe;

#[tokio::main()]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let outcoming = Arc::new(Mutex::new(Vec::new()));
    let outcoming_ref = outcoming.clone();

    let listen_ouncoming = tokio::spawn(async move { listen_uncoming(outcoming_ref).await });

    let listen_inncoming = tokio::spawn(async move { listen_incoming(outcoming).await });

    tokio::try_join!(listen_ouncoming, listen_inncoming).unwrap();
    Ok(())
}

async fn listen_uncoming(outcoming: Arc<Mutex<Vec<Multiplexor>>>) {
    let addr: SocketAddr = "0.0.0.0:5001".to_string().parse().unwrap();
    println!("outcoming listening at: {}", &addr);
    let listener = TcpListener::bind(&addr).await.unwrap();

    while let Ok((tcp_stream, peer_addr)) = listener.accept().await {
        let ws_stream = accept_async(TokioAdapter::new(tcp_stream)).await;
        let ws_stream = match ws_stream {
            Ok(ws_stream) => ws_stream,
            Err(e) => {
                println!("Failed WebSocket HandShake: {}", e);
                continue;
            }
        };

        println!("Incoming WS connection from: {}", peer_addr);
        let ws_stream = WsStream::new(ws_stream);

        let (reader, writer) = futures::AsyncReadExt::split(ws_stream);

        let mux = Multiplexor::new(reader.compat(), writer.compat_write());

        outcoming.lock().await.push(mux);
    }
}

async fn listen_incoming(outcoming: Arc<Mutex<Vec<Multiplexor>>>) {
    let certs = load_certs(&Path::new("data/cert.pem".into())).unwrap();
    let mut keys = load_keys(&Path::new("data/key.pem")).unwrap();

    let config = rustls::ServerConfig::builder()
        .with_safe_defaults()
        .with_no_client_auth()
        .with_single_cert(certs, keys.remove(0))
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidInput, err))
        .unwrap();
    let tls_acceptor = TlsAcceptor::from(Arc::new(config));

    let addr: SocketAddr = "0.0.0.0:8080".to_string().parse().unwrap();
    println!("outcoming listening at: {}", &addr);
    let listener = TcpListener::bind(addr).await.unwrap();

    let mut selector = 0;

    while let Ok((mut socket, _)) = listener.accept().await {
        let mut array = outcoming.lock().await;        
        if selector >= array.len() {
            selector = 0;
        }
        let mut outcoming = match array.get_mut(selector) {
            Some(mux) => mux.outcoming(),
            None => {
                socket.shutdown().await.unwrap();
                continue;
            }
        };
        selector+=1;

        let tls_acceptor = tls_acceptor.clone();

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
}

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
