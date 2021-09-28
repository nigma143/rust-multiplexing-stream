use std::{
    borrow::{Borrow, BorrowMut},
    collections::{HashMap, VecDeque},
    default,
    fmt::write,
    hash::Hash,
    io::{self, Cursor, Error, ErrorKind, Read, Result, Write},
    ops::Deref,
    sync::{Arc, PoisonError},
    thread::{self, JoinHandle},
    time::Duration,
};

use async_std::{
    channel::{self, Receiver, RecvError, Send, SendError, Sender},
    stream,
    sync::Mutex,
};
use async_std::{
    fs::File,
    future,
    prelude::*,
    task::{self, sleep},
};

use futures::{
    channel::{
        mpsc::UnboundedReceiver,
        oneshot::{self, channel},
    },
    AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, FutureExt, StreamExt,
};

use crate::frame::*;

mod frame;

fn main() -> io::Result<()> {
    task::block_on(async {
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

        let (out_tx, out_rx) = channel::unbounded();

        let mut testio = TestIo {
            output: out_rx,
            output_buf: Default::default(),
        };

        let mut stdout = async_std::io::stdout();
        let mut stdin = async_std::io::stdin();

        let mut file = File::open("d://1.txt").await?;

        let m = Arc::new(Multiplexor::new(stdin));

        let m1 = m.clone();
        task::spawn(async move {
            /*let mut stream = future::timeout(Duration::from_millis(5000), m1.offer_stream())
                .await
                .unwrap();

            println!("==1 offered");

            let mut buf = [0; 5];

            AsyncReadExt::read(&mut stream, &mut buf).await.unwrap();

            println!("==1: {:?}", buf);*/
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
            //m0.run(testio).await.unwrap();
            loop {
                //let mut b = vec![0; 1];
                //AsyncReadExt::read_exact(&mut testio, &mut b).await.unwrap();
                let mut b = [0; 5];
                AsyncReadExt::read(&mut testio, &mut b).await.unwrap();
                println!("{:?}", b);                
            }
        });

        std::thread::sleep(Duration::from_millis(500));
        //task::sleep(Duration::from_millis(500)).await;

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
        
        task::sleep(Duration::from_secs(100)).await;

        Ok(())
    })
}

struct Multiplexor<W> {
    write: Arc<Mutex<W>>,
    streams: Streams,
}

impl<W> Multiplexor<W> {
    fn new(write: W) -> Self {
        Self {
            write: Arc::new(Mutex::new(write)),
            streams: Streams::default(),
        }
    }

    async fn run<R: AsyncReadExt + Unpin>(&self, read: R) -> Result<()> {
        read_loop(read, self.streams.clone()).await
    }

    async fn offer_stream(&self) -> Stream {
        let (tx, rx) = oneshot::channel::<(Receiver<Vec<u8>>, Option<u64>)>();
        self.streams
            .lock()
            .await
            .insert(1, StreamState::Awaiting(tx));

        let (output, windows_size) = rx.await.unwrap();

        Stream::new(output)
    }
}

async fn read_loop<R: AsyncReadExt + Unpin>(mut read: R, streams: Streams) -> Result<()> {
    loop {
        let frame = read_frame(&mut read).await?;
        match frame {
            Frame::Offer {
                s_id,
                name,
                window_size,
            } => todo!(),
            Frame::OfferAccepted { s_id, window_size } => {
                let mut streams = streams.lock().await;
                match streams.remove(&s_id) {
                    Some(s) => match s {
                        StreamState::Awaiting(a) => {
                            let (out_tx, out_rx) = channel::unbounded();
                            streams.insert(s_id, StreamState::Ready(out_tx));
                            a.send((out_rx, window_size)).unwrap();
                        }
                        StreamState::Ready(_) => todo!(),
                    },
                    None => todo!(),
                }
            }
            Frame::Content { s_id, payload } => {
                let streams = streams.lock().await;
                match streams.get(&s_id) {
                    Some(s) => match s {
                        StreamState::Awaiting(_) => todo!(),
                        StreamState::Ready(out_tx) => match out_tx.send(payload).await {
                            Ok(_) => {}
                            Err(e) => println!("Stream `{}` is die. {}", s_id, e),
                        },
                    },
                    None => todo!(),
                }
            }
            Frame::ContentWritingCompleted { s_id } => todo!(),
        }
    }
}

type Streams = Arc<Mutex<HashMap<StreamId, StreamState>>>;

enum StreamState {
    Awaiting(oneshot::Sender<(Receiver<Vec<u8>>, Option<u64>)>),
    Ready(Sender<Vec<u8>>),
}

struct Stream {
    output: Receiver<Vec<u8>>,
    output_buf: VecDeque<u8>,
}

impl Stream {
    fn new(output: Receiver<Vec<u8>>) -> Self {
        Self {
            output,
            output_buf: VecDeque::new(),
        }
    }
}

impl AsyncRead for Stream {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut [u8],
    ) -> task::Poll<Result<usize>> {
        if !self.output_buf.is_empty() {
            let drain_size = if self.output_buf.len() > buf.len() {
                buf.len()
            } else {
                self.output_buf.len()
            };

            let chunk: Vec<_> = self.output_buf.drain(0..drain_size).collect();
            buf[0..chunk.len()].copy_from_slice(&chunk);

            return task::Poll::Ready(Ok(chunk.len()));
        }

        let result = futures::ready!(self.output.recv().poll_unpin(cx));
        match result {
            Ok(received) => {
                if received.len() <= buf.len() {
                    buf[0..received.len()].copy_from_slice(&received);

                    task::Poll::Ready(Ok(received.len())) //
                } else {
                    buf.copy_from_slice(&received[..buf.len()]);
                    self.output_buf.extend(&received[buf.len()..]);

                    task::Poll::Ready(Ok(buf.len())) //
                }
            }
            Err(e) => task::Poll::Ready(Err(Error::new(ErrorKind::Other, e))),
        }
    }
}

