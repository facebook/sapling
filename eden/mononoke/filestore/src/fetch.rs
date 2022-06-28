/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Borrow;
use std::cmp::max;
use std::cmp::min;

use anyhow::Error;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use futures::future;
use futures::stream;
use futures::stream::Stream;
use futures::stream::StreamExt;
use itertools::Either;
use mononoke_types::ContentChunk;
use mononoke_types::ContentChunkId;
use mononoke_types::ContentId;
use mononoke_types::FileContents;
use thiserror::Error;

// TODO: Make this configurable? Perhaps as a global, since it's something that only makes sense at
// the program level (as opposed to e;g. chunk size, which makes sense at the repo level).
const BUFFER_MEMORY_BUDGET: u64 = 16 * 1024 * 1024; // 16MB.

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Chunk not found: {0:?}")]
    ChunkNotFound(ContentChunkId),
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Range {
    inner: RangeInner,
    strict: bool,
}

impl Range {
    pub fn all() -> Self {
        Self {
            inner: RangeInner::All,
            strict: false,
        }
    }

    pub fn sized(start: u64, size: u64) -> Self {
        Self {
            inner: RangeInner::Span {
                start,
                end: start.saturating_add(size),
            },
            strict: false,
        }
    }

    pub fn strict(self) -> Self {
        Self {
            strict: true,
            ..self
        }
    }

