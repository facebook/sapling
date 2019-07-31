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
use futures_ext::{BoxStream, BufferedParams, StreamExt};
use mononoke_types::{ContentId, FileContents, MononokeId};

// TODO: Make this configurable? Perhaps as a global, since it's something that only makes sense at
// the program level (as opposed to e;g. chunk size, which makes sense at the repo level).
const BUFFER_MEMORY_BUDGET: u64 = 16 * 1024 * 1024; // 16MB.
const BUFFER_MAX_SIZE: usize = 1024; // Fairly arbitrarily large buffer size (we rely on weight).

#[derive(Debug, Fail)]
pub enum FetchError {
    #[fail(display = "Not found: {:?}", _0)]
    NotFound(ContentId, Depth),
    #[fail(display = "Error loading {:?}: {:?}", _0, _1)]
    Error(ContentId, #[cause] Error),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Depth(pub usize);

impl Depth {
    pub const ROOT: Self = Self(0);

    fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

pub fn do_fetch<B: Blobstore + Clone>(
    blobstore: B,
    ctx: CoreContext,
    content_id: ContentId,
    depth: Depth,
) -> BoxStream<Bytes, FetchError> {
    // We have some content to fetch. We're going to produce a Future whose Item is
    // (Option<Bytes>, Vec<(Depth, ContentId)>). Some(Bytes) means we have content to
    // provide in this iteration (i.e. we fetched actual file contents), whereas
    // None(Bytes) will mean we don't have any (i.e. we fetched an index of chunks).
    blobstore
        .get(ctx.clone(), content_id.blobstore_key())
        .map_err({
            cloned!(content_id);
            move |e| FetchError::Error(content_id, e)
        })
        .and_then({
            cloned!(content_id);
            move |maybe_blobstore_bytes| {
                maybe_blobstore_bytes.ok_or(FetchError::NotFound(content_id, depth))
            }
        })
        .and_then({
            cloned!(content_id);
            move |blobstore_bytes| {
                FileContents::from_encoded_bytes(blobstore_bytes.into_bytes())
                    .map_err(|e| FetchError::Error(content_id, e))
            }
        })
        .map({
            cloned!(blobstore, ctx);
            move |contents| match contents {
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

                    let next_depth = depth.next();
                    stream::iter_ok(chunked.into_chunks().into_iter().map(move |c| {
                        let fut =
                            do_fetch(blobstore.clone(), ctx.clone(), c.content_id(), next_depth)
                                .concat2();
                        let weight = c.size();
                        (fut, weight)
                    }))
                    .buffered_weight_limited(params)
                    .right_stream()
                }
            }
        })
        .flatten_stream()
        .boxify()
}

pub fn fetch<B: Blobstore + Clone>(
    blobstore: B,
    ctx: CoreContext,
    content_id: ContentId,
) -> impl Stream<Item = Bytes, Error = FetchError> {
    do_fetch(blobstore, ctx, content_id, Depth::ROOT)
}
