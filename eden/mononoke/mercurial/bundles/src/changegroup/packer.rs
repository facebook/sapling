/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_stream::try_stream;
use byteorder::BigEndian;
use byteorder::ByteOrder;
use bytes::BufMut;
use futures::pin_mut;
use futures::Stream;
use futures::TryStreamExt;

use super::CgDeltaChunk;
use super::Part;
use super::Section;
use crate::chunk::Chunk;
use crate::delta;

pub fn changegroup_packer(
    delta_stream: impl Stream<Item = Result<Part>>,
) -> impl Stream<Item = Result<Chunk>> {
    try_stream! {
        pin_mut!(delta_stream);
        let mut last_seen = Section::Changeset;
        while let Some(part) = delta_stream.try_next().await? {
            match part {
                Part::CgChunk(section, delta_chunk) => {
                    let mut builder = ChunkBuilder::new();
                    if last_seen != section {
                        builder.encode_section(&section)?;
                        last_seen = section;
                    }
                    builder.encode_delta_chunk(delta_chunk);
                    yield builder.build()?;
                }
                Part::SectionEnd(_) | Part::End => yield empty_cg_chunk(),
            }
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

#[derive(Debug)]
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
    pub fn encode_section(&mut self, section: &Section) -> Result<&mut Self> {
        assert_eq!(
            self.inner.len(),
            4,
            "encode_section must only be called once at the start"
        );
        // Changeset and manifest sections are implicitly encoded, so we don't
        // need to do anything there.
        // TODO: will need to encode tree manifests here as well
        if let Section::Filelog(f) = section {
            let f_vec = f.to_vec();
            // Note that the filename length must include the four bytes for itself.
            BigEndian::write_i32(&mut self.inner[0..], (f_vec.len() + 4) as i32);
            self.inner.put_slice(f_vec.as_slice());
            // Add four more bytes for the start of the section.
            self.len_offset = self.inner.len();
            self.inner.put_slice(&[0, 0, 0, 0]);
        }
        Ok(self)
    }

    pub fn encode_delta_chunk(&mut self, chunk: CgDeltaChunk) -> &mut Self {
        self.inner.put_slice(chunk.node.as_ref());
        self.inner.put_slice(chunk.p1.as_ref());
        self.inner.put_slice(chunk.p2.as_ref());
        self.inner.put_slice(chunk.base.as_ref());
        self.inner.put_slice(chunk.linknode.as_ref());

        if let Some(flags) = chunk.flags {
            let flags = flags.bits();
            self.inner
                .put_slice(&[(flags >> 8) as u8, (flags & 0xff) as u8]);
        }

        delta::encode_delta(&chunk.delta, &mut self.inner);

        self
    }

    pub fn build(self) -> Result<Chunk> {
        let len = self.inner.len() - self.len_offset;
        let mut inner = self.inner;
        BigEndian::write_i32(&mut inner[self.len_offset..], len as i32);
        Chunk::new(inner)
    }
}
