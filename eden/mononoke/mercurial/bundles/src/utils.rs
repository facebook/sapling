/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utilities for decoding bundles.

use std::io::Result as IoResult;
use std::pin::Pin;
use std::str;
use std::task::Context;
use std::task::Poll;

use anyhow::bail;
use anyhow::Context as _;
use anyhow::Result;
use async_compression::tokio::bufread::BzDecoder;
use async_compression::tokio::bufread::GzipDecoder;
use async_compression::tokio::bufread::ZstdDecoder;
use async_stream::try_stream;
use bytes::Bytes;
use bytes::BytesMut;
use futures::pin_mut;
use futures::Stream;
use futures::TryStreamExt;
use mercurial_types::HgNodeHash;
use mercurial_types::NonRootMPath;
use pin_project::pin_project;
use tokio::io::AsyncBufRead;
use tokio::io::AsyncRead;
use tokio::io::ReadBuf;
use tokio_util::codec::Decoder;

use crate::errors::ErrorKind;

pub trait BytesExt {
    fn get_str(&mut self, len: usize) -> Result<String>;
    fn get_path(&mut self, len: usize) -> Result<NonRootMPath>;
    fn get_node(&mut self) -> Result<HgNodeHash>;
}

impl BytesExt for Bytes {
    fn get_str(&mut self, len: usize) -> Result<String> {
        std::str::from_utf8(self.split_to(len).as_ref())
            .context("invalid UTF-8")
            .map(String::from)
    }
    fn get_path(&mut self, len: usize) -> Result<NonRootMPath> {
        NonRootMPath::new(self.split_to(len)).context("invalid path")
    }
    fn get_node(&mut self) -> Result<HgNodeHash> {
        // This only fails if the size of input passed in isn't 20 bytes.
        HgNodeHash::from_bytes(self.split_to(20).as_ref()).context("insufficient bytes in input")
    }
}

impl BytesExt for BytesMut {
    fn get_str(&mut self, len: usize) -> Result<String> {
        std::str::from_utf8(self.split_to(len).as_ref())
            .context("invalid UTF-8")
            .map(String::from)
    }
    fn get_path(&mut self, len: usize) -> Result<NonRootMPath> {
        NonRootMPath::new(self.split_to(len)).context("invalid path")
    }
    fn get_node(&mut self) -> Result<HgNodeHash> {
        // This only fails if the size of input passed in isn't 20 bytes.
        HgNodeHash::from_bytes(self.split_to(20).as_ref()).context("insufficient bytes in input")
    }
}

pub fn is_mandatory_param(s: &str) -> Result<bool> {
    match s.chars().next() {
        Some(ch) => {
            if !ch.is_alphabetic() {
                bail!("'{}': first char '{}' is not alphabetic", s, ch);
            }
            Ok(ch.is_uppercase())
        }
        None => bail!("string is empty"),
    }
}

#[pin_project(project = DecompressorProj)]
pub enum Decompressor<R: AsyncBufRead> {
    Uncompressed(#[pin] R),
    Bzip2(#[pin] BzDecoder<R>),
    Gzip(#[pin] GzipDecoder<R>),
    Zstd(#[pin] ZstdDecoder<R>),
}

impl<R: AsyncBufRead> Decompressor<R> {
    pub fn new(read: R, compression: Option<&str>) -> Result<Self> {
        match compression {
            Some("BZ") => Ok(Self::Bzip2(BzDecoder::new(read))),
            Some("GZ") => Ok(Self::Gzip(GzipDecoder::new(read))),
            Some("ZS") => Ok(Self::Zstd(ZstdDecoder::new(read))),
            Some("UN") | None => Ok(Self::Uncompressed(read)),
            Some(s) => bail!(ErrorKind::Bundle2Decode(format!(
                "unknown compression '{}'",
                s
            ),)),
        }
    }

    pub fn into_inner(self) -> R {
        match self {
            Self::Uncompressed(read) => read,
            Self::Bzip2(decoder) => decoder.into_inner(),
            Self::Gzip(decoder) => decoder.into_inner(),
            Self::Zstd(decoder) => decoder.into_inner(),
        }
    }
}

impl<R: AsyncBufRead> AsyncRead for Decompressor<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<IoResult<()>> {
        match self.project() {
            DecompressorProj::Uncompressed(r) => r.poll_read(cx, buf),
            DecompressorProj::Bzip2(r) => r.poll_read(cx, buf),
            DecompressorProj::Gzip(r) => r.poll_read(cx, buf),
            DecompressorProj::Zstd(r) => r.poll_read(cx, buf),
        }
    }
}

pub fn capitalize_first(s: String) -> String {
    // Capitalize Unicode style, since capitalizing a single code point can
    // produce multiple code points.
    // TODO: just enforce ASCII-only and make this simpler.
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(ch) => ch.to_uppercase().collect::<String>() + chars.as_str(),
    }
}

pub(crate) fn decode_stream<S, Dec, Err>(
    stream: S,
    mut decoder: Dec,
) -> impl Stream<Item = Result<Dec::Item, Dec::Error>>
where
    S: Stream<Item = Result<Bytes, Err>>,
    Dec: Decoder,
    Dec::Error: From<Err>,
{
    try_stream! {
        pin_mut!(stream);
        let mut buf = BytesMut::with_capacity(8 * 1024);
        while let Some(data) = stream.try_next().await? {
            buf.extend_from_slice(data.as_ref());
            while let Some(frame) = decoder.decode(&mut buf)? {
                yield frame;
            }
        }
        while !buf.is_empty() && let Some(frame) = decoder.decode_eof(&mut buf)? {
            yield frame;
        }
    }
}

#[cfg(test)]
mod test {
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn test_is_mandatory_param() {
        assert!(is_mandatory_param("Foo").unwrap());
        assert!(!is_mandatory_param("bar").unwrap());
        assert_eq!(
            format!("{}", is_mandatory_param("").unwrap_err()),
            "string is empty"
        );
        assert_eq!(
            format!("{}", is_mandatory_param("123").unwrap_err()),
            "'123': first char '1' is not alphabetic"
        );
    }
}
