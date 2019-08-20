// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![feature(never_type)]
#![deny(warnings)]

use bytes::Bytes;

use cloned::cloned;
use failure_ext::Error;
use futures::{Future, IntoFuture, Stream};
use futures_ext::FutureExt;

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
pub use rechunk::rechunk;

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
#[derive(Debug, Clone)]
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
    pub git_sha1: Option<hash::GitSha1>,
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

    pub fn with_git_sha1(size: u64, git_sha1: hash::GitSha1) -> Self {
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
        .map(Some)
        .or_else(|err| match err {
            LoadableError::Error(err) => Err(err),
            LoadableError::Missing => Ok(None),
        })
        .and_then({
            cloned!(blobstore, ctx);
            move |maybe_id| match maybe_id {
                Some(id) => metadata::get_metadata(blobstore, ctx, id).left_future(),
                None => Ok(None).into_future().right_future(),
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
        .map(Some)
        .or_else(|err| match err {
            LoadableError::Error(err) => Err(err),
            LoadableError::Missing => Ok(None),
        })
        .and_then({
            cloned!(blobstore, ctx);
            move |maybe_id| maybe_id.map(|id| blobstore.is_present(ctx, id.blobstore_key()))
        })
        .map(|exists: Option<bool>| exists.unwrap_or(false))
}

/// Fetch a file as a stream. This returns either success with a stream of data if the file
///  exists, success with None if it does not exist, or an Error if either existence can't
/// be determined or if opening the file failed. File contents are returned in chunks
/// configured by FilestoreConfig::read_chunk_size - this defines the max chunk size, but
/// they may be shorter (not just the final chunks - any of them). Chunks are guaranteed to
/// have non-zero size.
pub fn fetch<B: Blobstore + Clone>(
    blobstore: &B,
    ctx: CoreContext,
    key: &FetchKey,
) -> impl Future<Item = Option<impl Stream<Item = Bytes, Error = Error>>, Error = Error> {
    key.load(ctx.clone(), blobstore)
        .map(Some)
        .or_else(|err| match err {
            LoadableError::Error(err) => Err(err),
            LoadableError::Missing => Ok(None),
        })
        .and_then({
            cloned!(blobstore, ctx);
            move |content_id| match content_id {
                Some(content_id) => fetch::fetch(blobstore, ctx, content_id).left_future(),
                None => Ok(None).into_future().right_future(),
            }
        })
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
    config: &FilestoreConfig,
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

/// Store a set of bytes, and immediately return FileContents. This function does NOT do chunking.
/// This is intended as a transition function while we convert writers to streams and refactor them
/// to not expect to be able to obtain the ContentId for the content they uploaded immediately.
/// Avoid adding new callsites.
pub fn store_bytes<B: Blobstore + Clone>(
    blobstore: B,
    ctx: CoreContext,
    bytes: Bytes,
) -> (FileContents, impl Future<Item = (), Error = Error>) {
    let prepared = prepare::prepare_bytes(bytes);

    (
        prepared.contents.clone(),
        finalize::finalize(blobstore, ctx, None, prepared).map(|_| ()),
    )
}
