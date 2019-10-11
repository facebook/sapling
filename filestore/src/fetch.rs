/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::cmp::{max, min};

use blobstore::{Blobstore, Loadable, LoadableError};
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{Error, Fail};
use futures::{stream, Future, Stream};
use futures_ext::{BufferedParams, FutureExt, StreamExt};
use itertools::Either;
use mononoke_types::{ContentChunk, ContentChunkId, ContentId, FileContents};

// TODO: Make this configurable? Perhaps as a global, since it's something that only makes sense at
// the program level (as opposed to e;g. chunk size, which makes sense at the repo level).
const BUFFER_MEMORY_BUDGET: u64 = 16 * 1024 * 1024; // 16MB.
const BUFFER_MAX_SIZE: usize = 1024; // Fairly arbitrarily large buffer size (we rely on weight).

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Chunk not found: {:?}", _0)]
    ChunkNotFound(ContentChunkId),
}

#[derive(Debug)]
pub enum Range {
    All,
    Span { start: u64, end: u64 },
}

pub fn stream_file_bytes<B: Blobstore + Clone>(
    blobstore: B,
    ctx: CoreContext,
    file_contents: FileContents,
    range: Range,
) -> impl Stream<Item = Bytes, Error = Error> {
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
                    bytes.slice(slice_start as usize, slice_end as usize)
                }
                Range::All => bytes,
            };
            stream::once(Ok(bytes)).left_stream()
        }
        FileContents::Chunked(chunked) => {
            // File is split into multiple chunks. Dispatch fetches for the
            // chunks that overlap the range, and buffer them.
            let params = BufferedParams {
                weight_limit: BUFFER_MEMORY_BUDGET,
                buffer_size: BUFFER_MAX_SIZE,
            };

            let chunk_iter = match range {
                Range::Span {
                    start: range_start,
                    end: range_end,
                } => {
                    let iter = chunked
                        .into_chunks()
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
                    let iter = chunked
                        .into_chunks()
                        .into_iter()
                        .map(|chunk| (Range::All, chunk));
                    Either::Right(iter)
                }
            };

            stream::iter_ok(chunk_iter.map(move |(chunk_range, chunk)| {
                // Send some (maybe all) of this chunk.
                let chunk_id = chunk.chunk_id();

                let fut = chunk_id
                    .load(ctx.clone(), &blobstore)
                    .or_else(move |err| match err {
                        LoadableError::Error(err) => Err(err),
                        LoadableError::Missing(_) => Err(ErrorKind::ChunkNotFound(chunk_id).into()),
                    })
                    .map(ContentChunk::into_bytes);

                let fut = match chunk_range {
                    Range::Span { start, end } => fut
                        .map(move |b| b.slice(start as usize, end as usize))
                        .left_future(),
                    Range::All => fut.right_future(),
                };

                // Even if we're planning to return only part of this chunk,
                // the weight is still the full chunk size as that is what
                // must be fetched.
                let weight = chunk.size();
                (fut, weight)
            }))
            .buffered_weight_limited(params)
            .right_stream()
        }
    }
}

pub fn fetch_with_size<B: Blobstore + Clone>(
    blobstore: B,
    ctx: CoreContext,
    content_id: ContentId,
    range: Range,
) -> impl Future<Item = Option<(impl Stream<Item = Bytes, Error = Error>, u64)>, Error = Error> {
    content_id
        .load(ctx.clone(), &blobstore)
        .map(Some)
        .or_else(|err| match err {
            LoadableError::Error(err) => Err(err),
            LoadableError::Missing(_) => Ok(None),
        })
        .map({
            cloned!(blobstore, ctx);
            move |maybe_file_contents| {
                maybe_file_contents.map(|file_contents| {
                    let file_size = file_contents.size();
                    (
                        stream_file_bytes(blobstore, ctx, file_contents, range),
                        file_size,
                    )
                })
            }
        })
}
