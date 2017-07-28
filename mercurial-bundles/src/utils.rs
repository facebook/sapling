// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Utilities for decoding bundles.

use std::ops::Deref;
use std::str;

use byteorder::{BigEndian, ByteOrder};
use bytes::{Bytes, BytesMut};

use async_compression::{CompressorType, DecompressorType};
use mercurial_types::{NodeHash, Path};

use errors::*;

pub trait SplitTo {
    fn split_to(&mut self, at: usize) -> Self;
}

impl SplitTo for Bytes {
    #[inline]
    fn split_to(&mut self, at: usize) -> Self {
        Bytes::split_to(self, at)
    }
}

impl SplitTo for BytesMut {
    #[inline]
    fn split_to(&mut self, at: usize) -> Self {
        BytesMut::split_to(self, at)
    }
}

pub trait BytesExt {
    fn drain_u8(&mut self) -> u8;
    fn drain_u32(&mut self) -> u32;
    fn drain_i32(&mut self) -> i32;
    fn drain_str(&mut self, len: usize) -> Result<String>;
    fn drain_path(&mut self, len: usize) -> Result<Path>;
    fn drain_node(&mut self) -> NodeHash;
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
    fn drain_u32(&mut self) -> u32 {
        BigEndian::read_u32(self.split_to(4).as_ref())
    }

    #[inline]
    fn drain_i32(&mut self) -> i32 {
        BigEndian::read_i32(self.split_to(4).as_ref())
    }

    #[inline]
    fn drain_str(&mut self, len: usize) -> Result<String> {
        Ok(
            str::from_utf8(self.split_to(len).as_ref())
                .chain_err(|| "invalid UTF-8")?
                .into(),
        )
    }

    #[inline]
    fn drain_path(&mut self, len: usize) -> Result<Path> {
        Path::new(self.split_to(len)).chain_err(|| "invalid path")
    }

    #[inline]
    fn drain_node(&mut self) -> NodeHash {
        // This only fails if the size of input passed in isn't 20
        // bytes. drain_to would have panicked in that case anyway.
        NodeHash::from_bytes(self.split_to(20).as_ref()).unwrap()
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

pub fn get_decompressor_type(compression: Option<&str>) -> Result<DecompressorType> {
    match compression {
        Some("BZ") => Ok(DecompressorType::Bzip2),
        Some("GZ") => Ok(DecompressorType::Gzip),
        Some("ZS") => Ok(DecompressorType::Zstd),
        Some("UN") => Ok(DecompressorType::Uncompressed),
        Some(s) => {
            bail!(ErrorKind::Bundle2Decode(
                format!("unknown compression '{}'", s)
            ))
        }
        None => Ok(DecompressorType::Uncompressed),
    }
}

pub fn get_compression_param(ct: &CompressorType) -> &'static str {
    match ct {
        &CompressorType::Bzip2(_) => "BZ",
        &CompressorType::Gzip => "GZ",
        &CompressorType::Zstd { .. } => "ZS",
        &CompressorType::Uncompressed => "UN",
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

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_is_mandatory_param() {
        let f = |x: &str| is_mandatory_param(x.into());

        assert!(f("Foo").unwrap());
        assert!(!f("bar").unwrap());
        assert_matches!(f("").unwrap_err().kind(),
                        &ErrorKind::Msg(ref msg) if msg == "string is empty");
        assert_matches!(f("123").unwrap_err().kind(),
                        &ErrorKind::Msg(ref msg)
                        if msg == "'123': first char '1' is not alphabetic");
    }
}
