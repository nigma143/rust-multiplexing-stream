use std::{
    collections::HashMap,
    io::{Error, ErrorKind, Read, Write},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

use futures::{channel::oneshot, future::BoxFuture, Future, FutureExt};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream},
    sync::mpsc::{self, error::TryRecvError, Receiver, Sender},
};
use tokio_util::compat::TokioAsyncReadCompatExt;

use crate::{
    frame::{read_frame, write_frame, Frame, StreamId},
    rw::{Reader, Writer},
};

pub struct Multiplexor {
    //read_handler: JoinHandle<Result<(), Error>>,
    //write_handler: JoinHandle<Result<(), Error>>,
    incoming_r_rx: Receiver<Incoming>,
    listen_tx: Sender<(StreamId, oneshot::Sender<Outcoming>)>,

    incoming_w_tx: Sender<Incoming>,
    message_tx: Sender<Frame>,

    write_q_tx: Sender<Vec<u8>>,

    last_id: u64,
}

impl Multiplexor {
    pub fn new<R: AsyncRead + Unpin + Send + 'static, W: AsyncWrite + Send + Unpin + 'static>(
        read: R,
        mut write: W,
    ) -> Self {
        let (write_q_tx, write_q_rx) = mpsc::channel(1024);

        let (message_tx, message_rx) = mpsc::channel(1024);
        let message_tx_ref = message_tx.clone();

        let (incoming_r_tx, incoming_r_rx) = mpsc::channel(1024);
        let (listen_tx, listen_rx) = mpsc::channel(1024);
        let read_handler = tokio::spawn(async move {
            read_loop(read, message_tx_ref, incoming_r_tx, listen_rx)
                .await
                .unwrap()
        });

        let (incoming_w_tx, incoming_w_rx) = mpsc::channel(1024);

        let write_handler = tokio::spawn(async move {
            write_loop(write, message_rx, incoming_w_rx, write_q_rx)
                .await
                .unwrap()
        });

        Self {
            //write_handler,
            //read_handler,
            incoming_r_rx,
            listen_tx,
            message_tx,
            incoming_w_tx,
            write_q_tx,
            last_id: 0,
        }
    }

    pub async fn listen(&mut self) -> Result<(String, DuplexStream, DuplexStream), Error> {
        let incoming = self.incoming_r_rx.recv().await.unwrap();

        let (mut client, mut server) = tokio::io::duplex(64);

        /*self.incoming_w_tx
        .send(Incoming {
            s_id: incoming.s_id,
            name: incoming.name.clone(),
            rx: server,
            window_size: incoming.window_size,
        })
        .await
        .unwrap();*/

        self.message_tx
            .send(Frame::OfferAccepted {
                s_id: incoming.s_id,
                window_size: None,
            })
            .await
            .unwrap();

        let write_q_tx_ref = self.write_q_tx.clone();

        tokio::spawn(async move {
            let mut buffer = [0_u8; 1];
            match server.read(&mut buffer[..]).await {
                Ok(n) => {
                    write_q_tx_ref
                        .send(buffer.into_iter().take(n).map(|s| *s).collect())
                        .await
                        .unwrap();
                }
                Err(e) => {
                    println!("writer `{}` is die", 1);
                }
            }
        });

        Ok((incoming.name, client, incoming.rx))
    }

    pub async fn offer(
        &mut self,
        name: impl Into<String>,
    ) -> Result<(DuplexStream, DuplexStream), Error> {
        self.last_id += 1;
        let id = self.last_id;
        let name = name.into();

        let (outcoming_t, outcoming_r) = oneshot::channel();
        self.listen_tx.send((1, outcoming_t)).await.unwrap();

        self.message_tx
            .send(Frame::Offer {
                s_id: id,
                name: name.clone(),
                window_size: None,
            })
            .await
            .unwrap();

        let outcoming = outcoming_r.await.unwrap();

        let (mut client, mut server) = tokio::io::duplex(64);

        /*self.incoming_w_tx
        .send(Incoming {
            s_id: outcoming.s_id,
            name: name,
            rx: server,
            window_size: outcoming.window_size,
        })
        .await
        .unwrap();*/

        let write_q_tx_ref = self.write_q_tx.clone();

        tokio::spawn(async move {
            loop {
                let mut buffer = [0_u8; 1024];
                match server.read(&mut buffer[..]).await {
                    Ok(n) => {
                        write_q_tx_ref
                            .send(buffer.into_iter().take(n).map(|s| *s).collect())
                            .await
                            .unwrap();
                    }
                    Err(e) => {
                        println!("writer `{}` is die", 1);
                    }
                }
            }
        });

        Ok((client, outcoming.rx))
    }
}

#[derive(Debug)]
struct Incoming {
    s_id: StreamId,
    name: String,
    rx: DuplexStream,
    window_size: Option<u64>,
}

