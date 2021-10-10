use std::{
    cmp,
    collections::HashMap,
    io::{Error, ErrorKind, Read, Write},
    sync::{
        atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
};

use futures::{channel::oneshot, future::BoxFuture, Future, FutureExt};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::{
        mpsc::{self, error::TryRecvError, Receiver, Sender},
        Mutex,
    },
};
use tokio_util::compat::TokioAsyncReadCompatExt;

use crate::{
    frame::{read_frame, write_frame, Frame, StreamId},
    pipe::{self, PipeReader, PipeWriter},
};

const DEFAULT_REM_WIN_SIZE: usize = 5 * 20 * 1024;

pub struct Multiplexor {
    //read_handler: JoinHandle<Result<(), Error>>,
    //write_handler: JoinHandle<Result<(), Error>>,
    incoming_r_rx: Receiver<Incoming>,
    listen_tx: Sender<(Outcoming, oneshot::Sender<Incoming>)>,

    incoming_w_tx: Sender<Incoming>,
    message_tx: Sender<Frame>,

    write_q_tx: Sender<WriteMsg>,

    last_id: Arc<AtomicUsize>,
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
            last_id: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub async fn listen(&mut self) -> Result<(String, PipeReader, PipeWriter), Error> {
        let incoming = self.incoming_r_rx.recv().await.unwrap();

        self.message_tx
            .send(Frame::OfferAccepted {
                id: incoming.id,
                rem_window_size: Some(DEFAULT_REM_WIN_SIZE as u64),
            })
            .await
            .unwrap();

        let writer = run_read(self.write_q_tx.clone(), &incoming);

        Ok((incoming.name, incoming.rx, writer))
    }

    pub fn outcoming(&mut self) -> OutcomminStream {
        OutcomminStream {
            last_id: self.last_id.clone(),
            listen_tx: self.listen_tx.clone(),
            message_tx: self.message_tx.clone(),
            write_q_tx: self.write_q_tx.clone(),
        }
    }
}

pub struct OutcomminStream {
    last_id: Arc<AtomicUsize>,
    listen_tx: Sender<(Outcoming, oneshot::Sender<Incoming>)>,
    message_tx: Sender<Frame>,
    write_q_tx: Sender<WriteMsg>,
}

impl OutcomminStream {
    pub async fn offer(
        &mut self,
        name: impl Into<String>,
    ) -> Result<(PipeReader, PipeWriter), Error> {
        let id = StreamId::Local(self.last_id.fetch_add(1, Ordering::Relaxed) as u64);
        let name = name.into();

        let (incoming_tx, incoming_rx) = oneshot::channel();
        self.listen_tx
            .send((
                Outcoming {
                    id,
                    name: name.clone(),
                },
                incoming_tx,
            ))
            .await
            .unwrap();

        self.message_tx
            .send(Frame::Offer {
                id: id,
                name: name.clone(),
                window_size: Some(DEFAULT_REM_WIN_SIZE as u64),
            })
            .await
            .unwrap();

        let incoming = incoming_rx.await.unwrap();

        //println!("incoming: {:?}", incoming);

        let writer = run_read(self.write_q_tx.clone(), &incoming);

        Ok((incoming.rx, writer))
    }
}

#[derive(Debug)]
enum WriteMsg {
    Content { id: StreamId, payload: Vec<u8> },
    Error { id: StreamId, error: Error },
}

#[derive(Debug)]
struct Incoming {
    id: StreamId,
    name: String,
    rx: PipeReader,
    rem_w_size: Arc<AtomicUsize>,
}

#[derive(Debug)]
struct Outcoming {
    id: StreamId,
    name: String,
}

