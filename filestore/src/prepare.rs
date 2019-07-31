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
use mononoke_types::{hash, ChunkedFileContents, FileContents};

use crate::alias::alias_stream;
use crate::expected_size::ExpectedSize;
use crate::finalize::finalize;
use crate::incremental_hash::{
    hash_bytes, ContentIdIncrementalHasher, GitSha1IncrementalHasher, Sha1IncrementalHasher,
    Sha256IncrementalHasher,
};
use crate::streamhash::hash_stream;

#[derive(Debug, Clone)]
pub struct Prepared {
    pub sha1: hash::Sha1,
    pub sha256: hash::Sha256,
    pub git_sha1: hash::GitSha1,
    pub contents: FileContents,
}

pub fn prepare_bytes(bytes: Bytes) -> Prepared {
    let sha1 = hash_bytes(Sha1IncrementalHasher::new(), &bytes);
    let sha256 = hash_bytes(Sha256IncrementalHasher::new(), &bytes);
    let git_sha1 = hash_bytes(GitSha1IncrementalHasher::new(&bytes), &bytes);

    let contents = FileContents::Bytes(bytes);

    Prepared {
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
    concurrency: usize,
) -> impl Future<Item = Prepared, Error = Error> {
    lazy(move || {
        // Split the error out of the data stream so we don't need to worry about cloning it
        let (chunks, err) = futures_ext::split_err(chunks);

        // One stream for the data itself, one for the content ID, and one for the aliases.
        // NOTE: it's safe to unwrap copies.next() below because we make enough copies (and we didn't,
        // we'd hit the issue deterministically in tests).
        let mut copies = futures_ext::stream_clone(chunks, 3).into_iter();

        let content_id = hash_stream(ContentIdIncrementalHasher::new(), copies.next().unwrap())
            .map_err(|e| -> Error { e });
        let aliases =
            alias_stream(expected_size, copies.next().unwrap()).map_err(|e| -> Error { e });

        // XXX: Allow for buffering here? Note that ordering matters (we need the chunks in order)
        let contents = copies
            .next()
            .unwrap()
            .map_err(|e| -> Error { e })
            .map(move |bytes| {
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
                finalize(blobstore.clone(), ctx.clone(), None, prepared)
            })
            .buffered(concurrency)
            .fold(vec![], |mut chunks, chunk| {
                chunks.push(chunk);
                let res: Result<_> = Ok(chunks);
                res
            });

        let res = (content_id, aliases, contents).into_future().and_then(
            |(content_id, aliases, chunks)| {
                let contents = FileContents::Chunked(ChunkedFileContents::new(content_id, chunks));
                let (sha1, sha256, git_sha1) = aliases.redeem(contents.size())?;
                let prepared = Prepared {
                    sha1,
                    sha256,
                    git_sha1,
                    contents,
                };

                Ok(prepared)
            },
        );

        assert!(copies.next().is_none());

        // Reunite result with the error
        res.select(err.map(|e| -> Prepared { e }))
            .map(|(res, _)| res)
            .map_err(|(err, _)| err)
    })
}
