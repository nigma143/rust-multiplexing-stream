use std::{
    convert::TryInto,
    io::Result,
    io::{Read, Write},
    vec,
};

use rmp_futures::decode::ValueFuture;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
pub enum StreamId {
    Local(u64),
    Remote(u64),
}

impl StreamId {
    fn flip(self) -> Self {
        match self {
            StreamId::Local(id) => StreamId::Remote(id),
            StreamId::Remote(id) => StreamId::Local(id),
        }
    }
}

#[derive(Debug)]
pub enum Frame {
    Offer {
        id: StreamId,
        name: String,
        window_size: Option<u64>,
    },
    OfferAccepted {
        id: StreamId,
        rem_window_size: Option<u64>,
    },
    Content {
        id: StreamId,
        payload: Vec<u8>,
    },
    ContentProcessed {
        id: StreamId,
        processed: i64,
    },
    ContentWritingCompleted {
        id: StreamId,
    },
    ChannelTerminated {
        id: StreamId,
    },
}

pub async fn write_frame<W: AsyncWrite + Unpin>(write: W, value: Frame) -> Result<()> {
    /*println!(
        "write code {}",
        match value {
            Frame::Offer {
                id,
                ref name,
                window_size,
            } => "0",
            Frame::OfferAccepted {
                id,
                rem_window_size,
            } => "1",
            Frame::Content { id, ref payload } => "2",
            Frame::ContentProcessed { id, processed } => "5",
            Frame::ContentWritingCompleted { id } => "3",
            Frame::ChannelTerminated { id } => "4",
        }
    );*/
    let write = write.compat_write();
    match value {
        Frame::Offer {
            id,
            name,
            window_size,
        } => {
            let (id, source) = match id {
                StreamId::Local(id) => (id, 1),
                StreamId::Remote(id) => (id, -1),
            };
            let writer = rmp_futures::encode::MsgPackWriter::new(write);
            let writer = writer
                .write_array_len(4)
                .await?
                .next()
                .write_int(0)
                .await?
                .next()
                .write_int(id)
                .await?
                .next()
                .write_int(source)
                .await?
                .next();

            let mut payload = vec![];
            let payload_writer = rmp_futures::encode::MsgPackWriter::new(&mut payload);

            match window_size {
                Some(window_size) => {
                    payload_writer
                        .write_array_len(2)
                        .await?
                        .next()
                        .write_str(&name)
                        .await?
                        .next()
                        .write_int(window_size)
                        .await?;
                }
                None => {
                    payload_writer
                        .write_array_len(1)
                        .await?
                        .next()
                        .write_str(&name)
                        .await?;
                }
            }

            writer.write_bin(&payload[..]).await?;
        }
        Frame::OfferAccepted {
            id,
            rem_window_size,
        } => {
            let (id, source) = match id {
                StreamId::Local(id) => (id, 1),
                StreamId::Remote(id) => (id, -1),
            };
            let writer = rmp_futures::encode::MsgPackWriter::new(write);
            let writer = writer
                .write_array_len(4)
                .await?
                .next()
                .write_int(1)
                .await?
                .next()
                .write_int(id)
                .await?
                .next()
                .write_int(source)
                .await?
                .next();

            let mut payload = vec![];
            let payload_writer = rmp_futures::encode::MsgPackWriter::new(&mut payload);

            match rem_window_size {
                Some(rem_window_size) => {
                    payload_writer
                        .write_array_len(1)
                        .await?
                        .next()
                        .write_int(rem_window_size)
                        .await?;
                }
                None => {
                    payload_writer.write_array_len(0).await?;
                }
            }

            writer.write_bin(&payload[..]).await?;
        }
        Frame::Content { id, payload } => {
            let (id, source) = match id {
                StreamId::Local(id) => (id, 1),
                StreamId::Remote(id) => (id, -1),
            };
            let writer = rmp_futures::encode::MsgPackWriter::new(write);
            writer
                .write_array_len(4)
                .await?
                .next()
                .write_int(2)
                .await?
                .next()
                .write_int(id)
                .await?
                .next()
                .write_int(source)
                .await?
                .next()
                .write_bin(&payload[..])
                .await?;
        }
        Frame::ContentProcessed { id, processed } => {
            let (id, source) = match id {
                StreamId::Local(id) => (id, 1),
                StreamId::Remote(id) => (id, -1),
            };
            let writer = rmp_futures::encode::MsgPackWriter::new(write);
            let writer = writer
                .write_array_len(4)
                .await?
                .next()
                .write_int(5)
                .await?
                .next()
                .write_int(id)
                .await?
                .next()
                .write_int(source)
                .await?
                .next();

            let mut payload = vec![];
            rmp_futures::encode::MsgPackWriter::new(&mut payload)
                .write_array_len(1)
                .await?
                .next()
                .write_int(processed)
                .await?;

            writer.write_bin(&payload[..]).await?;
        }
        Frame::ContentWritingCompleted { id } => {
            let (id, source) = match id {
                StreamId::Local(id) => (id, 1),
                StreamId::Remote(id) => (id, -1),
            };
            let writer = rmp_futures::encode::MsgPackWriter::new(write);
            writer
                .write_array_len(3)
                .await?
                .next()
                .write_int(3)
                .await?
                .next()
                .write_int(id)
                .await?
                .next()
                .write_int(source)
                .await?;
        }
        Frame::ChannelTerminated { id } => {
            let (id, source) = match id {
                StreamId::Local(id) => (id, 1),
                StreamId::Remote(id) => (id, -1),
            };
            let writer = rmp_futures::encode::MsgPackWriter::new(write);
            writer
                .write_array_len(3)
                .await?
                .next()
                .write_int(4)
                .await?
                .next()
                .write_int(id)
                .await?
                .next()
                .write_int(source)
                .await?;
        }
    }
    Ok(())
}

