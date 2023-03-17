/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use anyhow::Result;
use blobstore::Blobstore;
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use futures::future;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::task::Poll;
use mononoke_types::content_chunk::new_blob_and_pointer;
use mononoke_types::hash;
use mononoke_types::BlobstoreKey;
use mononoke_types::ChunkedFileContents;
use mononoke_types::FileContents;

use crate::alias::add_aliases_to_multiplexer;
use crate::expected_size::ExpectedSize;
use crate::incremental_hash::hash_bytes;
use crate::incremental_hash::ContentIdIncrementalHasher;
use crate::incremental_hash::GitSha1IncrementalHasher;
use crate::incremental_hash::Sha1IncrementalHasher;
use crate::incremental_hash::Sha256IncrementalHasher;
use crate::multiplexer::Multiplexer;
use crate::multiplexer::MultiplexerError;
use crate::streamhash::hash_stream;

#[derive(Debug, Clone)]
pub struct Prepared {
    pub sha1: hash::Sha1,
    pub sha256: hash::Sha256,
    pub git_sha1: hash::RichGitSha1,
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

/// Prepare a stream of bytes for upload. This will return a Prepared struct that can be used to
/// finalize the upload. The hashes we compute may depend on the size hint.
pub async fn prepare_chunked<B: Blobstore + Clone + 'static, S>(
    ctx: CoreContext,
    blobstore: B,
    expected_size: ExpectedSize,
    chunks: S,
    concurrency: usize,
) -> Result<Prepared, Error>
where
    S: Stream<Item = Result<Bytes, Error>> + Send,
{
    // NOTE: The Multiplexer makes clones of the Bytes we pass in. It's worth noting that Bytes
    // actually behaves like an Arc with an inner reference-counted handle to data, so those
    // clones are actually fairly cheap
    let mut multiplexer = Multiplexer::<Bytes>::new();

    // Spawn a stream for each hash we need to produce.
    let content_id =
        multiplexer.add(|stream| hash_stream(ContentIdIncrementalHasher::new(), stream));

    let aliases = add_aliases_to_multiplexer(&mut multiplexer, expected_size);

    // For the file's contents, spawn new tasks for each individual chunk. This ensures that
    // each chunk is hashed and uploaded separately, and potentially on a different CPU core.
    // We allow up to concurrency uploads to progress at the same time, which creates
    // backpressure into the chunks Stream.
    let contents = multiplexer
        .add(move |stream| {
            stream
                .map(move |bytes| {
                    // NOTE: This is lazy to allow the hash computation for this chunk's ID to
                    // happen on a separate core.
                    let fut = {
                        cloned!(blobstore, ctx);
                        async move {
                            let (blob, pointer) = new_blob_and_pointer(bytes);

                            // TODO: Convert this along with other store calls to impl Storable for
                            // MononokeId.
                            blobstore
                                .put(&ctx, blob.id().blobstore_key(), blob.into())
                                .await?;

                            Result::<_, Error>::Ok(pointer)
                        }
                    };

                    async move { tokio::task::spawn(fut).await? }
                })
                .buffered(concurrency)
                .try_fold(vec![], |mut chunks, chunk| async move {
                    chunks.push(chunk);
                    Result::<_, Error>::Ok(chunks)
                })
        })
        .map(|res| match res {
            Ok(Ok(r)) => Ok(r),
            Ok(Err(e)) => Err(e),
            Err(e) => Err(e.into()),
        });

    let res = multiplexer.drain(chunks).await;

    // Coerce the Error value for all our futures to Error.
    let content_id = content_id.map_err(Error::from);
    let aliases = aliases.map_err(Error::from);
    let contents = contents.map_err(Error::from);

    let futs = future::try_join3(content_id, aliases, contents);

    match res {
        // All is well - get the results when our futures complete.
        Ok(_) => {
            let (content_id, aliases, chunks) = futs.await?;

            let contents = FileContents::Chunked(ChunkedFileContents::new(content_id, chunks));

            let (sha1, sha256, git_sha1) = aliases.redeem(contents.size())?;

            let prepared = Prepared {
                sha1,
                sha256,
                git_sha1,
                contents,
            };

            Ok(prepared)
        }
        // If the Multiplexer hit an error, then it's worth handling the Cancelled case
        // separately: Cancelled means our Multiplexer noted that one of its readers
        // stopped reading. Usually, this will be because one of the readers failed. So,
        // let's just poll the readers once to see if they have an error value ready, and
        // if so, let's return that Error (because it'll be a more usable one). If not,
        // we'll passthrough the cancellation (but, we do have a unit test to make sure we
        // hit the happy path that prettifies the error).
        Err(m @ MultiplexerError::Cancelled) => match futures::poll!(futs) {
            Poll::Ready(Err(e)) => Err(e),
            _ => Err(m.into()),
        },

        Err(m @ MultiplexerError::InputError(..)) => Err(m.into()),
    }
}
