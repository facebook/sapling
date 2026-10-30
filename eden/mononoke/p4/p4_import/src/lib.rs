/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Imports a Perforce depot into a repo.

use anyhow::Result;
use bytes::Bytes;
use context::CoreContext;
use filestore::FilestoreConfigRef;
use filestore::StoreRequest;
use futures::Stream;
use mononoke_types::ContentMetadataV2;
use repo_blobstore::RepoBlobstoreRef;

/// Stores a file revision's content in the repo's filestore. `size` is the size
/// Perforce reports for the revision: content of any other length is rejected.
pub async fn upload_content(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + FilestoreConfigRef),
    size: u64,
    content: impl Stream<Item = Result<Bytes>> + Send,
) -> Result<ContentMetadataV2> {
    filestore::store(
        repo.repo_blobstore(),
        *repo.filestore_config(),
        ctx,
        &StoreRequest::new(size),
        content,
    )
    .await
}

#[cfg(test)]
mod tests;
