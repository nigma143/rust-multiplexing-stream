use std::{io::Error, io::ErrorKind};

use async_std::channel::{Receiver, RecvError, SendError, Sender};
use futures::{future::BoxFuture, task, AsyncRead, AsyncWrite, FutureExt};

pub struct Reader<T> {
    output: Receiver<Vec<T>>,
    output_buf: Vec<T>,
    fut: Option<BoxFuture<'static, std::io::Result<usize>>>,
}

impl<T: Clone + Copy> Reader<T> {
    pub fn new(output: Receiver<Vec<T>>) -> Self {
        Self {
            output,
            output_buf: Vec::new(),
            fut: None,
        }
    }

    pub async fn recv(&mut self) -> Result<Vec<T>, RecvError> {
        if !self.output_buf.is_empty() {
            let buf = self.output_buf.clone();
            self.output_buf.clear();
            return Ok(buf);
        }

        self.output.recv().await
    }

    async fn read_wrap(&mut self, buf: &mut [T]) -> std::io::Result<usize> {
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

        let res = self.recv().await;
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

impl AsyncRead for Reader<u8> {
    fn poll_read(
        self: std::pin::Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut [u8],
    ) -> task::Poll<std::io::Result<usize>> {
        let this = unsafe { std::pin::Pin::into_inner_unchecked(self) };

        if this.fut.is_none() {
            let fut: BoxFuture<std::io::Result<usize>> = this.read_wrap(buf).boxed();
            let fut: BoxFuture<'static, std::io::Result<usize>> =
                unsafe { std::intrinsics::transmute(fut) };
            this.fut = Some(fut);
        }

        let poll = this.fut.as_mut().unwrap().poll_unpin(cx);

        if poll.is_ready() {
            this.fut = None;
        }

        poll
    }
}

pub struct Writer<T> {
    input: Sender<Vec<T>>,
    fut: Option<BoxFuture<'static, std::io::Result<usize>>>,
}

impl<T: Clone> Writer<T> {
    pub fn new(input: Sender<Vec<T>>) -> Self {
        Self { input, fut: None }
    }

    pub async fn send(&mut self, buf: Vec<T>) -> Result<(), SendError<Vec<T>>> {
        self.input.send(buf.to_vec()).await
    }

    async fn write_wrap(&mut self, buf: &[T]) -> std::io::Result<usize> {
        let res = self.send(buf.to_vec()).await;
        match res {
            Ok(_) => Ok(buf.len()),
            Err(e) => Err(Error::new(ErrorKind::Other, e.to_string())),
        }
    }
}

impl AsyncWrite for Writer<u8> {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &[u8],
    ) -> task::Poll<std::io::Result<usize>> {
        let this = unsafe { std::pin::Pin::into_inner_unchecked(self) };

        if this.fut.is_none() {
            let fut: BoxFuture<std::io::Result<usize>> = this.write_wrap(buf).boxed();
            let fut: BoxFuture<'static, std::io::Result<usize>> =
                unsafe { std::intrinsics::transmute(fut) };
            this.fut = Some(fut);
        }

        let poll = this.fut.as_mut().unwrap().poll_unpin(cx);

        if poll.is_ready() {
            this.fut = None;
        }

        poll
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut task::Context<'_>,
    ) -> task::Poll<std::io::Result<()>> {
        task::Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _: &mut task::Context<'_>,
    ) -> task::Poll<std::io::Result<()>> {
        task::Poll::Ready(match self.input.close() {
            true => Ok(()),
            false => Err(Error::new(ErrorKind::Other, "can't close channel")),
        })
    }
}
