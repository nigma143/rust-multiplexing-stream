use std::{
    convert::TryInto,
    io::Result,
    io::{Read, Write},
    vec,
};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

pub type StreamId = u64;

#[derive(Debug)]
pub enum Frame {
    Offer {
        s_id: StreamId,
        name: String,
        window_size: Option<u64>,
    },
    OfferAccepted {
        s_id: StreamId,
        window_size: Option<u64>,
    },
    Content {
        s_id: StreamId,
        payload: Vec<u8>,
    },
    ContentProcessed {
        s_id: StreamId,
        processed: i64,
    },
    ContentWritingCompleted {
        s_id: StreamId,
    },
    ChannelTerminated {
        s_id: StreamId,
    },
}

pub async fn write_frame<W: AsyncWrite + Unpin>(write: W, value: Frame) -> Result<()> {
    println!("write");
    let write = write.compat_write();
    match value {
        Frame::Offer {
            s_id,
            name,
            window_size,
        } => {
            let writer = rmp_futures::encode::MsgPackWriter::new(write);
            let writer = writer
                .write_array_len(4)
                .await?
                .next()
                .write_int(0)
                .await?
                .next()
                .write_int(s_id)
                .await?
                .next()
                .write_int(1)
                .await?
                .next();

            match window_size {
                Some(s) => {
                    writer
                        .write_array_len(2)
                        .await?
                        .next()
                        .write_str(&name)
                        .await?
                        .next()
                        .write_int(s)
                        .await?;
                }
                None => {
                    writer
                        .write_array_len(1)
                        .await?
                        .next()
                        .write_str(&name)
                        .await?;
                }
            }
        }
        Frame::OfferAccepted { s_id, window_size } => {
            let writer = rmp_futures::encode::MsgPackWriter::new(write);
            let writer = writer
                .write_array_len(4)
                .await?
                .next()
                .write_int(1)
                .await?
                .next()
                .write_int(s_id)
                .await?
                .next()
                .write_int(1)
                .await?
                .next();
            match window_size {
                Some(s) => {
                    writer.write_array_len(1).await?.next().write_int(s).await?;
                }
                None => {
                    writer.write_array_len(0).await?;
                }
            }
        }
        Frame::Content { s_id, payload } => {
            let writer = rmp_futures::encode::MsgPackWriter::new(write);
            writer
                .write_array_len(4)
                .await?
                .next()
                .write_int(2)
                .await?
                .next()
                .write_int(s_id)
                .await?
                .next()
                .write_int(1)
                .await?
                .next()
                .write_bin(&payload[..])
                .await?;
        }
        Frame::ContentProcessed { s_id, processed } => {
            let writer = rmp_futures::encode::MsgPackWriter::new(write);
            writer
                .write_array_len(4)
                .await?
                .next()
                .write_int(5)
                .await?
                .next()
                .write_int(s_id)
                .await?
                .next()
                .write_int(1)
                .await?
                .next()
                .write_array_len(1)
                .await?
                .next()
                .write_int(processed)
                .await?;
        }
        Frame::ContentWritingCompleted { s_id } => {
            let writer = rmp_futures::encode::MsgPackWriter::new(write);
            writer
                .write_array_len(3)
                .await?
                .next()
                .write_int(3)
                .await?
                .next()
                .write_int(s_id)
                .await?
                .next()
                .write_int(1)
                .await?;
        }
        Frame::ChannelTerminated { s_id } => {
            let writer = rmp_futures::encode::MsgPackWriter::new(write);
            writer
                .write_array_len(3)
                .await?
                .next()
                .write_int(4)
                .await?
                .next()
                .write_int(s_id)
                .await?
                .next()
                .write_int(1)
                .await?;
        }
    }
    println!("writeE");
    Ok(())
}

pub async fn read_frame<R: AsyncRead + Unpin + Send>(mut read: R) -> Result<Frame> {
    let read = read.compat();

    let reader = rmp_futures::decode::MsgPackFuture::new(read);
    let reader = reader.decode().await?.into_array().unwrap();
    let (code, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
    println!("s4");
    println!("{}", code);
    let frame = match code {
        0 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
            let (source, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
            let reader = reader.next().unwrap().decode().await?.into_array().unwrap();
            let (name, reader) = reader
                .next()
                .unwrap()
                .decode()
                .await?
                .into_string()
                .unwrap()
                .into_string()
                .await
                .unwrap();

            let window_size = if reader.len() > 0 {
                let (window_size, reader) =
                    reader.next().unwrap().decode().await?.into_u64().unwrap();
                Some(window_size)
            } else {
                None
            };

            Frame::Offer {
                s_id,
                name,
                window_size,
            }
        }
        1 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
            let (source, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
            let reader = reader.next().unwrap().decode().await?.into_array().unwrap();

            let window_size = if reader.len() > 0 {
                let (window_size, reader) =
                    reader.next().unwrap().decode().await?.into_u64().unwrap();
                Some(window_size)
            } else {
                None
            };

            Frame::OfferAccepted { s_id, window_size }
        }
        2 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
            let (source, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
            let (payload, reader) = reader
                .next()
                .unwrap()
                .decode()
                .await?
                .into_bin()
                .unwrap()
                .into_vec()
                .await
                .unwrap();

            Frame::Content { s_id, payload }
        }
        5 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
            let (source, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
            let reader = reader.next().unwrap().decode().await?.into_array().unwrap();
            let (processed, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            Frame::ContentProcessed {
                s_id,
                processed: processed as i64,
            }
        }
        3 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
            let (source, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            Frame::ContentWritingCompleted { s_id }
        }
        4 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
            let (source, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            Frame::ChannelTerminated { s_id }
        }

        _ => todo!(),
    };

    Ok(frame)

    /*let mut buf = [0; 1];
    read.read_exact(&mut buf)?;

    let mut buf = vec![0; buf[0] as usize];

    read.read_exact(&mut buf);

    Ok(buf.into())*/
}
