/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

// Codecs related to infinitepush also known as Commit Cloud.

use std::io::Cursor;

use anyhow::{bail, Error, Result};
use byteorder::ReadBytesExt;
use bytes_old::{Bytes, BytesMut};
use mercurial_mutation::HgMutationEntry;
use tokio_io::codec::Decoder;
use vlqencoding::VLQDecode;

use crate::utils::BytesExt;

#[derive(Debug)]
pub struct InfinitepushBookmarksUnpacker {
    finished: bool,
    expected_len: Option<usize>,
}

impl InfinitepushBookmarksUnpacker {
    pub fn new() -> Self {
        Self {
            finished: false,
            expected_len: None,
        }
    }
}

impl Decoder for InfinitepushBookmarksUnpacker {
    type Item = Bytes;
    type Error = Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        if self.finished {
            return Ok(None);
        }
        match self.expected_len {
            Some(toread) => {
                if buf.len() < toread {
                    Ok(None)
                } else {
                    self.finished = true;
                    Ok(Some(buf.split_to(toread).freeze()))
                }
            }
            None => {
                if buf.len() >= 4 {
                    self.expected_len = Some(buf.drain_u32() as usize);
                }
                Ok(None)
            }
        }
    }
}

#[derive(Debug)]
pub struct InfinitepushMutationUnpacker {}

impl InfinitepushMutationUnpacker {
    pub fn new() -> Self {
        Self {}
    }
}

/// Decoder for infinitepush mutation entries
///
/// This decoder decodes all entries in one operation, so needs to wait for eof.
impl Decoder for InfinitepushMutationUnpacker {
    type Item = Vec<HgMutationEntry>;
    type Error = Error;

    fn decode(&mut self, _buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        Ok(None)
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>> {
        let mut entries = Vec::new();
        let mut cursor = Cursor::new(buf);
        let version = cursor.read_u8()?;
        if version != 1 {
            bail!("Unsupported infinitepush mutation part format: {}", version);
        }
        let count = cursor.read_vlq()?;
        entries.reserve_exact(count);
        for _ in 0..count {
            let entry = HgMutationEntry::deserialize(&mut cursor)?;
            entries.push(entry);
        }
        let size = cursor.position();
        cursor.into_inner().advance(size as usize);
        Ok(Some(entries))
    }
}
