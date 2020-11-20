/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::cmp::{max, min};
use std::convert::TryInto;

use anyhow::Error;
use blobstore::{Blobstore, Loadable, LoadableError};
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use futures::{
    future,
    stream::{self, Stream, StreamExt},
};
use itertools::Either;
use mononoke_types::{ContentChunk, ContentChunkId, ContentId, FileContents};
use thiserror::Error;

// TODO: Make this configurable? Perhaps as a global, since it's something that only makes sense at
// the program level (as opposed to e;g. chunk size, which makes sense at the repo level).
const BUFFER_MEMORY_BUDGET: u64 = 16 * 1024 * 1024; // 16MB.

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Chunk not found: {0:?}")]
    ChunkNotFound(ContentChunkId),
}

#[derive(Debug)]
pub enum Range {
    All,
    Span { start: u64, end: u64 },
}

pub fn stream_file_bytes<'a, B: Blobstore + Clone + 'a>(
    blobstore: B,
    ctx: CoreContext,
    file_contents: FileContents,
    range: Range,
) -> impl Stream<Item = Result<Bytes, Error>> + 'a {
    match file_contents {
        FileContents::Bytes(bytes) => {
            // File is just a single chunk of bytes. Return the correct
            // slice, based on the requested range.
            let bytes = match range {
                Range::Span {
                    start: range_start,
                    end: range_end,
                } => {
                    let len = bytes.len() as u64;
                    let slice_start = min(range_start, len);
                    let slice_end = min(range_end, len);
                    bytes.slice((slice_start as usize)..(slice_end as usize))
                }
                Range::All => bytes,
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
                Range::Span {
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
                            *chunk_end < range_start
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
                                (Range::All, chunk)
                            } else {
                                (
                                    Range::Span {
                                        start: slice_start - chunk_start,
                                        end: slice_end - chunk_start,
                                    },
                                    chunk,
                                )
                            }
                        });
                    Either::Left(iter)
                }
                Range::All => {
                    let iter = chunks.into_iter().map(|chunk| (Range::All, chunk));
                    Either::Right(iter)
                }
            };

            stream::iter(chunk_iter.map(move |(chunk_range, chunk)| {
                // Send some (maybe all) of this chunk.
                let chunk_id = chunk.chunk_id();
                cloned!(ctx, blobstore);
                async move {
                    let bytes = chunk_id
                        .load(ctx, &blobstore)
                        .await
                        .map_err(move |err| {
                            match err {
                                LoadableError::Error(err) => err,
                                LoadableError::Missing(_) => {
                                    ErrorKind::ChunkNotFound(chunk_id).into()
                                }
                            }
                        })
                        .map(ContentChunk::into_bytes)?;

                    let bytes = match chunk_range {
                        Range::Span { start, end } => bytes.slice((start as usize)..(end as usize)),
                        Range::All => bytes,
                    };

                    Ok(bytes)
                }
            }))
            .buffered(buffer_size)
            .right_stream()
        }
    }
}

pub async fn fetch_with_size<'a, B: Blobstore + Clone + 'a>(
    blobstore: B,
    ctx: CoreContext,
    content_id: ContentId,
    range: Range,
) -> Result<Option<(impl Stream<Item = Result<Bytes, Error>> + 'a, u64)>, Error> {
    let maybe_file_contents = {
        cloned!(ctx, blobstore);
        async move { content_id.load(ctx, &blobstore).await }.await
    }
    .map(Some)
    .or_else(|err| match err {
        LoadableError::Error(err) => Err(err),
        LoadableError::Missing(_) => Ok(None),
    })?;

    Ok(maybe_file_contents.map(|file_contents| {
        let file_size = file_contents.size();
        (
            stream_file_bytes(blobstore, ctx, file_contents, range),
            file_size,
        )
    }))
}
