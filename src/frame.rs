use std::{
    io::Result,
    vec,
};

use rmp_futures::decode::ValueFuture;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::compat::{TokioAsyncReadCompatExt};

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

pub async fn read_frame<R: AsyncRead + Unpin>(mut read: R) -> Result<Frame> {
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

pub async fn write_frame<W: AsyncWrite + Unpin>(mut write: W, value: Frame) -> Result<()> {
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