pub async fn read_frame<R: AsyncRead + Unpin + Send>(mut read: R) -> Result<Frame> {
    let read = read.compat();
    let reader = rmp_futures::decode::MsgPackFuture::new(read);
    let reader = reader.decode().await?.into_array().unwrap();
    let (code, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
    //println!("read code {}", code);
    let frame = match code {
        0 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            let (source, reader) =
                if let ValueFuture::Integer(val, r) = reader.next().unwrap().decode().await? {
                    (val.as_i64().unwrap(), r)
                } else {
                    todo!();
                };

            let (payload, reader) = reader
                .next()
                .unwrap()
                .decode()
                .await?
                .into_bin()
                .unwrap()
                .into_vec()
                .await?;

            let reader = rmp_futures::decode::MsgPackFuture::new(&*payload)
                .decode()
                .await
                .unwrap()
                .into_array()
                .unwrap();

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
                let (rem_window_size, reader) =
                    reader.next().unwrap().decode().await?.into_u64().unwrap();
                Some(rem_window_size)
            } else {
                None
            };

            let id = match source {
                -1 => StreamId::Remote(s_id),
                1 => StreamId::Local(s_id),
                _ => todo!(),
            }
            .flip();

            Frame::Offer {
                id,
                name,
                window_size,
            }
        }
        1 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            let (source, reader) =
                if let ValueFuture::Integer(val, r) = reader.next().unwrap().decode().await? {
                    (val.as_i64().unwrap(), r)
                } else {
                    todo!();
                };

            let (payload, reader) = reader
                .next()
                .unwrap()
                .decode()
                .await?
                .into_bin()
                .unwrap()
                .into_vec()
                .await?;

            let reader = rmp_futures::decode::MsgPackFuture::new(&*payload)
                .decode()
                .await
                .unwrap()
                .into_array()
                .unwrap();

            let rem_window_size = if reader.len() > 0 {
                let (rem_window_size, reader) =
                    reader.next().unwrap().decode().await?.into_u64().unwrap();
                Some(rem_window_size)
            } else {
                None
            };

            let id = match source {
                -1 => StreamId::Remote(s_id),
                1 => StreamId::Local(s_id),
                _ => todo!(),
            }
            .flip();

            Frame::OfferAccepted {
                id,
                rem_window_size,
            }
        }
        2 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
            let (source, reader) =
                if let ValueFuture::Integer(val, r) = reader.next().unwrap().decode().await? {
                    (val.as_i64().unwrap(), r)
                } else {
                    todo!();
                };

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

            let id = match source {
                -1 => StreamId::Remote(s_id),
                1 => StreamId::Local(s_id),
                _ => todo!(),
            }
            .flip();

            Frame::Content { id, payload }
        }
        5 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            let (source, reader) =
                if let ValueFuture::Integer(val, r) = reader.next().unwrap().decode().await? {
                    (val.as_i64().unwrap(), r)
                } else {
                    todo!();
                };

            let (payload, reader) = reader
                .next()
                .unwrap()
                .decode()
                .await?
                .into_bin()
                .unwrap()
                .into_vec()
                .await?;

            let reader = rmp_futures::decode::MsgPackFuture::new(&*payload)
                .decode()
                .await
                .unwrap()
                .into_array()
                .unwrap();

            let (processed, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            let id = match source {
                -1 => StreamId::Remote(s_id),
                1 => StreamId::Local(s_id),
                _ => todo!(),
            }
            .flip();

            Frame::ContentProcessed {
                id,
                processed: processed as i64,
            }
        }
        3 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            let (source, reader) =
                if let ValueFuture::Integer(val, r) = reader.next().unwrap().decode().await? {
                    (val.as_i64().unwrap(), r)
                } else {
                    todo!();
                };

            let id = match source {
                -1 => StreamId::Remote(s_id),
                1 => StreamId::Local(s_id),
                _ => todo!(),
            }
            .flip();

            Frame::ContentWritingCompleted { id }
        }
        4 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            let (source, reader) =
                if let ValueFuture::Integer(val, r) = reader.next().unwrap().decode().await? {
                    (val.as_i64().unwrap(), r)
                } else {
                    todo!();
                };

            let id = match source {
                -1 => StreamId::Remote(s_id),
                1 => StreamId::Local(s_id),
                _ => todo!(),
            }
            .flip();

            Frame::ChannelTerminated { id }
        }

        _ => todo!(),
    };

    Ok(frame)
}

