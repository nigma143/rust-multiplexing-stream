use std::{
    collections::HashMap,
    io::{Error, ErrorKind, Read, Write},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
};

use futures::{channel::oneshot, future::BoxFuture, AsyncReadExt, Future, FutureExt};
use tokio::sync::mpsc::{self, Receiver, Sender};

use crate::{
    frame::{sync_read_frame, sync_write_frame, Frame, StreamId},
    rw::{Reader, Writer},
};

pub struct Multiplexor {
    read_handler: JoinHandle<Result<(), Error>>,
    write_handler: JoinHandle<Result<(), Error>>,

    incoming: Receiver<Incoming>,
    outcoming: Sender<(StreamId, oneshot::Sender<Outcoming>)>,

    message_tx: Sender<Frame>,

    in_writer_tx: Sender<Incoming>,
}

impl Multiplexor {
    pub fn new<R: Read + Send + 'static, W: Write + Send + 'static>(read: R, write: W) -> Self {
        let (incoming_tx, incoming_rx) = mpsc::channel(1024);
        let (outcoming_tx, outcoming_rx) = mpsc::channel(1024);
        let read_handler = thread::spawn(move || read_loop(read, incoming_tx, outcoming_rx));

        let (in_writer_tx, in_writer_rx) = mpsc::channel(1024);
        let (message_tx, message_rx) = mpsc::channel(1024);
        let write_handler = thread::spawn(move || write_loop(write, message_rx, in_writer_rx));

        Self {
            write_handler,
            read_handler,
            incoming: incoming_rx,
            outcoming: outcoming_tx,
            message_tx,
            in_writer_tx,
        }
    }

    pub async fn listen(&mut self) -> Result<(String, Writer<u8>, Reader<u8>), Error> {
        let incoming = self.incoming.recv().await.unwrap();

        let (write_tx, write_rx) = mpsc::channel(1024);

        self.in_writer_tx
            .send(Incoming {
                s_id: incoming.s_id,
                name: incoming.name.clone(),
                rx: write_rx,
                window_size: incoming.window_size,
            })
            .await;

        self.message_tx
            .send(Frame::OfferAccepted {
                s_id: 1,
                window_size: None,
            })
            .await;

        Ok((incoming.name, Writer::new(write_tx), Reader::new(incoming.rx)))
    }

    pub async fn offer(&mut self, name: impl Into<String>) -> Result<(Writer<u8>, Reader<u8>), Error> {
        let (outcoming_t, outcoming_r) = oneshot::channel();

        self.outcoming.send((1, outcoming_t)).await;

        self.message_tx
            .send(Frame::Offer {
                s_id: 1,
                name: name.into(),
                window_size: None,
            })
            .await;

        let outcoming = outcoming_r.await.unwrap();

        let (write_tx, write_rx) = mpsc::channel(1024);

        self.in_writer_tx
            .send(Incoming {
                s_id: outcoming.s_id,
                name: "Common".into(),
                rx: write_rx,
                window_size: outcoming.window_size,
            })
            .await;

        Ok((Writer::new(write_tx), Reader::new(outcoming.rx)))
    }
}

fn write_loop<W: Write>(
    mut write: W,
    mut message_rx: Receiver<Frame>,
    mut in_writer_rx: Receiver<Incoming>,
) -> std::io::Result<()> {
    let mut writers = Vec::new();
    loop {
        match message_rx.try_recv() {
            Ok(msg) => sync_write_frame(&mut write, msg.into())?,
            Err(e) => {
                match e {
                    mpsc::error::TryRecvError::Empty => {},
                    mpsc::error::TryRecvError::Disconnected => 
                    return Err(Error::new(ErrorKind::Other, "channel is closed".to_owned())),
                }
            },
        }

        match in_writer_rx.try_recv() {
            Ok(writer) => {
                writers.push(writer);   
            },
            Err(e) => {
                match e {
                    mpsc::error::TryRecvError::Empty => {},
                    mpsc::error::TryRecvError::Disconnected => 
                    return Err(Error::new(ErrorKind::Other, "channel is closed".to_owned())),
                }
            },
        }

        for i in 0..writers.len() {
            let item = writers.get_mut(i).unwrap();

            match item.rx.try_recv() {
                Ok(msg) => {
                    let frame = Frame::Content {
                        s_id: item.s_id,
                        payload: msg,
                    };
                    sync_write_frame(&mut write, frame.into())?;  
                },
                Err(e) => {
                    match e {
                        mpsc::error::TryRecvError::Empty => {},
                        mpsc::error::TryRecvError::Disconnected => {
                            println!("Writer `{}` is die. {}", item.s_id, e);
                            writers.remove(i);
                        },
                    }
                },
            }
        }
    }
}

struct Incoming {
    s_id: StreamId,
    name: String,
    rx: Receiver<Vec<u8>>,
    window_size: Option<u64>,
}

struct Outcoming {
    s_id: StreamId,
    rx: Receiver<Vec<u8>>,
    window_size: Option<u64>,
}

fn read_loop<R: Read>(
    mut read: R,
    mut incoming: Sender<Incoming>,
    mut outcoming: Receiver<(StreamId, oneshot::Sender<Outcoming>)>,
) -> std::io::Result<()> {
    let mut map = HashMap::new();
    let mut awaiter_map = HashMap::<StreamId, oneshot::Sender<Outcoming>>::new();
    loop {
        let frame = sync_read_frame(&mut read)?;
        match frame {
            Frame::Offer {
                s_id,
                name,
                window_size,
            } => {
                let (tx, rx) = mpsc::channel(1024);
                map.insert(s_id, tx);
                incoming
                    .try_send(Incoming {
                        s_id,
                        name,
                        rx: rx,
                        window_size,
                    });
            }
            Frame::OfferAccepted { s_id, window_size } => {
                let (tx, rx) = mpsc::channel(1024);
                let s = match awaiter_map.remove(&s_id) {
                    Some(s) => Some(s),
                    None => {
                        let mut r = None;

                        loop {
                            match outcoming.try_recv() {
                                Ok((a_s_id, sender)) => {
                                    if a_s_id == s_id {
                                        r = Some(sender);
                                        break;
                                    }
                                    awaiter_map.insert(a_s_id, sender);
                                },
                                Err(e) => {
                                    match e {
                                        mpsc::error::TryRecvError::Empty => break,
                                        mpsc::error::TryRecvError::Disconnected => 
                                        return Err(Error::new(ErrorKind::Other, "channel is closed".to_owned())),
                                    }
                                },
                            }
                        }

                        r
                    }
                };
                if let Some(s) = s {
                    s.send(Outcoming {
                        s_id,
                        rx: rx,
                        window_size,
                    });
                    map.insert(s_id, tx);
                }
            }
            Frame::Content { s_id, payload } => match map.get(&s_id) {
                Some(s) => match s.try_send(payload) {
                    Ok(_) => {}
                    Err(e) => {
                        map.remove(&s_id).unwrap();
                        println!("Reader `{}` is die. {}", s_id, e)
                    }
                },
                None => todo!(),
            },
            Frame::ContentWritingCompleted { s_id } => todo!(),
        }
    }
}

impl From<Frame> for std::io::Error {
    fn from(_: Frame) -> Self {
        todo!()
    }
}
