// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobstore::Blobstore;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use failure_ext::{Error, Result};
use futures::{
    future::{lazy, IntoFuture},
    Future, Stream,
};
use futures_ext::FutureExt;
use mononoke_types::{hash, ChunkedFileContents, FileContents};

use crate::alias::add_aliases_to_multiplexer;
use crate::chunk::{BufferedStream, ChunkedStream};
use crate::expected_size::ExpectedSize;
use crate::finalize::finalize;
use crate::incremental_hash::{
    hash_bytes, ContentIdIncrementalHasher, GitSha1IncrementalHasher, Sha1IncrementalHasher,
    Sha256IncrementalHasher,
};
use crate::multiplexer::{Multiplexer, MultiplexerError};
use crate::spawn::{self};
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
pub fn prepare_inline<S>(chunk: BufferedStream<S>) -> impl Future<Item = Prepared, Error = Error>
where
    S: Stream<Item = Bytes, Error = Error>,
{
    chunk.map(prepare_bytes)
}

/// Prepare a stream of bytes for upload. This will return a Prepared struct that can be used to
/// finalize the upload. The hashes we compute may depend on the size hint.
pub fn prepare_chunked<B: Blobstore + Clone, S>(
    ctx: CoreContext,
    blobstore: B,
    expected_size: ExpectedSize,
    chunks: ChunkedStream<S>,
    concurrency: usize,
) -> impl Future<Item = Prepared, Error = Error>
where
    S: Stream<Item = Bytes, Error = Error>,
{
    lazy(move || {
        // NOTE: The Multiplexer makes clones of the Bytes we pass in. It's worth noting that Bytes
        // actually behaves like an Arc with an inner reference-counted handle to data, so those
        // clones are actually fairly cheap
        let mut multiplexer = Multiplexer::new();

        // Spawn a stream for each hash we need to produce.
        let content_id =
            multiplexer.add(|stream| hash_stream(ContentIdIncrementalHasher::new(), stream));
        let aliases = add_aliases_to_multiplexer(&mut multiplexer, expected_size);

        // For the file's contents, spawn new tasks for each individual chunk. This ensures that
        // each chunk is hashed and uploaded separately, and potentially on a different CPU core.
        // We allow up to concurrency uploads to progress at the same time, which creates
        // backpressure into the chunks Stream.
        let contents = multiplexer.add(move |stream| {
            stream
                .map_err(|e| -> Error { e })  // Coerce the Error value for our stream.
                .map(move |bytes| {
                    // NOTE: This is lazy to ensure the hash computation that prepare_bytes
                    // performs happens on a separate task (and therefore potentially a separate
                    // CPU core).
                    let fut = lazy({
                        cloned!(blobstore, ctx);
                        move || {
                            let prepared = prepare_bytes(bytes);
                            finalize(blobstore, ctx, None, prepared)
                        }
                    });

                    spawn::spawn_and_start(fut).map_err(|e| e.into())
                })
                .buffered(concurrency)
                .fold(vec![], |mut chunks, chunk| {
                    chunks.push(chunk);
                    let res: Result<_> = Ok(chunks);
                    res
                })
        });

        multiplexer.drain(chunks).then(|res| {
            // Coerce the Error value for all our futures to Error. Note that the content_id and
            // alias ones actually cannot fail.
            let content_id = content_id.map_err(|e| e.into());
            let aliases = aliases.map_err(|e| e.into());
            let contents = contents.map_err(|e| e.into());

            // Mutable so we can poll later in the error case.
            let mut futs = (content_id, aliases, contents).into_future();

            match res {
                // All is well - get the results when our futures complete.
                Ok(_) => futs
                    .and_then(|(content_id, aliases, chunks)| {
                        let contents =
                            FileContents::Chunked(ChunkedFileContents::new(content_id, chunks));

                        let (sha1, sha256, git_sha1) = aliases.redeem(contents.size())?;

                        let prepared = Prepared {
                            sha1,
                            sha256,
                            git_sha1,
                            contents,
                        };

                        Ok(prepared)
                    })
                    .left_future(),
                // If the Multiplexer hit an error, then it's worth handling the Cancelled case
                // separately: Cancelled means our Multiplexer noted that one of its readers
                // stopped reading. Usually, this will be because one of the readers failed. So,
                // let's just poll the readers once to see if they have an error value ready, and
                // if so, let's return that Error (because it'll be a more usable one). If not,
                // we'll passthrough the cancellation (but, we do have a unit test to make sure we
                // hit the happy path that prettifies the error).
                Err(e) => Err(match e {
                    e @ MultiplexerError::Cancelled => match futs.poll() {
                        Ok(_) => e.into(),
                        Err(e) => e,
                    },
                    e @ MultiplexerError::InputError(_) => e.into(),
                })
                .into_future()
                .right_future(),
            }
        })
    })
}
