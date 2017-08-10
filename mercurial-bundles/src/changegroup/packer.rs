// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::convert::From;

use futures::{Async, Poll, Stream};

use byteorder::ByteOrder;
use bytes::{BigEndian, BufMut};
use chunk::Chunk;
use errors::*;

use super::{CgDeltaChunk, Part, Section};

pub struct Cg2Packer<S> {
    delta_stream: S,
    last_seen: Section,
}

impl<S> Cg2Packer<S> {
    pub fn new(delta_stream: S) -> Self {
        Cg2Packer {
            delta_stream: delta_stream,
            last_seen: Section::Changeset,
        }
    }
}

impl<S> Stream for Cg2Packer<S>
where
    S: Stream<Item = Part>,
    Error: From<S::Error>,
{
    type Item = Chunk;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Chunk>, Error> {
        use self::Part::*;

        match try_ready!(self.delta_stream.poll()) {
            None => Ok(Async::Ready(None)),
            Some(CgChunk(section, delta_chunk)) => {
                let mut builder = ChunkBuilder::new();
                if self.last_seen != section {
                    builder.encode_section(&section);
                    self.last_seen = section;
                }
                builder.encode_delta_chunk(delta_chunk);
                Ok(Async::Ready(Some(builder.build()?)))
            }
            Some(SectionEnd(_section)) => Ok(Async::Ready(Some(empty_cg_chunk()))),
            Some(End) => Ok(Async::Ready(Some(empty_cg_chunk()))),
        }
    }
}

/// Produce an empty changegroup chunk.
///
/// Note that this is distinct from Chunk::empty() -- this is an actual chunk
/// with a 4-byte payload.
fn empty_cg_chunk() -> Chunk {
    Chunk::new(vec![0, 0, 0, 0]).expect("Chunk::new should not fail for a 4-byte chunk")
}

struct ChunkBuilder {
    inner: Vec<u8>,
    len_offset: usize,
}

impl ChunkBuilder {
    pub fn new() -> Self {
        ChunkBuilder {
            // Reserve four bytes in the beginning for the length.
            inner: vec![0, 0, 0, 0],
            len_offset: 0,
        }
    }

    /// Encode the beginning of a section. This should always happen before any
    /// delta chunks are encoded.
    pub fn encode_section(&mut self, section: &Section) -> &mut Self {
        assert_eq!(
            self.inner.len(),
            4,
            "encode_section must only be called once at the start"
        );
        // Changeset and manifest sections are implicitly encoded, so we don't
        // need to do anything there.
        // TODO: will need to encode tree manifests here as well
        if let &Section::Filelog(ref f) = section {
            // Note that the filename length must include the four bytes for itself.
            BigEndian::write_i32(&mut self.inner[0..], (f.len() + 4) as i32);
            self.inner.put_slice(f.to_vec().as_slice());
            // Add four more bytes for the start of the section.
            self.len_offset = self.inner.len();
            self.inner.put_slice(&[0, 0, 0, 0]);
        }
        self
    }

    pub fn encode_delta_chunk(&mut self, chunk: CgDeltaChunk) -> &mut Self {
        self.inner.put_slice(chunk.node.as_ref());
        self.inner.put_slice(chunk.p1.as_ref());
        self.inner.put_slice(chunk.p2.as_ref());
        self.inner.put_slice(chunk.base.as_ref());
        self.inner.put_slice(chunk.linknode.as_ref());

        for fragment in chunk.delta.fragments() {
            self.inner.put_i32::<BigEndian>(fragment.start as i32);
            self.inner.put_i32::<BigEndian>(fragment.end as i32);
            self.inner
                .put_i32::<BigEndian>(fragment.content.len() as i32);
            self.inner.put_slice(&fragment.content[..]);
        }

        self
    }

    pub fn build(self) -> Result<Chunk> {
        let len = self.inner.len() - self.len_offset;
        let mut inner = self.inner;
        BigEndian::write_i32(&mut inner[self.len_offset..], len as i32);
        Chunk::new(inner)
    }
}