struct TestIo {
    output: Receiver<Vec<u8>>,
    output_buf: VecDeque<u8>,
}

impl AsyncRead for TestIo {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut [u8],
    ) -> task::Poll<Result<usize>> {
      
        if !self.output_buf.is_empty() {
            let drain_size = if self.output_buf.len() > buf.len() {
                buf.len()
            } else {
                self.output_buf.len()
            };

            let chunk: Vec<_> = self.output_buf.drain(0..drain_size).collect();
            buf[0..chunk.len()].copy_from_slice(&chunk);

            return task::Poll::Ready(Ok(chunk.len()));
        } 

        let result = futures::ready!(self.output.poll_next_unpin(cx));
        match result {
            Some(received) => {
                if received.len() <= buf.len() {
                    buf[0..received.len()].copy_from_slice(&received);

                    task::Poll::Ready(Ok(received.len())) //
                } else {
                    buf.copy_from_slice(&received[..buf.len()]);
                    self.output_buf.extend(&received[buf.len()..]);

                    task::Poll::Ready(Ok(buf.len())) //
                }
            }
            None => task::Poll::Ready(Err(Error::new(ErrorKind::Other, "e"))),
        }
    }
}

/*
struct StreamWriter<W> {
    id: StreamId,
    write: Arc<Mutex<W>>,
}

impl<W: AsyncWrite + Unpin> StreamWriter<W> {
    async fn send(&mut self, buf: &[u8]) -> io::Result<()> {
        let mut write = self.write.lock().await;

        AsyncWriteExt::write(&mut *write, &self.id.to_be_bytes()).await?;
        AsyncWriteExt::write(&mut *write, buf).await?;
        AsyncWriteExt::flush(&mut *write).await
    }
}

struct MultiplexingStream<W> {
    writer: StreamWriter<W>,
}

impl<W: AsyncWrite + Unpin> MultiplexingStream<W> {
    fn new(writer: StreamWriter<W>) -> Self {
        Self { writer }
    }

    async fn send(&mut self, buf: &[u8]) -> io::Result<()> {
        self.writer.send(buf).await
    }
}

async fn read_frame<R>(read: R) -> Result<Frame>
where
    R: AsyncRead + Unpin,
{
    let h: [u8; 1] = [0; 1];
    AsyncReadExt::read_to_end(&mut read, &mut h).await.unwrap();



    Ok(())
}*/

/*
struct Multiplexor<T> {
    io: T,
    input: Sender<Vec<u8>>,
    output: Receiver<Vec<u8>>,
    await_offer: HashMap<u32, (Trigger, Sender<Vec<u8>>)>
}

impl<T: AsyncWrite + AsyncRead + Unpin> Multiplexor<T> {
    fn new(io: T) -> Self {

        let (tx, rx) = channel::unbounded();

        Self {
            io,
            input: tx,
            output: rx,
            await_offer: HashMap::new()
        }
    }

    async fn offer(&mut self) {
        let id: u32 = 1;

        let mut buf = Vec::new();
        buf.push(3);
        buf.push(ControlCode::Offer.as_byte());
        buf.extend(&id.to_be_bytes());

        let (trigger, listener) = triggered::trigger();
        self.await_offer.insert(id, (trigger, self.input.clone()));



        self.io.write_all(&buf).await.unwrap();

        listener.await;
    }
}

type StreamId = u32;

struct MultiplexingStream {
    input: Sender<Vec<u8>>,
    output: Receiver<Vec<u8>>,
    output_buf: VecDeque<u8>,
}

impl MultiplexingStream {
    fn new(input: Sender<Vec<u8>>, output: Receiver<Vec<u8>>) -> Self {
        Self {
            input,
            output,
            output_buf: VecDeque::new()
        }
    }
}

impl AsyncWrite for MultiplexingStream {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &[u8],
    ) -> task::Poll<Result<usize>> {
        let sBuf = buf.to_vec();

        let result = futures::ready!(self.input.send(sBuf).poll_unpin(cx));

        task::Poll::Ready(match result {
            Ok(_) => Ok(buf.len()),
            Err(e) => Err(Error::new(ErrorKind::Other, e)),
        })
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> task::Poll<Result<()>> {
        task::Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> task::Poll<Result<()>> {
        task::Poll::Ready(match self.input.close() {
            true => Ok(()),
            false => Err(Error::new(ErrorKind::Other, "can't close channel")),
        })
    }
}

impl AsyncRead for MultiplexingStream {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut [u8],
    ) -> task::Poll<Result<usize>> {

        let this = self.get_mut();

        if !this.output_buf.is_empty() {
            let drain_size = if this.output_buf.len() > buf.len() {
                buf.len()
            } else {
                this.output_buf.len()
            };

            let chunk: Vec<_> = this.output_buf.drain(0..drain_size).collect();
            buf[0..chunk.len()].copy_from_slice(&chunk);

            return task::Poll::Ready(Ok(chunk.len()));
        }

        let result = futures::ready!(this.output.recv().poll_unpin(cx));
        match result {
            Ok(received) => {
                if received.len() <= buf.len() {
                    buf.copy_from_slice(&received);

                    task::Poll::Ready(Ok(received.len()))//
                } else {
                    buf.copy_from_slice(&received[..buf.len()]);
                    this.output_buf.extend(&received[buf.len()..]);

                    task::Poll::Ready(Ok(buf.len()))//
                }
            },
            Err(e) => task::Poll::Ready(Err(Error::new(ErrorKind::Other, e))),
        }
    }
}
*/
