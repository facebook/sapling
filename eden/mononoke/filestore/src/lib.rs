/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(never_type)]
#![deny(warnings)]
#![type_length_limit = "2000000"]

use bytes::{Bytes, BytesMut};
use std::convert::TryInto;

use anyhow::Error;
use cloned::cloned;
use futures::future::TryFutureExt;
use futures_ext::FutureExt;
use futures_old::{stream, Future, IntoFuture, Stream};

use blobstore::{Blobstore, Loadable, LoadableError};
use context::CoreContext;
use mononoke_types::{hash, ContentId, ContentMetadata, FileContents, MononokeId};

mod alias;
mod chunk;
mod errors;
mod expected_size;
mod fetch;
mod fetch_key;
mod finalize;
mod incremental_hash;
mod metadata;
mod multiplexer;
mod prepare;
mod rechunk;
mod spawn;
mod streamhash;

pub use fetch_key::{Alias, AliasBlob, FetchKey};
pub use rechunk::{force_rechunk, rechunk};

#[cfg(test)]
mod test;

/// File storage.
///
/// This is a specialized wrapper around a blobstore specifically for user data files (rather
/// rather than metadata, trees, etc). Its primary (initial) goals are:
/// - providing a streaming interface for file access
/// - maintain multiple aliases for each file using different key schemes
/// - maintain reverse mapping from primary key to aliases
///
/// Secondary:
/// - Implement chunking at this level
/// - Compression
/// - Range access (initially fetch, and later store)
///
/// Implementation notes:
/// This code takes over the management of file content in a backwards compatible way - it uses
/// the same blobstore key structure and the same encoding schemes for existing files.
/// Extensions (compression, chunking) will change this, but it will still allow backwards
/// compatibility.
#[derive(Debug, Copy, Clone)]
pub struct FilestoreConfig {
    pub chunk_size: Option<u64>,
    pub concurrency: usize,
}

impl Default for FilestoreConfig {
    fn default() -> Self {
        FilestoreConfig {
            chunk_size: None,
            concurrency: 1,
        }
    }
}

/// Key for storing. We'll compute any missing keys, but we must have the total size.
#[derive(Debug, Clone)]
pub struct StoreRequest {
    pub expected_size: expected_size::ExpectedSize,
    pub canonical: Option<ContentId>,
    pub sha1: Option<hash::Sha1>,
    pub sha256: Option<hash::Sha256>,
    pub git_sha1: Option<hash::RichGitSha1>,
}

impl StoreRequest {
    pub fn new(size: u64) -> Self {
        use expected_size::ExpectedSize;

        Self {
            expected_size: ExpectedSize::new(size),
            canonical: None,
            sha1: None,
            sha256: None,
            git_sha1: None,
        }
    }

    pub fn with_canonical(size: u64, canonical: ContentId) -> Self {
        use expected_size::ExpectedSize;

        Self {
            expected_size: ExpectedSize::new(size),
            canonical: Some(canonical),
            sha1: None,
            sha256: None,
            git_sha1: None,
        }
    }

    pub fn with_sha1(size: u64, sha1: hash::Sha1) -> Self {
        use expected_size::ExpectedSize;

        Self {
            expected_size: ExpectedSize::new(size),
            canonical: None,
            sha1: Some(sha1),
            sha256: None,
            git_sha1: None,
        }
    }

    pub fn with_sha256(size: u64, sha256: hash::Sha256) -> Self {
        use expected_size::ExpectedSize;

        Self {
            expected_size: ExpectedSize::new(size),
            canonical: None,
            sha1: None,
            sha256: Some(sha256),
            git_sha1: None,
        }
    }

    pub fn with_git_sha1(size: u64, git_sha1: hash::RichGitSha1) -> Self {
        use expected_size::ExpectedSize;

        Self {
            expected_size: ExpectedSize::new(size),
            canonical: None,
            sha1: None,
            sha256: None,
            git_sha1: Some(git_sha1),
        }
    }
}

/// Fetch the metadata for the underlying content. This will return None if the content does
/// not exist. It might recompute metadata on the fly if the content exists but the metadata does
/// not.
pub fn get_metadata<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: CoreContext,
    key: &FetchKey,
) -> impl Future<Item = Option<ContentMetadata>, Error = Error> {
    key.load(ctx.clone(), blobstore)
        .compat()
        .map(Some)
        .or_else(|err| match err {
            LoadableError::Error(err) => Err(err),
            LoadableError::Missing(_) => Ok(None),
        })
        .and_then({
            cloned!(blobstore, ctx);
            move |maybe_id| match maybe_id {
                Some(id) => metadata::get_metadata(blobstore, ctx, id).left_future(),
                None => Ok(None).into_future().right_future(),
            }
        })
}

