/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use blobstore::Blobstore;
use blobstore::Loadable;
use blobstore::LoadableError;
use blobstore::Storable;
use context::CoreContext;
use mononoke_types::BlobstoreValue;
use mononoke_types::ContentId;
use mononoke_types::ContentMetadata;
use mononoke_types::ContentMetadataId;
use thiserror::Error;

use crate::alias::alias_stream;
use crate::expected_size::ExpectedSize;
use crate::fetch;

#[derive(Debug, Error)]
pub enum RebuildBackmappingError {
    #[error("Not found: {0:?}")]
    NotFound(ContentId),

    #[error("Error computing metadata for {0:?}: {1:?}")]
    InternalError(ContentId, #[source] Error),
}

/// Finds the metadata for a ContentId. Returns None if the content does not exist, and returns
/// the metadata otherwise. This might recompute the metadata on the fly if it is found to
/// be missing but the content exists.
pub async fn get_metadata<B: Blobstore>(
    blobstore: &B,
    ctx: &CoreContext,
    content_id: ContentId,
) -> Result<Option<ContentMetadata>, Error> {
    let maybe_metadata = get_metadata_readonly(blobstore, ctx, content_id).await?;

    // We found the metadata. Return it.
    if let Some(metadata) = maybe_metadata {
        return Ok(Some(metadata));
    }

    // We didn't find the metadata. Try to recompute it. This might fail if the
    // content doesn't exist, or due to an internal error.
    rebuild_metadata(blobstore, ctx, content_id)
        .await
        .map(Some)
        .or_else({
            use RebuildBackmappingError::*;
            |e| match e {
                // If we didn't find the ContentId we're rebuilding the metadata for,
                // then there is nothing else to do but indicate this metadata does not
                // exist.
                NotFound(_) => Ok(None),
                // If we ran into some error rebuilding the metadata that isn't not
                // having found the content, then we pass it up.
                e @ InternalError(..) => Err(e.into()),
            }
        })
}

/// Finds the metadata for a ContentId. Returns None if the content metadata does not exist
/// and returns Some(metadata) if it already exists. Does not recompute it on the fly.
pub async fn get_metadata_readonly<B: Blobstore>(
    blobstore: &B,
    ctx: &CoreContext,
    content_id: ContentId,
) -> Result<Option<ContentMetadata>, Error> {
    ContentMetadataId::from(content_id)
        .load(ctx, blobstore)
        .await
        .map(Some)
        .or_else(|err| match err {
            LoadableError::Error(err) => Err(err),
            LoadableError::Missing(_) => Ok(None),
        })
}

/// If the metadata is missing, we can rebuild it on the fly, since all that's needed to do so
/// is the file contents. This can happen if we successfully stored a file, but failed to store
/// its metadata. To rebuild the metadata, we peek at the content in the blobstore to get
/// its size, then produce a stream of its contents and compute aliases over it. Finally, store
/// the metadata, and return it.
async fn rebuild_metadata<B: Blobstore>(
    blobstore: &B,
    ctx: &CoreContext,
    content_id: ContentId,
) -> Result<ContentMetadata, RebuildBackmappingError> {
    use RebuildBackmappingError::*;

    let file_contents = content_id
        .load(ctx, blobstore)
        .await
        .map_err(|err| match err {
            LoadableError::Error(err) => InternalError(content_id, err),
            LoadableError::Missing(_) => NotFound(content_id),
        })?;

    // NOTE: We implicitly trust data from the Filestore here. We do not validate
    // the size, nor the ContentId.
    let total_size = file_contents.size();
    let content_stream =
        fetch::stream_file_bytes(blobstore, ctx, file_contents, fetch::Range::all())
            .map_err(|e| InternalError(content_id, e))?;

    let redeemable = alias_stream(ExpectedSize::new(total_size), content_stream)
        .await
        .map_err(|e| InternalError(content_id, e))?;

    let (sha1, sha256, git_sha1) = redeemable
        .redeem(total_size)
        .map_err(|e| InternalError(content_id, e))?;

    let metadata = ContentMetadata {
        total_size,
        content_id,
        sha1,
        sha256,
        git_sha1,
    };

    let blob = metadata.clone().into_blob();

    blob.store(ctx, blobstore)
        .await
        .map_err(|e| InternalError(content_id, e))?;

    Ok(metadata)
}
