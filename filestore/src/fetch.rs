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
use mononoke_types::{ContentId, FileContents, MononokeId};

#[derive(Debug, Fail)]
pub enum FetchError {
    #[fail(display = "Not found: {:?}", _0)]
    NotFound(ContentId, Depth),
    #[fail(display = "Error loading {:?}: {:?}", _0, _1)]
    Error(ContentId, Error),
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct Depth(pub usize);

impl Depth {
    pub const ROOT: Self = Self(0);

    fn next(&self) -> Self {
        Self(self.0 + 1)
    }
}

pub fn fetch<B: Blobstore + Clone>(
    blobstore: B,
    ctx: CoreContext,
    key: ContentId,
) -> impl Stream<Item = Bytes, Error = FetchError> {
    let stack: Vec<(Depth, ContentId)> = vec![(Depth::ROOT, key)];
    // NOTE: This is not the most efficient implementation: we're walking down the chunks one by
    // one, so we'll have to wait until we have finished a branch before visiting the next branch.

    // TODO: Can we use Pavel's bounded traversal for this instead?

    stream::unfold(stack, move |mut stack| {
        cloned!(blobstore, ctx);

        // Pop the last element from the stack
        let next_content_id = stack.pop();

        next_content_id.map(move |(depth, content_id)| {
            // We have some content to fetch. We're going to produce a Future whose Item is
            // (Option<Bytes>, Vec<(Depth, ContentId)>). Some(Bytes) means we have content to
            // provide in this iteration (i.e. we fetched actual file contents), whereas
            // None(Bytes) will mean we don't have any (i.e. we fetched an index of chunks).
            blobstore
                .get(ctx, content_id.blobstore_key())
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
                        let contents = FileContents::from_encoded_bytes(blobstore_bytes.into_bytes())
                        .map_err(|e| FetchError::Error(content_id, e))?;

                        let bytes = match contents {
                            FileContents::Bytes(bytes) => {
                                // We got actual bytes. We don't need to add anythin to your stack
                                // (since there is nothing left to be fetched here), but we do have
                                // Some(Bytes) to return.
                                Some(bytes)
                            }
                            FileContents::Chunked((_, chunks)) => {
                                // We got a list of chunks. The next chunk we need to fetch should be
                                // placed last in our stack, so we need to put the list of chunks into
                                // our stack in reverse order. As for the bytes, we return None,
                                // because we didn't find any.
                                let next_depth = depth.next();
                                stack.extend(chunks.into_iter().rev().map(|c| (next_depth, c)));
                                None
                            }
                        };

                        Ok((bytes, stack))
                    }
                })
        })
    })
    // Finally, we exclude the iterations where Option<Bytes> is None, since our caller doesn't
    // care about those.
    .filter_map(|maybe_bytes| maybe_bytes)
}
