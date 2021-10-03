use std::{
    borrow::{Borrow, BorrowMut},
    collections::{HashMap, VecDeque},
    default,
    fmt::write,
    hash::Hash,
    io::{self, Cursor, Error, ErrorKind, Read, Write},
    iter::FromIterator,
    ops::Deref,
    sync::{
        mpsc::{self, sync_channel, Receiver, Sender},
        Arc, PoisonError,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use futures::{
    channel::{
        mpsc::UnboundedReceiver,
        oneshot::{self, channel},
    },
    future::{BoxFuture, LocalBoxFuture},
    AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, FutureExt, StreamExt,
};
use multiplexor::Multiplexor;

use crate::frame::*;

mod frame;
mod multiplexor;
mod rw;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {

        //let (in_tx, in_rx) = channel::unbounded();
        //let (out_tx, out_rx) = channel::unbounded();
        /*
        let mut m = MultiplexingStream {
            input: in_tx,
            output: out_rx,
            output_buf: VecDeque::new()
        };

        let b = vec![0, 1, 2];
        m.write(&b.as_slice()).await.unwrap();

        println!("{:?}", in_rx.recv().await.unwrap());

        let b = vec![0, 1, 2];
        out_tx.send(b).await.unwrap();

        let mut r = vec![0, 0];
        let rl = m.read(&mut r).await.unwrap();
        println!("{} {:?}", rl, r);

        let mut r = vec![0, 0];
        let rl = m.read(&mut r).await.unwrap();
        println!("{} {:?}", rl, r);*/

        /*let (out_tx, out_rx) = channel::unbounded();

        let mut testio = TestIo {
            output: out_rx,
            output_buf: Default::default(),
            read_fut: None,
        };

        let mut stdout = async_std::io::stdout();
        let mut stdin = async_std::io::stdin();

        //let mut file = File::open("d://1.txt").await?;

        let m = Arc::new(Multiplexor::new(stdin));

        let m1 = m.clone();
        task::spawn(async move {
            let mut stream = future::timeout(Duration::from_millis(5000), m1.offer_stream())
                .await
                .unwrap();

            println!("==1 offered");

            loop {
                let mut buf = [0; 5];
                AsyncReadExt::read(&mut stream, &mut buf).await.unwrap();
                println!("==1: {:?}", buf);
            }
            //let b = "Hello".as_bytes();
            //s.send(&b).await.unwrap();
        });

        let r: Vec<u8> = Frame::OfferAccepted {
            s_id: 1,
            window_size: None,
        }
        .into();
        out_tx.send(vec![r.len() as u8]).await.unwrap();
        out_tx.send(r).await.unwrap();

        let m0 = m.clone();
        task::spawn(async move {
            m0.run(testio).await.unwrap();
        });

        task::sleep(Duration::from_millis(500)).await;

        let r: Vec<u8> = Frame::Content {
            s_id: 1,
            payload: "sdfdsfds".to_owned().into_bytes(),
        }
        .into();
        out_tx.send(vec![r.len() as u8]).await.unwrap();
        out_tx.send(r).await.unwrap();

        let r: Vec<u8> = Frame::Content {
            s_id: 1,
            payload: "sdfdsfds".to_owned().into_bytes(),
        }
        .into();
        out_tx.send(vec![r.len() as u8]).await.unwrap();
        out_tx.send(r).await.unwrap();

        task::sleep(Duration::from_secs(100)).await;*/

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
                tokio::time::sleep(Duration::from_secs(100)).await;
        });

        let mut mux = Multiplexor::new(s_r, c_w);

        loop {
            let (name, mut writer, mut reader) = mux.listen().await.unwrap();
            println!("incoming stream: {}", name);

            tokio::spawn(async move {
                let mut buf = [1; 1];
                AsyncReadExt::read_exact(&mut reader, &mut buf)
                    .await
                    .unwrap();
                println!("Server in: {:?}", buf);
            });
        }

        Ok(())
}

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
