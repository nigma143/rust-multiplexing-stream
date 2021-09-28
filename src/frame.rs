use std::{
    convert::TryInto,
    io::Result,
    io::{Read, Write},
    vec,
};

use futures::{AsyncReadExt, AsyncWriteExt};

pub type StreamId = u32;

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
    ContentWritingCompleted {
        s_id: StreamId,
    },
    //ChannelTerminated,
    //ContentProcessed,
}

impl Into<Vec<u8>> for Frame {
    fn into(self) -> Vec<u8> {
        match self {
            Frame::Offer {
                s_id,
                name,
                window_size,
            } => {
                let mut buf = vec![0x01];
                buf.extend(&s_id.to_be_bytes());
                buf.extend(name.as_bytes());
                buf
            }
            Frame::OfferAccepted { s_id, window_size } => {
                let mut buf = vec![0x02];
                buf.extend(&s_id.to_be_bytes());
                buf
            }
            Frame::Content { s_id, payload } => {
                let mut buf = vec![0x03];
                buf.extend(&s_id.to_be_bytes());
                buf.extend(payload);
                buf
            }
            Frame::ContentWritingCompleted { s_id } => {
                let mut buf = vec![0x02];
                buf.extend(&s_id.to_be_bytes());
                buf
            }
        }
    }
}

impl From<Vec<u8>> for Frame {
    fn from(buf: Vec<u8>) -> Self {
        match buf[0] {
            0x01 => Self::Offer {
                s_id: u32::from_be_bytes(buf[1..5].try_into().unwrap()),
                name: String::from_utf8(buf[5..].try_into().unwrap()).unwrap(),
                window_size: None,
            },
            0x02 => Self::OfferAccepted {
                s_id: u32::from_be_bytes(buf[1..5].try_into().unwrap()),
                window_size: None,
            },
            0x03 => Self::Content {
                s_id: u32::from_be_bytes(buf[1..5].try_into().unwrap()),
                payload: buf[5..].try_into().unwrap(),
            },
            0x04 => Self::ContentWritingCompleted {
                s_id: u32::from_be_bytes(buf[1..5].try_into().unwrap()),
            },
            _ => panic!("dsfds"),
        }
    }
}

pub async fn write_frame<W: AsyncWriteExt + Unpin>(mut write: W, value: Frame) -> Result<()> {
    let buf: Vec<u8> = value.into();
    write.write_all(&vec![buf.len() as u8; 1]).await?;
    write.write_all(&buf).await
}

pub async fn read_frame<R: AsyncReadExt + Unpin>(mut read: R) -> Result<Frame> {
    let mut buf = [0; 1];
    read.read_exact(&mut buf).await?;
    
    let mut buf = vec![0; buf[0] as usize];

    read.read_exact(&mut buf).await?;

    Ok(buf.into())
}