/// Fetch the metadata for the underlying content. This will return None if the content does
/// not exist, Some(None) if the metadata does not exist, and Some(Some(ContentMetadata))
/// when metadata found. It will not recompute metadata on the fly
pub fn get_metadata_readonly<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: CoreContext,
    key: &FetchKey,
) -> impl Future<Item = Option<Option<ContentMetadata>>, Error = Error> {
    key.load(ctx.clone(), blobstore)
        .compat()
        .map(Some)
        .or_else(|err| match err {
            LoadableError::Error(err) => Err(err),
            LoadableError::Missing(_) => Ok(None),
        })
        .and_then({
            cloned!(blobstore, ctx);
            move |maybe_id| match maybe_id {
                Some(id) => metadata::get_metadata_readonly(&blobstore, ctx, id)
                    .map(|maybe_metadata| Some(maybe_metadata))
                    .left_future(),
                None => Ok(Some(None)).into_future().right_future(),
            }
        })
}

/// Return true if the given key exists. A successful return means the key definitely
/// either exists or doesn't; an error means the existence could not be determined.
pub fn exists<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: CoreContext,
    key: &FetchKey,
) -> impl Future<Item = bool, Error = Error> {
    key.load(ctx.clone(), blobstore)
        .compat()
        .map(Some)
        .or_else(|err| match err {
            LoadableError::Error(err) => Err(err),
            LoadableError::Missing(_) => Ok(None),
        })
        .and_then({
            cloned!(blobstore, ctx);
            move |maybe_id| maybe_id.map(|id| blobstore.is_present(ctx, id.blobstore_key()))
        })
        .map(|exists: Option<bool>| exists.unwrap_or(false))
}

/// Fetch a file as a stream. This returns either success with a stream of data and file size if
/// the file exists, success with None if it does not exist, or an Error if either existence can't
/// be determined or if opening the file failed. File contents are returned in chunks configured by
/// FilestoreConfig::read_chunk_size - this defines the max chunk size, but they may be shorter
/// (not just the final chunks - any of them). Chunks are guaranteed to have non-zero size.
pub fn fetch_with_size<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: CoreContext,
    key: &FetchKey,
) -> impl Future<Item = Option<(impl Stream<Item = Bytes, Error = Error>, u64)>, Error = Error> {
    key.load(ctx.clone(), blobstore)
        .compat()
        .map(Some)
        .or_else(|err| match err {
            LoadableError::Error(err) => Err(err),
            LoadableError::Missing(_) => Ok(None),
        })
        .and_then({
            cloned!(blobstore, ctx);
            move |content_id| match content_id {
                Some(content_id) => {
                    fetch::fetch_with_size(blobstore, ctx, content_id, fetch::Range::All)
                        .left_future()
                }
                None => Ok(None).into_future().right_future(),
            }
        })
}

/// This function has the same functionality as fetch_with_size, but only
/// returns data for a range within the file.
///
/// Requests for data beyond the end of the file will return only the part of
/// the file that overlaps with the requested range, if any.
pub fn fetch_range_with_size<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: CoreContext,
    key: &FetchKey,
    start: u64,
    size: u64,
) -> impl Future<Item = Option<(impl Stream<Item = Bytes, Error = Error>, u64)>, Error = Error> {
    key.load(ctx.clone(), blobstore)
        .compat()
        .map(Some)
        .or_else(|err| match err {
            LoadableError::Error(err) => Err(err),
            LoadableError::Missing(_) => Ok(None),
        })
        .and_then({
            cloned!(blobstore, ctx);
            move |content_id| match content_id {
                Some(content_id) => fetch::fetch_with_size(
                    blobstore,
                    ctx,
                    content_id,
                    fetch::Range::Span {
                        start,
                        end: start.saturating_add(size),
                    },
                )
                .left_future(),
                None => Ok(None).into_future().right_future(),
            }
        })
}

/// This function has the same functionality as fetch_with_size, but doesn't return the file size.
pub fn fetch<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: CoreContext,
    key: &FetchKey,
) -> impl Future<Item = Option<impl Stream<Item = Bytes, Error = Error>>, Error = Error> {
    fetch_with_size(blobstore, ctx, key).map(|res| res.map(|(stream, _len)| stream))
}

/// Fetch the contents of a blob concatenated together. This bad for buffering, and you shouldn't
/// add new callsites. This is only for compatibility with existin callsites.
pub fn fetch_concat_opt<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: CoreContext,
    key: &FetchKey,
) -> impl Future<Item = Option<Bytes>, Error = Error> {
    fetch_with_size(blobstore, ctx, key).and_then(|res| match res {
        Some((stream, len)) => {
            let len = len
                .try_into()
                .map_err(|_| anyhow::format_err!("Cannot fetch file with length {}", len));

            len.into_future()
                .and_then(move |len| {
                    let buf = BytesMut::with_capacity(len);

                    stream
                        .fold(buf, |mut buffer, chunk| {
                            buffer.extend_from_slice(&chunk);
                            Result::<_, Error>::Ok(buffer)
                        })
                        .map(BytesMut::freeze)
                        .map(Some)
                })
                .left_future()
        }
        None => Ok(None).into_future().right_future(),
    })
}

