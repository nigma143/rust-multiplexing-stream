use std::{
    collections::HashMap,
    io::{Error, ErrorKind, Read, Write},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

use futures::{channel::oneshot, future::BoxFuture, Future, FutureExt};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc::{self, error::TryRecvError, Receiver, Sender},
};
use tokio_util::compat::TokioAsyncReadCompatExt;

use crate::{
    frame::{read_frame, write_frame, Frame, StreamId},
    pipe::{self, PipeReader, PipeWriter},
};

pub struct Multiplexor {
    //read_handler: JoinHandle<Result<(), Error>>,
    //write_handler: JoinHandle<Result<(), Error>>,
    incoming_r_rx: Receiver<Incoming>,
    listen_tx: Sender<(Outcoming, oneshot::Sender<Incoming>)>,

    incoming_w_tx: Sender<Incoming>,
    message_tx: Sender<Frame>,

    write_q_tx: Sender<WriteMsg>,

    last_id: u64,
}

#[derive(Debug)]
enum WriteMsg {
    Content { s_id: StreamId, payload: Vec<u8> },
    Error { s_id: StreamId, error: Error },
}

impl Multiplexor {
    pub fn new<R: AsyncRead + Unpin + Send + 'static, W: AsyncWrite + Send + Unpin + 'static>(
        read: R,
        write: W,
    ) -> Self {
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

        let (write_q_tx, write_q_rx) = mpsc::channel(1024);

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

    pub async fn listen(&mut self) -> Result<(String, PipeReader, PipeWriter), Error> {
        let incoming = self.incoming_r_rx.recv().await.unwrap();

        self.message_tx
            .send(Frame::OfferAccepted {
                s_id: incoming.s_id,
                window_size: None,
            })
            .await
            .unwrap();
        
        let writer = self.run_read(&incoming);

        Ok((incoming.name, incoming.rx, writer))
    }

    pub async fn offer(
        &mut self,
        name: impl Into<String>,
    ) -> Result<(PipeReader, PipeWriter), Error> {
        self.last_id += 1;
        let id = StreamId::Local(self.last_id);
        let name = name.into();

        let (incoming_tx, incoming_rx) = oneshot::channel();
        self.listen_tx
            .send((
                Outcoming {
                    s_id: id,
                    name: name.clone(),
                    window_size: Some(1024),
                },
                incoming_tx,
            ))
            .await
            .unwrap();

        self.message_tx
            .send(Frame::Offer {
                s_id: id,
                name: name.clone(),                
                window_size: Some(1024),
            })
            .await
            .unwrap();

        let incoming = incoming_rx.await.unwrap();

        let writer = self.run_read(&incoming);

        Ok((incoming.rx, writer))
    }

    fn run_read(&self, incoming: &Incoming) -> PipeWriter {         
        let write_q_tx = self.write_q_tx.clone();
        let (mut reader, writer) = pipe::new_pipe(2048); 
        let s_id = incoming.s_id;  

        tokio::spawn(async move {
            let mut buf = [0_u8; 1024];
            let msg = match reader.read(&mut buf[..]).await {
                Ok(n) => WriteMsg::Content {
                    s_id: s_id,
                    payload: buf[0..n].to_vec(),
                },
                Err(e) => WriteMsg::Error { s_id: s_id, error: e },
            };
            write_q_tx.send(msg).await.unwrap();
        });

        writer
    }
}

#[derive(Debug)]
struct Incoming {
    s_id: StreamId,
    name: String,
    rx: PipeReader,
    window_size: Option<u64>,
}

#[derive(Debug)]
struct Outcoming {
    s_id: StreamId,
    name: String,
    window_size: Option<u64>,
}

async fn read_loop<R: AsyncRead + Unpin + Send>(
    mut read: R,
    message_tx: Sender<Frame>,
    incoming_tx: Sender<Incoming>,
    mut listen_rx: Receiver<(Outcoming, oneshot::Sender<Incoming>)>,
) -> std::io::Result<()> {
    let mut map = HashMap::new();
    let mut listen_map = HashMap::<StreamId, (Outcoming, oneshot::Sender<Incoming>)>::new();

    loop {
        match read_frame(&mut read).await? {
            Frame::Offer {
                s_id,
                name,
                window_size,
            } => {
                let (reader, writer) = pipe::new_pipe(2048);
                map.insert(s_id, writer);
                incoming_tx
                    .send(Incoming {
                        s_id,
                        name,
                        rx: reader,
                        window_size,
                    })
                    .await
                    .unwrap();
            }
            Frame::OfferAccepted { s_id, window_size } => {
                let (reader, writer) = pipe::new_pipe(2048);
                if let Some((outcoming, listen)) = listen_map.remove(&s_id) {
                    map.insert(s_id, writer);
                    listen
                        .send(Incoming {
                            s_id: outcoming.s_id,
                            name: outcoming.name,
                            rx: reader,
                            window_size: window_size,
                        })
                        .unwrap();
                } else {
                    match listen_rx.try_recv() {
                        Ok((outcoming, listen)) => {
                            if outcoming.s_id != s_id {
                                listen_map.insert(outcoming.s_id, (outcoming, listen));
                            } else {
                                map.insert(s_id, writer);
                                listen
                                    .send(Incoming {
                                        s_id: outcoming.s_id,
                                        name: outcoming.name,
                                        rx: reader,
                                        window_size: window_size,
                                    })
                                    .unwrap();
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
            Frame::Content { s_id, payload } => match map.get_mut(&s_id) {
                Some(s) => match s.write_all(&payload).await {
                    Ok(_) => {
                        message_tx
                            .send(Frame::ContentProcessed {
                                s_id,
                                processed: payload.len() as i64,
                            })
                            .await
                            .unwrap();
                    }
                    Err(e) => {
                        map.remove(&s_id).unwrap();
                        println!("reader `{:?}` is die. {}", s_id, e);
                        message_tx
                            .send(Frame::ChannelTerminated { s_id })
                            .await
                            .unwrap();
                    }
                },
                None => println!("unknown reader `{:?}` on Content frame", s_id),
            },
            Frame::ContentProcessed { s_id, processed } => {
                println!("`{:?}` processed content size `{}`", s_id, processed)
            }
            Frame::ContentWritingCompleted { s_id } => match map.remove(&s_id) {
                Some(r) => drop(r),
                None => println!("unknown reader `{:?}` on ContentWritingCompleted frame", s_id),
            },
            Frame::ChannelTerminated { s_id } => match map.remove(&s_id) {
                Some(r) => drop(r),
                None => println!("unknown reader `{:?}` on ChannelTerminated", s_id),
            },
        }
    }
}

async fn write_loop<W: AsyncWrite + Unpin>(
    mut write: W,
    mut message_rx: Receiver<Frame>,
    mut incoming_rx: Receiver<Incoming>,
    mut write_q_rx: Receiver<WriteMsg>,
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
            Ok(m) => {
                let frame = match m {
                    WriteMsg::Content { s_id, payload } => Frame::Content { s_id, payload },
                    WriteMsg::Error { s_id, error } => {
                        println!("writer `{:?}` is die. {}", s_id, error);
                        Frame::ChannelTerminated { s_id }
                    }
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

        if !processed {
            tokio::task::yield_now().await;
        }
    }
}
