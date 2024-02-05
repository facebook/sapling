/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Utilities for decoding bundles.

use std::ops::Deref;
use std::str;

use anyhow::bail;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_compression::DecompressorType;
use async_stream::try_stream;
use byteorder::BigEndian;
use byteorder::ByteOrder;
use bytes::Bytes;
use bytes::BytesMut;
use bytes_old::Bytes as BytesOld;
use bytes_old::BytesMut as BytesMutOld;
use futures::pin_mut;
use futures::Stream;
use futures::TryStreamExt;
use mercurial_types::HgNodeHash;
use mercurial_types::NonRootMPath;
use tokio_util::codec::Decoder;

use crate::errors::ErrorKind;

pub trait BytesNewExt {
    fn get_str(&mut self, len: usize) -> Result<String>;
    fn get_path(&mut self, len: usize) -> Result<NonRootMPath>;
    fn get_node(&mut self) -> Result<HgNodeHash>;
}

impl BytesNewExt for Bytes {
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

impl BytesNewExt for BytesMut {
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

pub trait SplitTo {
    fn split_to(&mut self, at: usize) -> Self;
}

impl SplitTo for BytesOld {
    #[inline]
    fn split_to(&mut self, at: usize) -> Self {
        BytesOld::split_to(self, at)
    }
}

impl SplitTo for BytesMutOld {
    #[inline]
    fn split_to(&mut self, at: usize) -> Self {
        BytesMutOld::split_to(self, at)
    }
}

pub trait BytesExt {
    fn drain_u8(&mut self) -> u8;
    fn drain_u16(&mut self) -> u16;
    fn drain_u32(&mut self) -> u32;
    fn drain_u64(&mut self) -> u64;
    fn drain_i32(&mut self) -> i32;
    fn drain_str(&mut self, len: usize) -> Result<String>;
    fn drain_path(&mut self, len: usize) -> Result<NonRootMPath>;
    fn drain_node(&mut self) -> HgNodeHash;
    fn peek_u16(&self) -> u16;
    fn peek_u32(&self) -> u32;
    fn peek_i32(&self) -> i32;
}

impl<T> BytesExt for T
where
    T: SplitTo + AsRef<[u8]> + Deref<Target = [u8]>,
{
    #[inline]
    fn drain_u8(&mut self) -> u8 {
        self.split_to(1)[0]
    }

    #[inline]
    fn drain_u16(&mut self) -> u16 {
        BigEndian::read_u16(self.split_to(2).as_ref())
    }

    #[inline]
    fn drain_u32(&mut self) -> u32 {
        BigEndian::read_u32(self.split_to(4).as_ref())
    }

    #[inline]
    fn drain_u64(&mut self) -> u64 {
        BigEndian::read_u64(self.split_to(8).as_ref())
    }

    #[inline]
    fn drain_i32(&mut self) -> i32 {
        BigEndian::read_i32(self.split_to(4).as_ref())
    }

    #[inline]
    fn drain_str(&mut self, len: usize) -> Result<String> {
        Ok(str::from_utf8(self.split_to(len).as_ref())
            .context("invalid UTF-8")?
            .into())
    }

    #[inline]
    fn drain_path(&mut self, len: usize) -> Result<NonRootMPath> {
        NonRootMPath::new(self.split_to(len))
            .context("invalid path")
            .map_err(Error::from)
    }

    #[inline]
    fn drain_node(&mut self) -> HgNodeHash {
        // This only fails if the size of input passed in isn't 20
        // bytes. drain_to would have panicked in that case anyway.
        HgNodeHash::from_bytes(self.split_to(20).as_ref()).unwrap()
    }

    #[inline]
    fn peek_u16(&self) -> u16 {
        BigEndian::read_u16(&self[..2])
    }

    #[inline]
    fn peek_u32(&self) -> u32 {
        BigEndian::read_u32(&self[..4])
    }

    #[inline]
    fn peek_i32(&self) -> i32 {
        BigEndian::read_i32(&self[..4])
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

pub fn get_decompressor_type(compression: Option<&str>) -> Result<Option<DecompressorType>> {
    match compression {
        Some("BZ") => Ok(Some(DecompressorType::Bzip2)),
        Some("GZ") => Ok(Some(DecompressorType::Gzip)),
        Some("ZS") => Ok(Some(DecompressorType::OverreadingZstd)),
        Some("UN") => Ok(None),
        Some(s) => bail!(ErrorKind::Bundle2Decode(format!(
            "unknown compression '{}'",
            s
        ),)),
        None => Ok(None),
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
    use super::*;

    #[test]
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