/// Similar to `fetch_concat_opt`, but requires the blob to be present, or errors out.
pub fn fetch_concat<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: CoreContext,
    key: impl Into<FetchKey>,
) -> impl Future<Item = Bytes, Error = Error> {
    let key: FetchKey = key.into();
    fetch_concat_opt(blobstore, ctx, &key)
        .and_then(move |bytes| bytes.ok_or_else(|| errors::ErrorKind::MissingContent(key).into()))
}

/// Fetch content associated with the key as a stream
///
/// Moslty behaves as the `fetch`, except it is pushing missing content error into stream if
/// data associated with the key was not found.
pub fn fetch_stream<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: CoreContext,
    key: impl Into<FetchKey>,
) -> impl Stream<Item = Bytes, Error = Error> {
    let key: FetchKey = key.into();
    fetch(blobstore, ctx, &key)
        .and_then(move |stream| stream.ok_or_else(|| errors::ErrorKind::MissingContent(key).into()))
        .flatten_stream()
}

/// This function has the same functionality as fetch_range_with_size, but doesn't return the file size.
pub fn fetch_range<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: CoreContext,
    key: &FetchKey,
    start: u64,
    size: u64,
) -> impl Future<Item = Option<impl Stream<Item = Bytes, Error = Error>>, Error = Error> {
    fetch_range_with_size(blobstore, ctx, key, start, size)
        .map(|res| res.map(|(stream, _len)| stream))
}

/// Fetch the start of a file. Returns a Future that resolves with Some(Bytes) if the file was
/// found, and None otherwise. If Bytes are found, this function is guaranteed to return as many
/// Bytes as requested unless the file is shorter than that.
pub fn peek<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: CoreContext,
    key: &FetchKey,
    size: usize,
) -> impl Future<Item = Option<Bytes>, Error = Error> {
    fetch(blobstore, ctx, key)
        .map(move |maybe_stream| match maybe_stream {
            None => Ok(None).into_future().left_future(),
            Some(stream) => chunk::ChunkStream::new(stream, size)
                .into_future()
                .map(|(bytes, _rest)| bytes)
                .map_err(|(err, _rest)| err)
                .right_future(),
        })
        .flatten()
}

/// Store a file from a stream. This is guaranteed atomic - either the store will succeed
/// for the entire file, or it will fail and the file will logically not exist (however
/// there's no guarantee that any partially written parts will be cleaned up).
pub fn store<B: Blobstore + Clone>(
    blobstore: B,
    config: FilestoreConfig,
    ctx: CoreContext,
    req: &StoreRequest,
    data: impl Stream<Item = Bytes, Error = Error>,
) -> impl Future<Item = ContentMetadata, Error = Error> {
    use chunk::Chunks;

    let prepared = match chunk::make_chunks(data, req.expected_size, config.chunk_size) {
        Chunks::Inline(fut) => prepare::prepare_inline(fut).left_future(),
        Chunks::Chunked(expected_size, chunks) => prepare::prepare_chunked(
            ctx.clone(),
            blobstore.clone(),
            expected_size,
            chunks,
            config.concurrency,
        )
        .right_future(),
    };

    prepared.and_then({
        cloned!(blobstore, ctx, req);
        move |prepared| finalize::finalize(blobstore, ctx, Some(&req), prepared)
    })
}

/// Store a set of bytes, and immediately return their Contentid and size. This function is
/// inefficient for large files, since it will hash the file twice if it's larger than the chunk
/// size. This function is intended as a transition function while we convert writers to streams
/// and refactor them to not expect to be able to obtain the ContentId for the content they
/// uploaded immediately. Do NOT add new callsites.
pub fn store_bytes<B: Blobstore + Clone>(
    blobstore: B,
    config: FilestoreConfig,
    ctx: CoreContext,
    bytes: Bytes,
) -> ((ContentId, u64), impl Future<Item = (), Error = Error>) {
    // NOTE: Like in other places in the Filestore, we assume that the size of buffers being passed
    // in can be represented in 64 bits (which is OK for the world we live in).

    let content_id = FileContents::content_id_for_bytes(&bytes);
    let size: u64 = bytes.len().try_into().unwrap();

    let upload = store(
        blobstore,
        config,
        ctx,
        &StoreRequest::with_canonical(size, content_id),
        stream::once(Ok(bytes)),
    )
    .map(|_| ());

    ((content_id, size), upload)
}
