/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use cloned::cloned;
use futures_ext::{BoxFuture, FutureExt};
use futures_old::{future::IntoFuture, Future};
use slog::debug;
use thiserror::Error;

use blobstore::{Blobstore, Loadable, LoadableError};
use context::CoreContext;
use mononoke_types::{ChunkedFileContents, ContentId, ContentMetadata, FileContents};

use crate::{fetch, get_metadata, store, FetchKey, FilestoreConfig, StoreRequest};

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Content not found: {0:?}")]
    ContentNotFound(ContentId),
}

/// Fetch a file from the blobstore and reupload it in a chunked form.
/// NOTE: This could actually unchunk a file if the chunk size threshold
/// is increased after the file is written.
pub fn force_rechunk<B: Blobstore + Clone>(
    blobstore: B,
    config: FilestoreConfig,
    ctx: CoreContext,
    content_id: ContentId,
) -> impl Future<Item = ContentMetadata, Error = Error> {
    content_id
        .load(ctx.clone(), &blobstore)
        .map_err(move |err| match err {
            LoadableError::Error(err) => err,
            LoadableError::Missing(_) => ErrorKind::ContentNotFound(content_id).into(),
        })
        .and_then(move |file_contents| {
            do_rechunk_file_contents(blobstore, config, ctx, file_contents, content_id)
        })
}

/// Fetch a file from the blobstore and reupload it in a chunked form
/// only if it is chunked using a larger chunk size (or unchunked)
/// Note that this fn is not suitable for unchunking a file,
/// as if existing file uses smaller-than-requested chunk size,
/// this fn won't do anything.
/// Returns a future, resolving to the `ContentMetadata` of the
/// processed `ContentId` and whether it was *actually* rechunked
pub fn rechunk<B: Blobstore + Clone>(
    blobstore: B,
    filestore_config: FilestoreConfig,
    ctx: CoreContext,
    content_id: ContentId,
) -> BoxFuture<(ContentMetadata, bool), Error> {
    let fetch_key = FetchKey::Canonical(content_id.clone());
    let chunk_size = filestore_config.chunk_size;
    get_metadata(&blobstore, ctx.clone(), &fetch_key)
        .and_then({
            cloned!(content_id);
            move |maybe_content_metadata| match maybe_content_metadata {
                Some(content_metadata) => Ok(content_metadata),
                None => Err(ErrorKind::ContentNotFound(content_id).into()),
            }
        })
        .and_then({
            cloned!(ctx, blobstore);
            move |content_metadata| match chunk_size {
                Some(chunk_size) if content_metadata.total_size > chunk_size => {
                    rechunk_if_uses_larger_chunk_size(
                        blobstore,
                        chunk_size,
                        filestore_config.concurrency,
                        ctx.clone(),
                        content_metadata,
                    )
                    .left_future()
                }
                _ => Ok((content_metadata, false)).into_future().right_future(),
            }
        })
        .boxify()
}

/// Return true if stored `chunked_file_contents` uses chunks larger
/// than `expected_chunk_size`
fn uses_larger_chunks(
    ctx: &CoreContext,
    chunked_file_contents: &ChunkedFileContents,
    expected_chunk_size: u64,
    content_id: &ContentId,
) -> bool {
    let mut all_smaller_or_equal = true;
    let mut num_smaller_chunks = 0;
    let num_chunks = chunked_file_contents.num_chunks();
    for (idx, content_chunk_pointer) in chunked_file_contents.iter_chunks().enumerate() {
        let real_chunk_size = content_chunk_pointer.size();
        if real_chunk_size > expected_chunk_size {
            all_smaller_or_equal = false;
            break;
        } else if real_chunk_size < expected_chunk_size {
            if idx == num_chunks - 1 {
                // last pointer, it's ok if it is smaller
                continue;
            }

            num_smaller_chunks += 1;
        }
    }

    if num_smaller_chunks > 0 {
        debug!(
            ctx.logger(),
            "{} chunks of {} have size smaller than what we want: {}. No action will be taken",
            num_smaller_chunks,
            content_id,
            expected_chunk_size
        );
    }

    !all_smaller_or_equal
}

/// For content, represented by `content_metadata`, rechunk it
/// if it is unchunked or uses larger chunk sizes
/// Note: this fn expects `expected_chunk_size` and `concurrency`
/// instead of `FilestoreConfig` to emphasize that it can only be
/// called, if the filestore's chunk size is not `None`
fn rechunk_if_uses_larger_chunk_size<B: Blobstore + Clone>(
    blobstore: B,
    expected_chunk_size: u64,
    concurrency: usize,
    ctx: CoreContext,
    contend_metadata: ContentMetadata,
) -> BoxFuture<(ContentMetadata, bool), Error> {
    let content_id = contend_metadata.content_id.clone();

    content_id
        .load(ctx.clone(), &blobstore)
        .map_err(move |err| match err {
            LoadableError::Error(err) => err,
            LoadableError::Missing(_) => ErrorKind::ContentNotFound(content_id).into(),
        })
        .and_then({
            cloned!(ctx, blobstore);
            move |file_contents| {
                let should_rechunk = match file_contents {
                    FileContents::Bytes(_) => true,
                    FileContents::Chunked(ref chunked_file_contents) => uses_larger_chunks(
                        &ctx,
                        chunked_file_contents,
                        expected_chunk_size,
                        &content_id,
                    ),
                };

                if should_rechunk {
                    let filestore_config = FilestoreConfig {
                        chunk_size: Some(expected_chunk_size),
                        concurrency,
                    };

                    do_rechunk_file_contents(
                        blobstore,
                        filestore_config,
                        ctx,
                        file_contents,
                        content_id,
                    )
                    .map(|v| (v, true))
                    .left_future()
                } else {
                    Ok((contend_metadata, false)).into_future().right_future()
                }
            }
        })
        .boxify()
}

/// Unconditionally rechunk `file_contents` using the `filestore_config`
/// NOTE: This could actually unchunk a file if the chunk size threshold
/// is increased after the file is written.
fn do_rechunk_file_contents<B: Blobstore + Clone>(
    blobstore: B,
    filestore_config: FilestoreConfig,
    ctx: CoreContext,
    file_contents: FileContents,
    content_id: ContentId,
) -> impl Future<Item = ContentMetadata, Error = Error> {
    let req = StoreRequest::with_canonical(file_contents.size(), content_id);
    let file_stream = fetch::stream_file_bytes(
        blobstore.clone(),
        ctx.clone(),
        file_contents,
        fetch::Range::All,
    );
    store(blobstore, filestore_config, ctx, &req, file_stream)
}
