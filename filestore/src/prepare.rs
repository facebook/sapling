// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobstore::Blobstore;
use bytes::Bytes;
use context::CoreContext;
use failure_ext::{Error, Result};
use futures::{
    future::{lazy, IntoFuture},
    Future, Stream,
};
use futures_ext::{BoxFuture, BoxStream};
use mononoke_types::{hash, ContentId, FileContents};
use std::convert::TryInto;

use crate::expected_size::ExpectedSize;
use crate::finalize::finalize;
use crate::incremental_hash::{
    hash_bytes, ContentIdIncrementalHasher, GitSha1IncrementalHasher, Sha1IncrementalHasher,
    Sha256IncrementalHasher,
};
use crate::streamhash::hash_stream;
use crate::StoreRequest;

#[derive(Debug, Clone)]
pub struct Prepared {
    pub total_size: u64,
    pub sha1: hash::Sha1,
    pub sha256: hash::Sha256,
    pub git_sha1: hash::GitSha1,
    pub contents: FileContents,
}

fn prepare_bytes(bytes: Bytes) -> Prepared {
    // This will panic if we have a buffer whose size is too large to fit in a u64. Not worth
    // handling here.
    let total_size = bytes.len().try_into().unwrap();

    let sha1 = hash_bytes(Sha1IncrementalHasher::new(), &bytes);
    let sha256 = hash_bytes(Sha256IncrementalHasher::new(), &bytes);
    let git_sha1 = hash_bytes(GitSha1IncrementalHasher::new(&bytes), &bytes);

    let contents = FileContents::Bytes(bytes);

    Prepared {
        total_size,
        sha1,
        sha256,
        git_sha1,
        contents,
    }
}

/// Prepare a set of Bytes for upload. The size hint isn't actually used here, it's just passed
/// through.
pub fn prepare_inline(
    chunk: BoxFuture<Bytes, Error>,
) -> impl Future<Item = Prepared, Error = Error> {
    chunk.map(prepare_bytes)
}

/// Prepare a stream of bytes for upload. This will return a Prepared struct that can be used to
/// finalize the upload. The hashes we compute may depend on the size hint.
pub fn prepare_chunked<B: Blobstore + Clone>(
    ctx: CoreContext,
    blobstore: B,
    expected_size: ExpectedSize,
    chunks: BoxStream<Bytes, Error>,
) -> impl Future<Item = Prepared, Error = Error> {
    lazy(move || {
        // Split the error out of the data stream so we don't need to worry about cloning it
        let (chunks, err) = futures_ext::split_err(chunks);

        // One stream for the data itself, and one for each hash format we might need
        let mut copies = futures_ext::stream_clone(chunks, 5).into_iter();

        let chunks = copies.next().unwrap();

        let content_id = hash_stream(ContentIdIncrementalHasher::new(), copies.next().unwrap())
            .shared()
            .map_err(|_| -> Error { unreachable!() });

        let sha1 = hash_stream(Sha1IncrementalHasher::new(), copies.next().unwrap())
            .shared()
            .map_err(|_| -> Error { unreachable!() });

        let sha256 = hash_stream(Sha256IncrementalHasher::new(), copies.next().unwrap())
            .shared()
            .map_err(|_| -> Error { unreachable!() });

        let git_sha1 = hash_stream(
            GitSha1IncrementalHasher::new(expected_size),
            copies.next().unwrap(),
        )
        .shared()
        .map_err(|_| -> Error { unreachable!() });
        let acc: Vec<(ContentId, u64)> = vec![];

        // XXX: Allow for buffering here? Note that ordering matters (we need the chunks in order)
        let contents = chunks
            .map_err(|_| -> Error { unreachable!() })
            .and_then(move |bytes| {
                // NOTE: When uploading individual chunks, we still treat them as a regular prepare +
                // finalize Filestore upload (i.e. we create all mappings and such). Here's why:
                //
                // Consider a file that contains the chunks A B C.
                //
                // Now, consider that we upload this file but don't create mappings, and later we
                // attempt to upload a file contents happen to be A, but that upload fails halfway
                // through, and while the Sha1 mapping is created, the Sha256 one isn't.
                //
                // If we'd never uploaded A B C, then this file would logically not exist, since the
                // contents A wouldn't have been uploaded. But, since we did create A, we now have a
                // file that exists for Sha1 readers, but not Sha256 readers. Whoops.
                //
                // To avoid this problem, we always perform a proper upload, even for individual
                // chunks.
                let prepared = prepare_bytes(bytes);
                let chunk_size = prepared.total_size;
                let req = StoreRequest::new(chunk_size);
                finalize(blobstore.clone(), ctx.clone(), &req, prepared)
                    .map(move |content_id| (content_id, chunk_size))
            })
            .fold(acc, |mut chunks, chunk| {
                chunks.push(chunk);
                let res: Result<_> = Ok(chunks);
                res
            });

        let res = (content_id, sha1, sha256, git_sha1, contents)
            .into_future()
            .map(|(content_id, sha1, sha256, git_sha1, chunks)| {
                // NOTE: We don't use the size hint that was provided here! Instead, we compute the
                // actual size we observed.
                let total_size = chunks
                    .iter()
                    .map(|(_, size)| size)
                    .fold(0, |acc, x| acc + x);

                let chunks: Vec<_> = chunks.into_iter().map(|(key, _)| key).collect();

                let contents = FileContents::Chunked((*content_id, chunks));

                Prepared {
                    total_size,
                    sha1: *sha1,
                    sha256: *sha256,
                    git_sha1: *git_sha1,
                    contents,
                }
            });

        assert!(copies.next().is_none());

        // Reunite result with the error
        res.select(err.map(|_| -> Prepared { unreachable!() }))
            .map(|(res, _)| res)
            .map_err(|(err, _)| err)
    })
}
