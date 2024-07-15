/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Packing wirepacks to be sent over the wire during e.g. an hg pull.
//! The format is documented at
//! <https://phab.mercurial-scm.org/diffusion/FBHGX/browse/default/remotefilelog/wirepack.py>

use anyhow::bail;
use anyhow::Result;
use bytes::BufMut;
use futures::Stream;
use mercurial_types::NonRootMPath;
use mercurial_types::RepoPath;

use super::converter::convert_wirepack;
use super::converter::WirePackPartProcessor;
use super::DataEntry;
use super::HistoryEntry;
use super::Kind;
use super::Part;
use super::WIREPACK_END;
use crate::chunk::Chunk;
use crate::errors::ErrorKind;

pub fn pack_wirepack(
    part_stream: impl Stream<Item = Result<Part>>,
    kind: Kind,
) -> impl Stream<Item = Result<Chunk>> {
    convert_wirepack(part_stream, PackerProcessor { kind })
}

struct PackerProcessor {
    kind: Kind,
}

impl WirePackPartProcessor for PackerProcessor {
    type Data = Chunk;

    fn history_meta(&mut self, path: &RepoPath, entry_count: u32) -> Result<Option<Self::Data>> {
        let mut builder = ChunkBuilder::new(self.kind);
        builder.encode_filename(path)?;
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
        builder.encode_data(data_entry)?;
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
            (Kind::File, &RepoPath::RootPath) => bail!(ErrorKind::WirePackEncode(
                "attempted to encode a zero-length filename into a file wirepack".into()
            )),
            (Kind::Tree, RepoPath::DirectoryPath(dir_path)) => Some(verify_path(dir_path)?),
            (Kind::File, RepoPath::FilePath(file_path)) => Some(verify_path(file_path)?),
            (kind, path) => bail!(ErrorKind::WirePackEncode(format!(
                "attempted to encode incompatible path into wirepack (kind: {}, path: {:?})",
                kind, path
            ))),
        };

        match mpath {
            Some(mpath) => {
                self.inner.put_u16(mpath.len() as u16);
                mpath.generate(&mut self.inner)?;
            }
            None => {
                self.inner.put_u16(0);
            }
        }
        Ok(self)
    }

    #[inline]
    fn encode_entry_count(&mut self, entry_count: u32) -> &mut Self {
        self.inner.put_u32(entry_count);
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

fn verify_path<'a>(mpath: &'a NonRootMPath) -> Result<&'a NonRootMPath> {
    let len = mpath.len();
    if len > (u16::MAX as usize) {
        bail!(ErrorKind::WirePackEncode(format!(
            "attempted to encode a filename of length {} -- maximum length supported is {}",
            len,
            u16::MAX
        )));
    }
    Ok(mpath)
}