fn run_read(write_q_tx: Sender<WriteMsg>, incoming: &Incoming) -> PipeWriter {
    let (mut reader, writer) = pipe::new_pipe(DEFAULT_REM_WIN_SIZE);
    let id = incoming.id;
    let rem_w_size = incoming.rem_w_size.clone();

    tokio::spawn(async move {
        let mut buf = [0_u8; 2 * 1024];
        loop {
            let size = rem_w_size.load(Ordering::Relaxed);
            let size = cmp::min(size, buf.len());
            if size <= 0 {
                tokio::task::yield_now().await;
                continue;
            }
            let (msg, br) = match reader.read(&mut buf[0..size]).await {
                Ok(n) => {
                    if n == 0 {
                        (
                            WriteMsg::Error {
                                id,
                                error: Error::new(ErrorKind::Other, "read zero buffer"),
                            },
                            true,
                        )
                    } else {
                        rem_w_size.fetch_sub(n, Ordering::Relaxed);
                        (
                            WriteMsg::Content {
                                id,
                                payload: buf[0..n].to_vec(),
                            },
                            false,
                        )
                    }
                }
                Err(e) => (WriteMsg::Error { id, error: e }, true),
            };
            write_q_tx.send(msg).await.unwrap();
            if br {
                return;
            }
        }
    });

    writer
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
                id,
                name,
                window_size,
            } => {
                let (reader, writer) = pipe::new_pipe(DEFAULT_REM_WIN_SIZE);
                let rem_w_size = Arc::new(AtomicUsize::new(
                    window_size.map_or(DEFAULT_REM_WIN_SIZE, |x| x as usize),
                ));
                let rem_w_size_ref = rem_w_size.clone();
                map.insert(id, (writer, rem_w_size_ref));
                incoming_tx
                    .send(Incoming {
                        id,
                        name,
                        rx: reader,
                        rem_w_size,
                    })
                    .await
                    .unwrap();
            }
            Frame::OfferAccepted {
                id,
                rem_window_size,
            } => {
                let (reader, writer) = pipe::new_pipe(DEFAULT_REM_WIN_SIZE);
                let rem_w_size = Arc::new(AtomicUsize::new(
                    rem_window_size.map_or(DEFAULT_REM_WIN_SIZE, |x| x as usize),
                ));
                let rem_w_size_ref = rem_w_size.clone();

                if let Some((outcoming, listen)) = listen_map.remove(&id) {
                    map.insert(id, (writer, rem_w_size_ref));
                    listen
                        .send(Incoming {
                            id: outcoming.id,
                            name: outcoming.name,
                            rx: reader,
                            rem_w_size,
                        })
                        .unwrap();
                } else {
                    loop {
                        match listen_rx.try_recv() {
                            Ok((outcoming, listen)) => {
                                if outcoming.id != id {
                                    listen_map.insert(outcoming.id, (outcoming, listen));
                                } else {
                                    map.insert(id, (writer, rem_w_size_ref));
                                    listen
                                        .send(Incoming {
                                            id: outcoming.id,
                                            name: outcoming.name,
                                            rx: reader,
                                            rem_w_size,
                                        })
                                        .unwrap();
                                    break;
                                }
                            }
                            Err(e) => match e {
                                TryRecvError::Empty => break,
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
            }
            Frame::Content { id, payload } => match map.get_mut(&id) {
                Some((w, _)) => match w.write_all(&payload).await {
                    Ok(_) => {
                        message_tx
                            .send(Frame::ContentProcessed {
                                id,
                                processed: payload.len() as i64,
                            })
                            .await
                            .unwrap();
                    }
                    Err(e) => {
                        map.remove(&id).unwrap();
                        println!("reader `{:?}` is die. {}", id, e);
                        message_tx
                            .send(Frame::ChannelTerminated { id })
                            .await
                            .unwrap();
                    }
                },
                None => {} //println!("unknown reader `{:?}` on Content frame", id),
            },
            Frame::ContentProcessed { id, processed } => match map.get_mut(&id) {
                Some((_, r)) => {
                    println!("processed: {}", processed);
                    r.fetch_add(processed as usize, Ordering::Relaxed);
                }
                None => {} //println!("unknown reader `{:?}` on ContentProcessed frame", id),
            },
            Frame::ContentWritingCompleted { id } => match map.remove(&id) {
                Some(r) => drop(r),
                None => {} //println!("unknown reader `{:?}` on ContentWritingCompleted frame", id),
            },
            Frame::ChannelTerminated { id } => match map.remove(&id) {
                Some(r) => drop(r),
                None => {} //println!("unknown reader `{:?}` on ChannelTerminated", id),
            },
        }
    }
}

async fn write_loop<W: AsyncWrite + Unpin>(
    write: W,
    mut message_rx: Receiver<Frame>,
    mut incoming_rx: Receiver<Incoming>,
    mut write_q_rx: Receiver<WriteMsg>,
) -> std::io::Result<()> {
    let mut writers = Vec::new();

    let write = Arc::new(Mutex::new(write));
    let write_ref1 = write.clone();

    let write_message = async move {
        loop {
            match message_rx.recv().await {
                Some(msg) => {
                    let mut write = write.lock().await;
                    write_frame(&mut *write, msg.into()).await?;
                }
                None => return Err(Error::new(ErrorKind::Other, "message channel is closed")),
            }
        }
        Ok(())
    };

    let incoming = async move {
        loop {
            match incoming_rx.recv().await {
                Some(w) => {
                    writers.push(w);
                }
                None => return Err(Error::new(ErrorKind::Other, "incoming channel is closed")),
            }
        }
        Ok(())
    };

    let write_content = async move {
        loop {
            match write_q_rx.recv().await {
                Some(m) => {
                    let frame = match m {
                        WriteMsg::Content { id, payload } => Frame::Content { id, payload },
                        WriteMsg::Error { id, error } => {
                            //println!("writer `{:?}` is die. {}", id, error);
                            Frame::ChannelTerminated { id }
                        }
                    };
                    let mut write = write_ref1.lock().await;
                    write_frame(&mut *write, frame).await?;
                }
                None => return Err(Error::new(ErrorKind::Other, "incoming channel is closed")),
            }
        }
        Ok(())
    };

    tokio::try_join!(write_message, incoming, write_content)?;

    Ok(())
}
