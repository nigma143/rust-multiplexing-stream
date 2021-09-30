use std::{
    collections::HashMap,
    io::{Error, ErrorKind, Read, Write},
    sync::Arc,
    thread::{self, JoinHandle},
};

use async_std::{
    channel::{self, Receiver, Sender},
    sync::Mutex,
    task::{self},
};
use futures::{channel::oneshot, future::BoxFuture, AsyncReadExt, Future, FutureExt};

use crate::{
    frame::{read_frame, sync_read_frame, sync_write_frame, Frame, StreamId},
    rw::{Reader, Writer},
};

type StreamMaps = Arc<Mutex<HashMap<StreamId, StreamState>>>;

type WriterMaps = Arc<Mutex<HashMap<StreamId, Receiver<Vec<u8>>>>>;

type OfferAwaiter = Arc<Mutex<Option<oneshot::Sender<Offered>>>>;

struct Offered {
    s_id: StreamId,
    name: String,
    reader: Receiver<Vec<u8>>,
    window_size: Option<u64>,
}

enum StreamState {
    Awaiting(oneshot::Sender<Offered>),
    Ready(Sender<Vec<u8>>),
}

pub struct Multiplexor {
    read_handler: JoinHandle<Result<(), Error>>,
    write_handler: JoinHandle<Result<(), Error>>,

    incoming: Receiver<Incoming>,
    outcoming: Receiver<Outcoming>,

    message_tx: Sender<Frame>,

    in_writer_tx: Sender<Incoming>,
}

impl Multiplexor {
    pub fn new<R: Read + Send + 'static, W: Write + Send + 'static>(read: R, write: W) -> Self {
        let (incoming_tx, incoming_rx) = channel::unbounded();
        let (outcoming_tx, outcoming_rx) = channel::unbounded();
        let read_handler = thread::spawn(move || read_loop(read, incoming_tx, outcoming_tx));

        let (in_writer_tx, in_writer_rx) = channel::unbounded();
        let (message_tx, message_rx) = channel::unbounded();
        let write_handler = thread::spawn(move || write_loop(write, message_rx, in_writer_rx));

        Self {
            write_handler,
            read_handler,
            incoming: incoming_rx,
            outcoming: outcoming_rx,
            message_tx,
            in_writer_tx,
        }
    }

    pub async fn listen(&mut self) -> Result<(Writer<u8>, Reader<u8>), Error> {
        let incoming = self.incoming.recv().await.unwrap();

        let (write_tx, write_rx) = channel::unbounded();

        self.in_writer_tx
            .send(Incoming {
                s_id: incoming.s_id,
                name: incoming.name,
                rx: write_rx,
                window_size: incoming.window_size,
            })
            .await
            .unwrap();

        self.message_tx
            .send(Frame::OfferAccepted {
                s_id: 1,
                window_size: None,
            })
            .await
            .unwrap();

        Ok((Writer::new(write_tx), Reader::new(incoming.rx)))
    }

    pub async fn offer(&mut self) -> Result<(Writer<u8>, Reader<u8>), Error> {
        self.message_tx
            .send(Frame::Offer {
                s_id: 1,
                name: "Common".into(),
                window_size: None,
            })
            .await
            .unwrap();
            
        let outcoming = self.outcoming.recv().await.unwrap();
        
        let (write_tx, write_rx) = channel::unbounded();

        self.in_writer_tx
            .send(Incoming {
                s_id: outcoming.s_id,
                name: "Common".into(),
                rx: write_rx,
                window_size: outcoming.window_size,
            })
            .await
            .unwrap();

        Ok((Writer::new(write_tx), Reader::new(outcoming.rx)))
    }
}

fn write_loop<W: Write>(
    mut write: W,
    message_rx: Receiver<Frame>,
    in_writer_rx: Receiver<Incoming>,
) -> std::io::Result<()> {
    let mut writers = Vec::new();
    loop {
        if !message_rx.is_empty() {
            match message_rx.try_recv() {
                Ok(o) => {
                    sync_write_frame(&mut write, o.into())?;
                }
                Err(e) => println!("Message channel is die. {}", e),
            }
        }

        if !in_writer_rx.is_empty() {
            match in_writer_rx.try_recv() {
                Ok(o) => {
                    writers.push(o);
                }
                Err(e) => println!("Incoimng writer channel is die. {}", e),
            }
        }
       
        for i in 0..writers.len() {
            let item = writers.get(i).unwrap();
            if !item.rx.is_empty() {
                println!("{}", item.rx.len());
                match item.rx.try_recv() {
                    Ok(o) => {
                        let frame = Frame::Content {
                            s_id: item.s_id,
                            payload: o,
                        };                        
                        sync_write_frame(&mut write, frame.into())?;
                    }
                    Err(e) => {
                        println!("Writer `{}` is die. {}", item.s_id, e);
                        writers.remove(i);
                    }
                }
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
    incoming: Sender<Incoming>,
    outcoming: Sender<Outcoming>,
) -> std::io::Result<()> {
    let mut map = HashMap::new();
    loop {
        let frame = sync_read_frame(&mut read)?;
        match frame {
            Frame::Offer {
                s_id,
                name,
                window_size,
            } => {
                let (tx, rx) = channel::unbounded();
                map.insert(s_id, tx);

                incoming
                    .try_send(Incoming {
                        s_id,
                        name,
                        rx: rx,
                        window_size,
                    })
                    .unwrap();
            }
            Frame::OfferAccepted { s_id, window_size } => {
                let (tx, rx) = channel::unbounded();
                map.insert(s_id, tx);

                outcoming
                    .try_send(Outcoming {
                        s_id,
                        rx: rx,
                        window_size,
                    })
                    .unwrap();
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
