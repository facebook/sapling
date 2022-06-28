/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use futures::future::TryFutureExt;
use slog::debug;
use thiserror::Error;

use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use context::CoreContext;
use mononoke_types::ChunkedFileContents;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadata;
use mononoke_types::FileContents;

use crate::fetch;
use crate::get_metadata;
use crate::store;
use crate::FetchKey;
use crate::FilestoreConfig;
use crate::StoreRequest;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Content not found: {0:?}")]
    ContentNotFound(ContentId),
}

/// Fetch a file from the blobstore and reupload it in a chunked form.
/// NOTE: This could actually unchunk a file if the chunk size threshold
/// is increased after the file is written.
pub async fn force_rechunk<B: Blobstore + Clone + 'static>(
    blobstore: &B,
    config: FilestoreConfig,
    ctx: &CoreContext,
    content_id: ContentId,
) -> Result<ContentMetadata, Error> {
    let file_contents: FileContents = content_id
        .load(ctx, blobstore)
        .map_err(move |err| match err {
            LoadableError::Error(err) => err,
            LoadableError::Missing(_) => ErrorKind::ContentNotFound(content_id).into(),
        })
        .await?;
    do_rechunk_file_contents(blobstore, config, ctx, file_contents, content_id).await
}

/// Fetch a file from the blobstore and reupload it in a chunked form
/// only if it is chunked using a larger chunk size (or unchunked)
/// Note that this fn is not suitable for unchunking a file,
/// as if existing file uses smaller-than-requested chunk size,
/// this fn won't do anything.
/// Returns a future, resolving to the `ContentMetadata` of the
/// processed `ContentId` and whether it was *actually* rechunked
pub async fn rechunk<B: Blobstore + Clone + 'static>(
    blobstore: &B,
    filestore_config: FilestoreConfig,
    ctx: &CoreContext,
    content_id: ContentId,
) -> Result<(ContentMetadata, bool), Error> {
    let fetch_key = FetchKey::Canonical(content_id.clone());
    let chunk_size = filestore_config.chunk_size;
    let metadata = get_metadata(blobstore, ctx, &fetch_key).await?;
    let content_metadata: ContentMetadata = match metadata {
        Some(content_metadata) => content_metadata,
        None => return Err(ErrorKind::ContentNotFound(content_id).into()),
    };

    match chunk_size {
        Some(chunk_size) if content_metadata.total_size > chunk_size => {
            let r: Result<(ContentMetadata, bool), Error> = rechunk_if_uses_larger_chunk_size(
                blobstore,
                chunk_size,
                filestore_config.concurrency,
                ctx,
                content_metadata,
            )
            .await;

            r
        }
        _ => Ok((content_metadata, false)),
    }
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
async fn rechunk_if_uses_larger_chunk_size<B: Blobstore + Clone + 'static>(
    blobstore: &B,
    expected_chunk_size: u64,
    concurrency: usize,
    ctx: &CoreContext,
    content_metadata: ContentMetadata,
) -> Result<(ContentMetadata, bool), Error> {
    let content_id = content_metadata.content_id.clone();

    let file_contents: FileContents = content_id
        .load(ctx, blobstore)
        .map_err(move |err| match err {
            LoadableError::Error(err) => err,
            LoadableError::Missing(_) => ErrorKind::ContentNotFound(content_id).into(),
        })
        .await?;

    let should_rechunk = match file_contents {
        FileContents::Bytes(_) => true,
        FileContents::Chunked(ref chunked_file_contents) => {
            uses_larger_chunks(ctx, chunked_file_contents, expected_chunk_size, &content_id)
        }
    };

    if should_rechunk {
        let filestore_config = FilestoreConfig {
            chunk_size: Some(expected_chunk_size),
            concurrency,
        };

        let content_metadata: ContentMetadata =
            do_rechunk_file_contents(blobstore, filestore_config, ctx, file_contents, content_id)
                .await?;

        Ok((content_metadata, true))
    } else {
        Ok((content_metadata, false))
    }
}

/// Unconditionally rechunk `file_contents` using the `filestore_config`
/// NOTE: This could actually unchunk a file if the chunk size threshold
/// is increased after the file is written.
async fn do_rechunk_file_contents<B: Blobstore + Clone + 'static>(
    blobstore: &B,
    filestore_config: FilestoreConfig,
    ctx: &CoreContext,
    file_contents: FileContents,
    content_id: ContentId,
) -> Result<ContentMetadata, Error> {
    let req = StoreRequest::with_canonical(file_contents.size(), content_id);
    let file_stream = fetch::stream_file_bytes(blobstore, ctx, file_contents, fetch::Range::all())?;

    store(blobstore, filestore_config, ctx, &req, file_stream).await
}
