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
use futures::future::Future;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use futures::join;
use futures::stream::Stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::stream::{self};
use futures::task::Poll;
use mononoke_macros::mononoke;
use mononoke_types::BlobstoreKey;
use mononoke_types::ChunkedFileContents;
use mononoke_types::FileContents;
use mononoke_types::content_chunk::new_blob_and_pointer;
use mononoke_types::content_metadata_v2::PartialMetadata;
use mononoke_types::content_metadata_v2::ends_in_newline;
use mononoke_types::content_metadata_v2::first_line;
use mononoke_types::content_metadata_v2::is_ascii;
use mononoke_types::content_metadata_v2::is_binary;
use mononoke_types::content_metadata_v2::is_generated;
use mononoke_types::content_metadata_v2::is_partially_generated;
use mononoke_types::content_metadata_v2::is_utf8;
use mononoke_types::content_metadata_v2::newline_count;
use mononoke_types::hash;

use crate::alias::add_aliases_to_multiplexer;
use crate::expected_size::ExpectedSize;
use crate::incremental_hash::Blake3IncrementalHasher;
use crate::incremental_hash::ContentIdIncrementalHasher;
use crate::incremental_hash::GitSha1IncrementalHasher;
use crate::incremental_hash::Sha1IncrementalHasher;
use crate::incremental_hash::Sha256IncrementalHasher;
use crate::incremental_hash::hash_bytes;
use crate::multiplexer::Multiplexer;
use crate::multiplexer::MultiplexerError;
use crate::streamhash::hash_stream;

#[derive(Debug, Clone)]
pub struct Prepared {
    pub sha1: hash::Sha1,
    pub sha256: hash::Sha256,
    pub git_sha1: hash::RichGitSha1,
    pub seeded_blake3: hash::Blake3,
    pub is_binary: bool,
    pub is_ascii: bool,
    pub is_utf8: bool,
    pub ends_in_newline: bool,
    pub newline_count: u64,
    pub first_line: Option<String>,
    pub is_generated: bool,
    pub is_partially_generated: bool,
    pub contents: FileContents,
}

pub async fn prepare_bytes(bytes: Bytes) -> Prepared {
    let sha1 = hash_bytes(Sha1IncrementalHasher::new(), &bytes);
    let sha256 = hash_bytes(Sha256IncrementalHasher::new(), &bytes);
    let git_sha1 = hash_bytes(GitSha1IncrementalHasher::new(&bytes), &bytes);
    let seeded_blake3 = hash_bytes(Blake3IncrementalHasher::new_seeded(), &bytes);
    let is_binary = is_binary(stream::once(future::ready(bytes.clone())));
    let is_ascii = is_ascii(stream::once(future::ready(bytes.clone())));
    let is_utf8 = is_utf8(stream::once(future::ready(bytes.clone())));
    let ends_in_newline = ends_in_newline(stream::once(future::ready(bytes.clone())));
    let newline_count = newline_count(stream::once(future::ready(bytes.clone())));
    let first_line = first_line(stream::once(future::ready(bytes.clone())));
    let is_generated = is_generated(stream::once(future::ready(bytes.clone())));
    let is_partially_generated = is_partially_generated(stream::once(future::ready(bytes.clone())));
    let (
        is_binary,
        is_ascii,
        is_utf8,
        ends_in_newline,
        newline_count,
        first_line,
        is_generated,
        is_partially_generated,
    ) = join!(
        is_binary,
        is_ascii,
        is_utf8,
        ends_in_newline,
        newline_count,
        first_line,
        is_generated,
        is_partially_generated
    );
    let contents = FileContents::Bytes(bytes);

    Prepared {
        sha1,
        sha256,
        git_sha1,
        seeded_blake3,
        contents,
        is_binary,
        is_ascii,
        is_utf8,
        ends_in_newline,
        newline_count,
        first_line,
        is_generated,
        is_partially_generated,
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
    let metadata = add_partial_metadata_to_multiplexer(&mut multiplexer);
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

                    async move { mononoke::spawn_task(fut).await? }
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
    let metadata = metadata.map_err(Error::from);

    let futs = future::try_join4(content_id, aliases, contents, metadata);

    match res {
        // All is well - get the results when our futures complete.
        Ok(_) => {
            let (content_id, aliases, chunks, metadata) = futs.await?;
            let contents = FileContents::Chunked(ChunkedFileContents::new(content_id, chunks));

            let (sha1, sha256, git_sha1, seeded_blake3) = aliases.redeem(contents.size())?;

            let prepared = Prepared {
                sha1,
                sha256,
                git_sha1,
                seeded_blake3,
                contents,
                is_ascii: metadata.is_ascii,
                is_binary: metadata.is_binary,
                is_utf8: metadata.is_utf8,
                ends_in_newline: metadata.ends_in_newline,
                newline_count: metadata.newline_count,
                first_line: metadata.first_line,
                is_generated: metadata.is_generated,
                is_partially_generated: metadata.is_partially_generated,
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

pub fn add_partial_metadata_to_multiplexer(
    multiplexer: &mut Multiplexer<Bytes>,
) -> impl Future<Output = Result<PartialMetadata, Error>> + std::marker::Unpin + use<> {
    let is_ascii = multiplexer.add(is_ascii).map_err(Error::from);
    let is_utf8 = multiplexer.add(is_utf8).map_err(Error::from);
    let is_binary = multiplexer.add(is_binary).map_err(Error::from);
    let ends_in_newline = multiplexer.add(ends_in_newline).map_err(Error::from);
    let newline_count = multiplexer.add(newline_count).map_err(Error::from);
    let first_line = multiplexer.add(first_line).map_err(Error::from);
    let is_generated = multiplexer.add(is_generated).map_err(Error::from);
    let is_partially_generated = multiplexer.add(is_partially_generated).map_err(Error::from);

    let fut1 = future::try_join4(is_ascii, is_utf8, is_binary, is_generated);
    let fut2 = future::try_join4(
        ends_in_newline,
        newline_count,
        first_line,
        is_partially_generated,
    );
    future::try_join(fut1, fut2)
        .map_ok(
            |(
                (is_ascii, is_utf8, is_binary, is_generated),
                (ends_in_newline, newline_count, first_line, is_partially_generated),
            )| {
                PartialMetadata {
                    is_binary,
                    is_ascii,
                    is_utf8,
                    ends_in_newline,
                    newline_count,
                    first_line,
                    is_generated,
                    is_partially_generated,
                }
            },
        )
        .map_err(Error::from)
}