    pub fn range_inclusive(start: u64, end: u64) -> Result<Self, Error> {
        if start > end {
            return Err(anyhow::anyhow!("Invalid range bounds: {}-{}", start, end));
        }

        let end = end + 1;

        Ok(Self {
            inner: RangeInner::Span { start, end },
            strict: false,
        })
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum RangeInner {
    All,
    Span { start: u64, end: u64 },
}

impl RangeInner {
    /// Returns how many byts stream_file_bytes will return when this range is applied to a file of
    /// size file_size.
    fn real_size(&self, file_size: u64) -> u64 {
        match self {
            RangeInner::All => file_size,
            RangeInner::Span { start, end } => {
                let end = std::cmp::min(*end, file_size);
                end.saturating_sub(*start)
            }
        }
    }

    fn exceeds_file_size(&self, file_size: u64) -> bool {
        match self {
            RangeInner::All => true,
            RangeInner::Span { start: _, end } => *end > file_size,
        }
    }
}

pub fn stream_file_bytes<'a, B: Blobstore + Clone + 'a>(
    blobstore: B,
    ctx: impl Borrow<CoreContext> + Clone + Send + Sync + 'a,
    file_contents: FileContents,
    range: Range,
) -> Result<impl Stream<Item = Result<Bytes, Error>> + 'a, Error> {
    let Range {
        inner: range,
        strict,
    } = range;

    if strict && range.exceeds_file_size(file_contents.size()) {
        return Err(Error::msg("Range exceeds file size"));
    }

    let stream = match file_contents {
        FileContents::Bytes(bytes) => {
            // File is just a single chunk of bytes. Return the correct
            // slice, based on the requested range.
            let bytes = match range {
                RangeInner::Span {
                    start: range_start,
                    end: range_end,
                } => {
                    let len = bytes.len() as u64;
                    let slice_start = min(range_start, len);
                    let slice_end = min(range_end, len);
                    bytes.slice((slice_start as usize)..(slice_end as usize))
                }
                RangeInner::All => bytes,
            };
            stream::once(future::ready(Ok(bytes))).left_stream()
        }
        FileContents::Chunked(chunked) => {
            // File is split into multiple chunks. Dispatch fetches for the chunks that overlap the
            // range, and buffer them. We know all chunks are the same size (same possibly for the
            // last one) so we use that to get our buffer size.
            let chunks = chunked.into_chunks();

            let max_chunk_size = chunks.iter().map(|c| c.size()).max();
            let buffer_size = match max_chunk_size {
                Some(size) if size > 0 => BUFFER_MEMORY_BUDGET / size,
                _ => 1,
            };
            let buffer_size = std::cmp::max(buffer_size, 1);

            // NOTE: buffer_size cannot be greater than our memory budget given how it's computed,
            // so that's safe.
            let buffer_size = buffer_size.try_into().unwrap();

            let chunk_iter = match range {
                RangeInner::Span {
                    start: range_start,
                    end: range_end,
                } => {
                    let iter = chunks
                        .into_iter()
                        .map({
                            // Compute chunk start and end within the file.
                            let mut stream_offset = 0;
                            move |chunk| {
                                let chunk_start = stream_offset.clone();
                                let chunk_size = chunk.size();
                                stream_offset += chunk_size;
                                (chunk_start, chunk_start + chunk_size, chunk)
                            }
                        })
                        .skip_while(move |(_chunk_start, chunk_end, _chunk)| {
                            // Skip chunks from before the range.
                            *chunk_end <= range_start
                        })
                        .take_while(move |(chunk_start, _chunk_end, _chunk)| {
                            // Take chunks that overlap the range.
                            *chunk_start < range_end
                        })
                        .map(move |(chunk_start, chunk_end, chunk)| {
                            // Compute the slice within this chunk that we need.
                            let slice_start = max(chunk_start, range_start);
                            let slice_end = min(chunk_end, range_end);
                            if (slice_start, slice_end) == (chunk_start, chunk_end) {
                                (RangeInner::All, chunk)
                            } else {
                                (
                                    RangeInner::Span {
                                        start: slice_start - chunk_start,
                                        end: slice_end - chunk_start,
                                    },
                                    chunk,
                                )
                            }
                        });
                    Either::Left(iter)
                }
                RangeInner::All => {
                    let iter = chunks.into_iter().map(|chunk| (RangeInner::All, chunk));
                    Either::Right(iter)
                }
            };

            stream::iter(chunk_iter.map(move |(chunk_range, chunk)| {
                // Send some (maybe all) of this chunk.
                let chunk_id = chunk.chunk_id();
                cloned!(ctx, blobstore);
                async move {
                    let bytes = chunk_id
                        .load(ctx.borrow(), &blobstore)
                        .await
                        .map_err(move |err| match err {
                            LoadableError::Error(err) => err,
                            LoadableError::Missing(_) => ErrorKind::ChunkNotFound(chunk_id).into(),
                        })
                        .map(ContentChunk::into_bytes)?;

                    let bytes = match chunk_range {
                        RangeInner::Span { start, end } => {
                            bytes.slice((start as usize)..(end as usize))
                        }
                        RangeInner::All => bytes,
                    };

                    Ok(bytes)
                }
            }))
            .buffered(buffer_size)
            .right_stream()
        }
    };

    Ok(stream)
}

pub async fn fetch_with_size<'a, B: Blobstore + Clone + 'a>(
    blobstore: B,
    ctx: impl Borrow<CoreContext> + Clone + Send + Sync + 'a,
    content_id: ContentId,
    range: Range,
) -> Result<Option<(impl Stream<Item = Result<Bytes, Error>> + 'a, u64)>, Error> {
    let maybe_file_contents = {
        cloned!(ctx, blobstore);
        async move { content_id.load(ctx.borrow(), &blobstore).await }.await
    }
    .map(Some)
    .or_else(|err| match err {
        LoadableError::Error(err) => Err(err),
        LoadableError::Missing(_) => Ok(None),
    })?;

    let file_contents = match maybe_file_contents {
        Some(file_contents) => file_contents,
        None => return Ok(None),
    };

    let file_size = file_contents.size();
    let stream = stream_file_bytes(blobstore, ctx, file_contents, range)?;
    Ok(Some((stream, range.inner.real_size(file_size))))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_real_size() {
        assert_eq!(RangeInner::All.real_size(5), 5);

        // Bytes 0 to 9 in a 5 bytes file
        assert_eq!(RangeInner::Span { start: 0, end: 10 }.real_size(5), 5);

        // Bytes 0 to 4 in a 5 bytes file
        assert_eq!(RangeInner::Span { start: 0, end: 5 }.real_size(5), 5);

        // Bytes 0 to 3 in a 5 bytes file
        assert_eq!(RangeInner::Span { start: 0, end: 4 }.real_size(5), 4);

        // Bytes 1 to 3 in a 5 bytes file
        assert_eq!(RangeInner::Span { start: 1, end: 4 }.real_size(5), 3);

        // Bytes 3 to 3 in a 5 bytes file
        assert_eq!(RangeInner::Span { start: 3, end: 4 }.real_size(5), 1);

        // Bytes 10 to 10 in a 5 bytes file
        assert_eq!(RangeInner::Span { start: 10, end: 11 }.real_size(5), 0);

        // Bytes 10 to 14 in a 5 bytes file
        assert_eq!(RangeInner::Span { start: 10, end: 15 }.real_size(5), 0);

        // Nothing
        assert_eq!(RangeInner::Span { start: 10, end: 10 }.real_size(5), 0);

        // Nothing
        assert_eq!(RangeInner::Span { start: 0, end: 0 }.real_size(5), 0);
    }
}
