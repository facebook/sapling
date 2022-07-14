/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use blobrepo::BlobRepo;
use blobstore::Blobstore;
use blobstore::Loadable;
use bytes::Bytes;
use context::CoreContext;

use filestore::FetchKey;
use futures::TryStreamExt;
use mononoke_types::blame::BlameRejected;
use mononoke_types::FileUnodeId;

use crate::DEFAULT_BLAME_FILESIZE_LIMIT;

pub enum FetchOutcome {
    Fetched(Bytes),
    Rejected(BlameRejected),
}

impl FetchOutcome {
    pub fn into_bytes(self) -> Result<Bytes, BlameRejected> {
        match self {
            FetchOutcome::Fetched(bytes) => Ok(bytes),
            FetchOutcome::Rejected(rejected) => Err(rejected),
        }
    }
}

/// Fetch the content of a file ready for blame.  If the file content is
/// too large or binary data is detected then the fetch may be rejected.
pub async fn fetch_content_for_blame(
    ctx: &CoreContext,
    repo: &BlobRepo,
    file_unode_id: FileUnodeId,
) -> Result<FetchOutcome> {
    let filesize_limit = repo
        .get_active_derived_data_types_config()
        .blame_filesize_limit
        .unwrap_or(DEFAULT_BLAME_FILESIZE_LIMIT);
    let blobstore = repo.blobstore().boxed();
    fetch_content_for_blame_with_limit(ctx, &blobstore, file_unode_id, filesize_limit).await
}

/// Fetch the content of a file ready for blame.  If the file content is
/// too large or binary data is detected then the fetch may be rejected.
pub async fn fetch_content_for_blame_with_limit(
    ctx: &CoreContext,
    blobstore: &Arc<dyn Blobstore>,
    file_unode_id: FileUnodeId,
    filesize_limit: u64,
) -> Result<FetchOutcome> {
    let file_unode = file_unode_id.load(ctx, blobstore).await?;
    let content_id = *file_unode.content_id();
    let (mut stream, size) =
        filestore::fetch_with_size(blobstore, ctx.clone(), &FetchKey::Canonical(content_id))
            .await?
            .ok_or_else(|| anyhow!("Missing content: {}", content_id))?;
    if size > filesize_limit {
        return Ok(FetchOutcome::Rejected(BlameRejected::TooBig));
    }
    let mut buffer = Vec::with_capacity(size as usize);
    while let Some(bytes) = stream.try_next().await? {
        if bytes.contains(&0u8) {
            return Ok(FetchOutcome::Rejected(BlameRejected::Binary));
        }
        buffer.extend(bytes);
    }
    Ok(FetchOutcome::Fetched(Bytes::from(buffer)))
}