pub async fn sync_read_frame<R: AsyncRead + Unpin + Send>(mut read: R) -> Result<Frame> {
    let read = read.compat();
    let reader = rmp_futures::decode::MsgPackFuture::new(read);
    let reader = reader.decode().await?.into_array().unwrap();
    let (code, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
    //println!("read code {}", code);
    let frame = match code {
        0 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            let (source, reader) =
                if let ValueFuture::Integer(val, r) = reader.next().unwrap().decode().await? {
                    (val.as_i64().unwrap(), r)
                } else {
                    todo!();
                };
                
            let (payload, reader) = reader
                .next()
                .unwrap()
                .decode()
                .await?
                .into_bin()
                .unwrap()
                .into_vec()
                .await?;

            let reader = rmp_futures::decode::MsgPackFuture::new(&*payload)
                .decode()
                .await
                .unwrap()
                .into_array()
                .unwrap();

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
                let (rem_window_size, reader) =
                    reader.next().unwrap().decode().await?.into_u64().unwrap();
                Some(rem_window_size)
            } else {
                None
            };

            let id = match source {
                -1 => StreamId::Remote(s_id),
                1 => StreamId::Local(s_id),
                _ => todo!(),
            }
            .flip();

            Frame::Offer {
                id,
                name,
                window_size,
            }
        }
        1 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            let (source, reader) =
                if let ValueFuture::Integer(val, r) = reader.next().unwrap().decode().await? {
                    (val.as_i64().unwrap(), r)
                } else {
                    todo!();
                };

            let (payload, reader) = reader
                .next()
                .unwrap()
                .decode()
                .await?
                .into_bin()
                .unwrap()
                .into_vec()
                .await?;

            let reader = rmp_futures::decode::MsgPackFuture::new(&*payload)
                .decode()
                .await
                .unwrap()
                .into_array()
                .unwrap();

            let rem_window_size = if reader.len() > 0 {
                let (rem_window_size, reader) =
                    reader.next().unwrap().decode().await?.into_u64().unwrap();
                Some(rem_window_size)
            } else {
                None
            };

            let id = match source {
                -1 => StreamId::Remote(s_id),
                1 => StreamId::Local(s_id),
                _ => todo!(),
            }
            .flip();

            Frame::OfferAccepted {
                id,
                rem_window_size,
            }
        }
        2 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();
            let (source, reader) =
                if let ValueFuture::Integer(val, r) = reader.next().unwrap().decode().await? {
                    (val.as_i64().unwrap(), r)
                } else {
                    todo!();
                };

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

            let id = match source {
                -1 => StreamId::Remote(s_id),
                1 => StreamId::Local(s_id),
                _ => todo!(),
            }
            .flip();

            Frame::Content { id, payload }
        }
        5 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            let (source, reader) =
                if let ValueFuture::Integer(val, r) = reader.next().unwrap().decode().await? {
                    (val.as_i64().unwrap(), r)
                } else {
                    todo!();
                };

            let (payload, reader) = reader
                .next()
                .unwrap()
                .decode()
                .await?
                .into_bin()
                .unwrap()
                .into_vec()
                .await?;

            let reader = rmp_futures::decode::MsgPackFuture::new(&*payload)
                .decode()
                .await
                .unwrap()
                .into_array()
                .unwrap();

            let (processed, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            let id = match source {
                -1 => StreamId::Remote(s_id),
                1 => StreamId::Local(s_id),
                _ => todo!(),
            }
            .flip();

            Frame::ContentProcessed {
                id,
                processed: processed as i64,
            }
        }
        3 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            let (source, reader) =
                if let ValueFuture::Integer(val, r) = reader.next().unwrap().decode().await? {
                    (val.as_i64().unwrap(), r)
                } else {
                    todo!();
                };

            let id = match source {
                -1 => StreamId::Remote(s_id),
                1 => StreamId::Local(s_id),
                _ => todo!(),
            }
            .flip();

            Frame::ContentWritingCompleted { id }
        }
        4 => {
            let (s_id, reader) = reader.next().unwrap().decode().await?.into_u64().unwrap();

            let (source, reader) =
                if let ValueFuture::Integer(val, r) = reader.next().unwrap().decode().await? {
                    (val.as_i64().unwrap(), r)
                } else {
                    todo!();
                };

            let id = match source {
                -1 => StreamId::Remote(s_id),
                1 => StreamId::Local(s_id),
                _ => todo!(),
            }
            .flip();

            Frame::ChannelTerminated { id }
        }

        _ => todo!(),
    };

    Ok(frame)
}


