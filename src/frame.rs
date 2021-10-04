use std::{
    convert::TryInto,
    io::Result,
    io::{Read, Write},
    vec,
};

use futures::{AsyncReadExt, AsyncWriteExt};

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

pub fn sync_write_frame<W: Write>(mut write: W, value: Frame) -> Result<()> {
    match value {
        Frame::Offer {
            s_id,
            name,
            window_size,
        } => {
            rmp::encode::write_array_len(&mut write, 4)?;
            rmp::encode::write_i32(&mut write, 0)?;
            rmp::encode::write_u64(&mut write, s_id)?;
            rmp::encode::write_i8(&mut write, 1)?;
            match window_size {
                Some(s) => {
                    rmp::encode::write_array_len(&mut write, 2)?;
                    rmp::encode::write_str(&mut write, &name)?;
                    rmp::encode::write_u64(&mut write, s)?;
                }
                None => {
                    rmp::encode::write_array_len(&mut write, 1)?;
                    rmp::encode::write_str(&mut write, &name)?;
                }
            }
        }
        Frame::OfferAccepted { s_id, window_size } => {
            rmp::encode::write_array_len(&mut write, 4)?;
            rmp::encode::write_i32(&mut write, 1)?;
            rmp::encode::write_u64(&mut write, s_id)?;
            rmp::encode::write_i8(&mut write, 1)?;
            match window_size {
                Some(s) => {
                    rmp::encode::write_array_len(&mut write, 1)?;
                    rmp::encode::write_u64(&mut write, s)?;
                }
                None => {
                    rmp::encode::write_array_len(&mut write, 0)?;
                }
            }
        }
        Frame::Content { s_id, payload } => {
            rmp::encode::write_array_len(&mut write, 4)?;
            rmp::encode::write_i32(&mut write, 2)?;
            rmp::encode::write_u64(&mut write, s_id)?;
            rmp::encode::write_i8(&mut write, 1)?;
            rmp::encode::write_bin(&mut write, &payload[..])?;
        }
        Frame::ContentProcessed { s_id, processed } => {
            rmp::encode::write_array_len(&mut write, 4)?;
            rmp::encode::write_i32(&mut write, 5)?;
            rmp::encode::write_u64(&mut write, s_id)?;
            rmp::encode::write_i8(&mut write, 1)?;
            rmp::encode::write_array_len(&mut write, 1)?;
            rmp::encode::write_i64(&mut write, processed)?;
        }
        Frame::ContentWritingCompleted { s_id } => {
            rmp::encode::write_array_len(&mut write, 3)?;
            rmp::encode::write_i32(&mut write, 3)?;
            rmp::encode::write_u64(&mut write, s_id)?;
            rmp::encode::write_i8(&mut write, 1)?;
        }
        Frame::ChannelTerminated { s_id } => {
            rmp::encode::write_array_len(&mut write, 3)?;
            rmp::encode::write_i32(&mut write, 4)?;
            rmp::encode::write_u64(&mut write, s_id)?;
            rmp::encode::write_i8(&mut write, 1)?;
        }
    }

    Ok(())
}

pub fn sync_read_frame<R: Read>(mut read: R) -> Result<Frame> {
    let _ = rmp::decode::read_array_len(&mut read).unwrap();
    let code = rmp::decode::read_i32(&mut read).unwrap();

    let frame = match code {
        0 => {
            let s_id = rmp::decode::read_u64(&mut read).unwrap();
            let source = rmp::decode::read_i8(&mut read).unwrap();
            let offer_param_len = rmp::decode::read_array_len(&mut read).unwrap();

            let mut buf = vec![0; 128];
            let name = rmp::decode::read_str(&mut read, &mut buf).unwrap().into();

            let window_size = if offer_param_len == 2 {
                Some(rmp::decode::read_u64(&mut read).unwrap())
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
            let s_id = rmp::decode::read_u64(&mut read).unwrap();
            let source = rmp::decode::read_i8(&mut read).unwrap();
            let offer_param_len = rmp::decode::read_array_len(&mut read).unwrap();
            let window_size = if offer_param_len == 1 {
                Some(rmp::decode::read_u64(&mut read).unwrap())
            } else {
                None
            };

            Frame::OfferAccepted { s_id, window_size }
        }
        2 => {
            let s_id = rmp::decode::read_u64(&mut read).unwrap();
            let source = rmp::decode::read_i8(&mut read).unwrap();
            let payload_len = rmp::decode::read_bin_len(&mut read).unwrap() as usize;

            let mut payload: Vec<u8> = Vec::with_capacity(payload_len);
            unsafe {
                payload.set_len(payload_len);
            }

            read.read_exact(&mut payload)?;

            Frame::Content { s_id, payload }
        }
        5 => {
            let s_id = rmp::decode::read_u64(&mut read).unwrap();
            let source = rmp::decode::read_i8(&mut read).unwrap();
            let _ = rmp::decode::read_array_len(&mut read).unwrap();
            let processed = rmp::decode::read_i64(&mut read).unwrap();

            Frame::ContentProcessed { s_id, processed }
        }
        3 => {
            let s_id = rmp::decode::read_u64(&mut read).unwrap();
            let source = rmp::decode::read_i8(&mut read).unwrap();

            Frame::ContentWritingCompleted { s_id }
        }
        4 => {
            let s_id = rmp::decode::read_u64(&mut read).unwrap();
            let source = rmp::decode::read_i8(&mut read).unwrap();

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