#[derive(Debug)]
struct Outcoming {
    s_id: StreamId,
    rx: DuplexStream,
    window_size: Option<u64>,
}

async fn read_loop<R: AsyncRead + Unpin + Send>(
    mut read: R,
    message_tx: Sender<Frame>,
    incoming_tx: Sender<Incoming>,
    mut listen_rx: Receiver<(StreamId, oneshot::Sender<Outcoming>)>,
) -> std::io::Result<()> {
    let mut map = HashMap::new();
    let mut listen_map = HashMap::<StreamId, oneshot::Sender<Outcoming>>::new();

    loop {
        match read_frame(&mut read).await? {
            Frame::Offer {
                s_id,
                name,
                window_size,
            } => {
                let (mut client, mut server) = tokio::io::duplex(64);

                map.insert(s_id, client);
                incoming_tx
                    .try_send(Incoming {
                        s_id,
                        name,
                        rx: server,
                        window_size,
                    })
                    .unwrap();
            }
            Frame::OfferAccepted { s_id, window_size } => {
                let (mut client, mut server) = tokio::io::duplex(64);
                if let Some(listen) = listen_map.remove(&s_id) {
                    listen
                        .send(Outcoming {
                            s_id,
                            rx: server,
                            window_size,
                        })
                        .unwrap();
                    map.insert(s_id, client);
                } else {
                    match listen_rx.try_recv() {
                        Ok((a_s_id, listen)) => {
                            if a_s_id != s_id {
                                listen_map.insert(a_s_id, listen);
                            } else {
                                listen
                                    .send(Outcoming {
                                        s_id,
                                        rx: server,
                                        window_size,
                                    })
                                    .unwrap();
                                map.insert(s_id, client);
                            }
                        }
                        Err(e) => match e {
                            TryRecvError::Empty => {}
                            TryRecvError::Disconnected => {
                                return Err(Error::new(
                                    ErrorKind::Other,
                                    "listen channel is closed",
                                ))
                            }
                        },
                    }
                }
            }
            Frame::Content { s_id, payload } => {
                let payload_len = payload.len() as i64;

                match map.get_mut(&s_id) {
                    Some(s) => match s.write_all(&payload).await {
                        Ok(_) => {
                            message_tx
                                .send(Frame::ContentProcessed {
                                    s_id,
                                    processed: payload_len,
                                })
                                .await
                                .unwrap();
                        }
                        Err(e) => {
                            map.remove(&s_id).unwrap();
                            println!("reader `{}` is die. {}", s_id, e);
                            message_tx
                                .try_send(Frame::ChannelTerminated { s_id })
                                .unwrap();
                        }
                    },
                    None => println!("unknown reader `{}` on Content frame", s_id),
                }
            }
            Frame::ContentProcessed { s_id, processed } => {
                println!("`{}` processed content size `{}`", s_id, processed)
            }
            Frame::ContentWritingCompleted { s_id } => match map.remove(&s_id) {
                Some(r) => drop(r),
                None => println!("unknown reader `{}` on ContentWritingCompleted frame", s_id),
            },
            Frame::ChannelTerminated { s_id } => match map.remove(&s_id) {
                Some(r) => drop(r),
                None => println!("unknown reader `{}` on ChannelTerminated", s_id),
            },
        }
    }
}

async fn write_loop<W: AsyncWrite + Unpin>(
    mut write: W,
    mut message_rx: Receiver<Frame>,
    mut incoming_rx: Receiver<Incoming>,
    mut write_q_rx: Receiver<Vec<u8>>,
) -> std::io::Result<()> {
    let mut writers = Vec::new();
    loop {
        let mut processed = false;

        match message_rx.try_recv() {
            Ok(msg) => {
                write_frame(&mut write, msg.into()).await?;
                processed = true;
            }
            Err(e) => match e {
                TryRecvError::Empty => {}
                TryRecvError::Disconnected => {
                    return Err(Error::new(ErrorKind::Other, "message channel is closed"))
                }
            },
        }
        match incoming_rx.try_recv() {
            Ok(w) => {
                writers.push(w);
                processed = true;
            }
            Err(e) => match e {
                TryRecvError::Empty => {}
                TryRecvError::Disconnected => {
                    return Err(Error::new(ErrorKind::Other, "incoming channel is closed"))
                }
            },
        }
        match write_q_rx.try_recv() {
            Ok(w) => {
                let frame = Frame::Content {
                    s_id: 1,
                    payload: w,
                };
                write_frame(&mut write, frame.into()).await?;
                processed = true;
            }
            Err(e) => match e {
                TryRecvError::Empty => {}
                TryRecvError::Disconnected => {
                    return Err(Error::new(ErrorKind::Other, "incoming channel is closed"))
                }
            },
        }

        if (!processed) {
            tokio::task::yield_now().await;
        }
    }
}
