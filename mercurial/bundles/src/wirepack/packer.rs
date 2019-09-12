// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Packing wirepacks to be sent over the wire during e.g. an hg pull.
//! The format is documented at
//! https://bitbucket.org/facebook/hg-experimental/src/@/remotefilelog/wirepack.py.

#![allow(deprecated)] // TODO: T29077977 convert from put_X::<BigEndian> -> put_X_be

use byteorder::BigEndian;
use bytes::BufMut;
use failure_ext::bail_err;
use futures::{Poll, Stream};

use crate::chunk::Chunk;
use mercurial_types::{MPath, RepoPath};

use super::converter::{WirePackConverter, WirePackPartProcessor};
use super::{DataEntry, HistoryEntry, Kind, Part, WIREPACK_END};

use crate::errors::*;

pub struct WirePackPacker<S> {
    stream: WirePackConverter<S, PackerProcessor>,
}

impl<S> WirePackPacker<S>
where
    S: Stream<Item = Part, Error = Error>,
{
    pub fn new(part_stream: S, kind: Kind) -> Self {
        Self {
            stream: WirePackConverter::new(part_stream, PackerProcessor { kind }),
        }
    }
}

impl<S> Stream for WirePackPacker<S>
where
    S: Stream<Item = Part, Error = Error>,
{
    type Item = Chunk;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Chunk>, Error> {
        self.stream.poll()
    }
}

struct PackerProcessor {
    kind: Kind,
}

unsafe impl Send for PackerProcessor {}
unsafe impl Sync for PackerProcessor {}

impl WirePackPartProcessor for PackerProcessor {
    type Data = Chunk;

    fn history_meta(&mut self, path: &RepoPath, entry_count: u32) -> Result<Option<Self::Data>> {
        let mut builder = ChunkBuilder::new(self.kind);
        builder.encode_filename(&path)?;
        builder.encode_entry_count(entry_count);
        Ok(Some(builder.build()?))
    }

    fn history(&mut self, entry: &HistoryEntry) -> Result<Option<Self::Data>> {
        let mut builder = ChunkBuilder::new(self.kind);
        builder.encode_history(entry)?;
        Ok(Some(builder.build()?))
    }

    fn data_meta(&mut self, _path: &RepoPath, entry_count: u32) -> Result<Option<Self::Data>> {
        let mut builder = ChunkBuilder::new(self.kind);
        builder.encode_entry_count(entry_count);
        Ok(Some(builder.build()?))
    }

    fn data(&mut self, data_entry: &DataEntry) -> Result<Option<Self::Data>> {
        let mut builder = ChunkBuilder::new(self.kind);
        builder.encode_data(&data_entry)?;
        Ok(Some(builder.build()?))
    }

    fn end(&mut self) -> Result<Option<Self::Data>> {
        let mut builder = ChunkBuilder::new(self.kind);
        builder.encode_end();
        Ok(Some(builder.build()?))
    }
}

#[derive(Debug)]
struct ChunkBuilder {
    kind: Kind,
    inner: Vec<u8>,
}

impl ChunkBuilder {
    pub fn new(kind: Kind) -> Self {
        Self {
            kind,
            inner: Vec::with_capacity(256),
        }
    }

    /// Encode a filename -- this should always happen before any history or data entries are
    /// encoded.
    fn encode_filename(&mut self, filename: &RepoPath) -> Result<&mut Self> {
        let mpath = match (self.kind, filename) {
            (Kind::Tree, &RepoPath::RootPath) => None,
            (Kind::File, &RepoPath::RootPath) => bail_err!(ErrorKind::WirePackEncode(
                "attempted to encode a zero-length filename into a file wirepack".into()
            )),
            (Kind::Tree, &RepoPath::DirectoryPath(ref dir_path)) => Some(verify_path(dir_path)?),
            (Kind::File, &RepoPath::FilePath(ref file_path)) => Some(verify_path(file_path)?),
            (kind, path) => bail_err!(ErrorKind::WirePackEncode(format!(
                "attempted to encode incompatible path into wirepack (kind: {}, path: {:?})",
                kind, path
            ))),
        };

        match mpath {
            Some(mpath) => {
                self.inner.put_u16::<BigEndian>(mpath.len() as u16);
                mpath.generate(&mut self.inner)?;
            }
            None => {
                self.inner.put_u16::<BigEndian>(0);
            }
        }
        Ok(self)
    }

    #[inline]
    fn encode_entry_count(&mut self, entry_count: u32) -> &mut Self {
        self.inner.put_u32::<BigEndian>(entry_count);
        self
    }

    #[inline]
    fn encode_history(&mut self, history_entry: &HistoryEntry) -> Result<&mut Self> {
        history_entry.encode(self.kind, &mut self.inner)?;
        Ok(self)
    }

    #[inline]
    fn encode_data(&mut self, data_entry: &DataEntry) -> Result<&mut Self> {
        data_entry.encode(&mut self.inner)?;
        Ok(self)
    }

    #[inline]
    fn encode_end(&mut self) -> &mut Self {
        self.inner.put_slice(WIREPACK_END);
        self
    }

    #[inline]
    fn build(self) -> Result<Chunk> {
        Chunk::new(self.inner)
    }
}

fn verify_path<'a>(mpath: &'a MPath) -> Result<&'a MPath> {
    let len = mpath.len();
    if len > (u16::max_value() as usize) {
        bail_err!(ErrorKind::WirePackEncode(format!(
            "attempted to encode a filename of length {} -- maximum length supported is {}",
            len,
            u16::max_value()
        )));
    }
    Ok(mpath)
}
