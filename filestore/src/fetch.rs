// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobstore::Blobstore;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{Error, Fail};
use futures::{stream, Future, Stream};
use futures_ext::{BufferedParams, StreamExt};
use mononoke_types::{ContentChunk, ContentChunkId, ContentId, FileContents, Loadable};

// TODO: Make this configurable? Perhaps as a global, since it's something that only makes sense at
// the program level (as opposed to e;g. chunk size, which makes sense at the repo level).
const BUFFER_MEMORY_BUDGET: u64 = 16 * 1024 * 1024; // 16MB.
const BUFFER_MAX_SIZE: usize = 1024; // Fairly arbitrarily large buffer size (we rely on weight).

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Chunk not found: {:?}", _0)]
    ChunkNotFound(ContentChunkId),
}

pub fn stream_file_bytes<B: Blobstore + Clone>(
    blobstore: B,
    ctx: CoreContext,
    file_contents: FileContents,
) -> impl Stream<Item = Bytes, Error = Error> {
    match file_contents {
        FileContents::Bytes(bytes) => {
            // Finally got bytes? Return them.
            stream::once(Ok(bytes)).left_stream()
        }
        FileContents::Chunked(chunked) => {
            // We got chunks. Dispatch new fetches for them, and buffer those. However,
            // note that we actually buffer the data here in each "substream". This is to
            // optimize for the common case of 1 level of nesting.
            let params = BufferedParams {
                weight_limit: BUFFER_MEMORY_BUDGET,
                buffer_size: BUFFER_MAX_SIZE,
            };

            stream::iter_ok(chunked.into_chunks().into_iter().map(move |c| {
                let chunk_id = c.chunk_id();

                let fut = chunk_id
                    .load(ctx.clone(), &blobstore)
                    .and_then({
                        cloned!(chunk_id);
                        move |maybe_chunk| {
                            let e = ErrorKind::ChunkNotFound(chunk_id);
                            maybe_chunk.ok_or(e.into())
                        }
                    })
                    .map(ContentChunk::into_bytes);

                let weight = c.size();
                (fut, weight)
            }))
            .buffered_weight_limited(params)
            .right_stream()
        }
    }
}

pub fn fetch<B: Blobstore + Clone>(
    blobstore: B,
    ctx: CoreContext,
    content_id: ContentId,
) -> impl Future<Item = Option<impl Stream<Item = Bytes, Error = Error>>, Error = Error> {
    content_id.load(ctx.clone(), &blobstore).map({
        cloned!(blobstore, ctx);
        move |maybe_file_contents| {
            maybe_file_contents
                .map(|file_contents| stream_file_bytes(blobstore, ctx, file_contents))
        }
    })
}
