/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Scaffolding for encoding bundle2 parts.

use std::fmt;
use std::fmt::Debug;
use std::fmt::Formatter;

use anyhow::Error;
use anyhow::Result;
use async_stream::try_stream;
use bytes::Bytes;
use futures::future;
use futures::stream::BoxStream;
use futures::Future;
use futures::FutureExt;
use futures::Stream;
use futures::StreamExt;
use futures::TryFutureExt;
use futures::TryStreamExt;

use crate::chunk::Chunk;
use crate::part_header::PartHeaderBuilder;
use crate::part_header::PartHeaderType;
use crate::part_header::PartId;

/// Represents a stream of chunks produced by the individual part handler.
pub struct ChunkStream(BoxStream<'static, Result<Chunk, Error>>);

impl Debug for ChunkStream {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("ChunkStream").finish()
    }
}

#[derive(Debug)]
pub enum PartEncodeData {
    None,
    Fixed(Chunk),
    Generated(ChunkStream),
}

pub struct PartEncodeBuilder {
    headerb: PartHeaderBuilder,
    data: PartEncodeData,
}

pub type PartEncode = BoxStream<'static, Result<Chunk>>;

impl PartEncodeBuilder {
    pub fn mandatory(part_type: PartHeaderType) -> Result<Self> {
        Ok(PartEncodeBuilder {
            headerb: PartHeaderBuilder::new(part_type, true)?,
            data: PartEncodeData::None,
        })
    }

    pub fn advisory(part_type: PartHeaderType) -> Result<Self> {
        Ok(PartEncodeBuilder {
            headerb: PartHeaderBuilder::new(part_type, false)?,
            data: PartEncodeData::None,
        })
    }

    #[inline]
    pub fn add_mparam<S, B>(&mut self, key: S, val: B) -> Result<&mut Self>
    where
        S: Into<String>,
        B: Into<Bytes>,
    {
        self.headerb.add_mparam(key, val)?;
        Ok(self)
    }

    #[inline]
    pub fn add_aparam<S, B>(&mut self, key: S, val: B) -> Result<&mut Self>
    where
        S: Into<String>,
        B: Into<Bytes>,
    {
        self.headerb.add_aparam(key, val)?;
        Ok(self)
    }

    pub fn set_data_fixed<T: Into<Chunk>>(&mut self, data: T) -> &mut Self {
        self.data = PartEncodeData::Fixed(data.into());
        self
    }

    pub fn set_data_future<B, Fut>(&mut self, future: Fut) -> &mut Self
    where
        Fut: Future<Output = Result<B, Error>> + Send + 'static,
        B: Into<Bytes> + Send + 'static,
    {
        let stream = future
            .and_then(|data| async move { Chunk::new(data) })
            .into_stream();
        self.set_data_generated(stream)
    }

    pub fn set_data_generated<S>(&mut self, stream: S) -> &mut Self
    where
        S: Stream<Item = Result<Chunk, Error>> + Send + 'static,
    {
        let stream = stream.try_filter(|chunk| future::ready(!chunk.is_empty()));
        self.data = PartEncodeData::Generated(ChunkStream(stream.boxed()));
        self
    }

    pub fn build(self, part_id: PartId) -> PartEncode {
        try_stream! {
            // Yield the part header.
            yield self.headerb.build(part_id).encode();

            // Yield the chunk(s) for this part.
            match self.data {
                PartEncodeData::Fixed(chunk) => {
                    yield chunk;
                }
                PartEncodeData::Generated(ChunkStream(mut stream)) => {
                    while let Some(chunk) = stream.try_next().await? {
                        yield chunk;
                    }
                }
                PartEncodeData::None => {}
            }

            // Yield an empty chunk to indicate this part is complete.
            yield Chunk::empty();
        }
        .boxed()
    }
}