pub async fn sync_write_frame<W: AsyncWrite + Unpin>(mut write: W, value: Frame) -> Result<()> {
    match value {
        Frame::Offer {
            id,
            name,
            window_size,
        } => {
            let (id, source) = match id {
                StreamId::Local(id) => (id, 1),
                StreamId::Remote(id) => (id, -1),
            };

            let mut buf = vec![];

            rmp::encode::write_array_len(&mut buf, 4)?;
            rmp::encode::write_i32(&mut buf, 0)?;
            rmp::encode::write_u64(&mut buf, id)?;
            rmp::encode::write_i8(&mut buf, source)?;

            let mut payload = vec![];

            match window_size {
                Some(window_size) => {
                    rmp::encode::write_array_len(&mut payload, 2)?;
                    rmp::encode::write_str(&mut payload, &name)?;
                    rmp::encode::write_u64(&mut payload, window_size)?;
                }
                None => {
                    rmp::encode::write_array_len(&mut payload, 1)?;
                    rmp::encode::write_str(&mut payload, &name)?;
                }
            }
            rmp::encode::write_bin(&mut buf, &payload)?;

            tokio::io::AsyncWriteExt::write_all(&mut write, &buf).await?;
        }
        Frame::OfferAccepted {
            id,
            rem_window_size,
        } => {
            let (id, source) = match id {
                StreamId::Local(id) => (id, 1),
                StreamId::Remote(id) => (id, -1),
            };

            let mut buf = vec![];

            rmp::encode::write_array_len(&mut buf, 4)?;
            rmp::encode::write_i32(&mut buf, 1)?;
            rmp::encode::write_u64(&mut buf, id)?;
            rmp::encode::write_i8(&mut buf, source)?;

            let mut payload = vec![];

            match rem_window_size {
                Some(rem_window_size) => {
                    rmp::encode::write_array_len(&mut payload, 1)?;
                    rmp::encode::write_u64(&mut payload, rem_window_size)?;
                }
                None => {
                    rmp::encode::write_array_len(&mut payload, 0)?;
                }
            }
            rmp::encode::write_bin(&mut buf, &payload)?;

            tokio::io::AsyncWriteExt::write_all(&mut write, &buf).await?;
        }
        Frame::Content { id, payload } => {
            let (id, source) = match id {
                StreamId::Local(id) => (id, 1),
                StreamId::Remote(id) => (id, -1),
            };

            let mut buf = vec![];

            rmp::encode::write_array_len(&mut buf, 4)?;
            rmp::encode::write_i32(&mut buf, 2)?;
            rmp::encode::write_u64(&mut buf, id)?;
            rmp::encode::write_i8(&mut buf, source)?;
            rmp::encode::write_bin(&mut buf, &payload)?;

            tokio::io::AsyncWriteExt::write_all(&mut write, &buf).await?;
        }
        Frame::ContentProcessed { id, processed } => {
            let (id, source) = match id {
                StreamId::Local(id) => (id, 1),
                StreamId::Remote(id) => (id, -1),
            };

            let mut buf = vec![];

            rmp::encode::write_array_len(&mut buf, 4)?;
            rmp::encode::write_i32(&mut buf, 5)?;
            rmp::encode::write_u64(&mut buf, id)?;
            rmp::encode::write_i8(&mut buf, source)?;

            let mut payload = vec![];

            rmp::encode::write_array_len(&mut payload, 1)?;
            rmp::encode::write_i64(&mut payload, processed)?;

            rmp::encode::write_bin(&mut buf, &payload)?;

            tokio::io::AsyncWriteExt::write_all(&mut write, &buf).await?;
        }
        Frame::ContentWritingCompleted { id } => {
            let (id, source) = match id {
                StreamId::Local(id) => (id, 1),
                StreamId::Remote(id) => (id, -1),
            };

            let mut buf = vec![];

            rmp::encode::write_array_len(&mut buf, 3)?;
            rmp::encode::write_i32(&mut buf, 3)?;
            rmp::encode::write_u64(&mut buf, id)?;
            rmp::encode::write_i8(&mut buf, source)?;

            tokio::io::AsyncWriteExt::write_all(&mut write, &buf).await?;
        }
        Frame::ChannelTerminated { id } => {
            let (id, source) = match id {
                StreamId::Local(id) => (id, 1),
                StreamId::Remote(id) => (id, -1),
            };

            let mut buf = vec![];

            rmp::encode::write_array_len(&mut buf, 3)?;
            rmp::encode::write_i32(&mut buf, 4)?;
            rmp::encode::write_u64(&mut buf, id)?;
            rmp::encode::write_i8(&mut buf, source)?;

            tokio::io::AsyncWriteExt::write_all(&mut write, &buf).await?;
        }
    }
    Ok(())
}
