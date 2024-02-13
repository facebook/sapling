/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io;
use std::marker::Unpin;

use tokio::io::AsyncWrite;
use tokio::io::AsyncWriteExt;

use crate::Channel;
use crate::DELIMITER_LINE;
use crate::ERR_PREFIX;
use crate::FLUSH_LINE;
use crate::MAX_DATA_LEN;
use crate::RESPONSE_END_LINE;
use crate::U16_HEX_BYTES;

/// The error returned by most functions in this module
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Cannot encode more than {MAX_DATA_LEN} bytes, got {length_in_bytes}")]
    DataLengthLimitExceeded { length_in_bytes: usize },
    #[error("Empty lines are invalid")]
    DataIsEmpty,
}

pub(crate) fn u16_to_hex(value: u16) -> [u8; 4] {
    let mut buf = [0u8; 4];
    faster_hex::hex_encode(&value.to_be_bytes(), &mut buf)
        .expect("two bytes to 4 hex chars never fails");
    buf
}

/// Write a response-end message to `out`.
pub async fn response_end_to_write(mut out: impl AsyncWrite + Unpin) -> io::Result<usize> {
    out.write_all(RESPONSE_END_LINE).await.map(|_| 4)
}

/// Write a delim message to `out`.
pub async fn delim_to_write(mut out: impl AsyncWrite + Unpin) -> io::Result<usize> {
    out.write_all(DELIMITER_LINE).await.map(|_| 4)
}

/// Write a flush message to `out`.
pub async fn flush_to_write(mut out: impl AsyncWrite + Unpin) -> io::Result<usize> {
    out.write_all(FLUSH_LINE).await.map(|_| 4)
}

/// Write an error `message` to `out`.
pub async fn error_to_write(
    message: &[u8],
    out: &mut (impl AsyncWrite + Unpin),
) -> io::Result<usize> {
    prefixed_data_to_write(ERR_PREFIX, message, out).await
}

/// Write `data` of `kind` to `out` using side-band encoding.
pub async fn band_to_write(
    kind: Channel,
    data: &[u8],
    out: &mut (impl AsyncWrite + Unpin),
) -> io::Result<usize> {
    prefixed_data_to_write(&[kind as u8], data, out).await
}

/// Write a `data` message to `out`.
pub async fn data_to_write(data: &[u8], out: &mut (impl AsyncWrite + Unpin)) -> io::Result<usize> {
    prefixed_data_to_write(&[], data, out).await
}

/// Write a `text` message to `out`, which is assured to end in a newline.
pub async fn text_to_write(text: &[u8], out: &mut (impl AsyncWrite + Unpin)) -> io::Result<usize> {
    prefixed_and_suffixed_data_to_write(&[], text, &[b'\n'], out).await
}

/// Write text byte level data to `out` in packetline format
pub async fn write_text_packetline(
    buf: &[u8],
    out: &mut (impl AsyncWrite + Unpin),
) -> io::Result<usize> {
    write_packetline(buf, false /* is_binary */, None, out).await
}

/// Write binary byte level data to `out` in packetline format
pub async fn write_binary_packetline(
    buf: &[u8],
    out: &mut (impl AsyncWrite + Unpin),
) -> io::Result<usize> {
    write_packetline(buf, true /* is_binary */, None, out).await
}

/// Write binary byte level data to `out` in data channel with packetline format
pub async fn write_data_channel(
    buf: &[u8],
    out: &mut (impl AsyncWrite + Unpin),
) -> io::Result<usize> {
    write_packetline(buf, true /* is_binary */, Some(Channel::Data), out).await
}

async fn write_packetline(
    mut buf: &[u8],
    is_binary: bool,
    channel: Option<Channel>,
    out: &mut (impl AsyncWrite + Unpin),
) -> io::Result<usize> {
    if buf.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "empty packet lines are not permitted as '0004' is invalid",
        ));
    }
    let mut written = 0;
    while !buf.is_empty() {
        let (data, rest) = buf.split_at(buf.len().min(MAX_DATA_LEN));
        written += if is_binary {
            if let Some(channel) = channel {
                band_to_write(channel, data, out).await?
            } else {
                data_to_write(data, out).await?
            }
        } else {
            text_to_write(data, out).await?
        };
        // subtract header (and trailing NL) because write-all can't handle writing more than it passes in
        written -= U16_HEX_BYTES + usize::from(is_binary);
        buf = rest;
    }
    Ok(written)
}

async fn prefixed_data_to_write(
    prefix: &[u8],
    data: &[u8],
    out: &mut (impl AsyncWrite + Unpin),
) -> io::Result<usize> {
    prefixed_and_suffixed_data_to_write(prefix, data, &[], out).await
}

async fn prefixed_and_suffixed_data_to_write(
    prefix: &[u8],
    data: &[u8],
    suffix: &[u8],
    out: &mut (impl AsyncWrite + Unpin),
) -> io::Result<usize> {
    let data_len = prefix.len() + data.len() + suffix.len();
    if data_len > MAX_DATA_LEN {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            Error::DataLengthLimitExceeded {
                length_in_bytes: data_len,
            },
        ));
    }
    if data.is_empty() {
        return Err(io::Error::new(io::ErrorKind::Other, Error::DataIsEmpty));
    }

    let data_len = data_len + 4;
    let buf = u16_to_hex(data_len as u16);

    out.write_all(&buf).await?;
    if !prefix.is_empty() {
        out.write_all(prefix).await?;
    }
    out.write_all(data).await?;
    if !suffix.is_empty() {
        out.write_all(suffix).await?;
    }
    Ok(data_len)
}
