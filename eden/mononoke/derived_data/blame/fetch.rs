/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{anyhow, Result};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bytes::Bytes;
use context::CoreContext;
use derived_data::{BonsaiDerivedMapping, BonsaiDerivedOld};
use filestore::{self, FetchKey};
use futures::TryStreamExt;
use metaconfig_types::BlameVersion;
use mononoke_types::blame::BlameRejected;
use mononoke_types::FileUnodeId;
use repo_blobstore::RepoBlobstore;

use crate::mapping_v1::BlameRoot;
use crate::mapping_v2::RootBlameV2;
use crate::BlameDeriveOptions;

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
    options: impl Into<Option<BlameDeriveOptions>>,
) -> Result<FetchOutcome> {
    let options = match options.into() {
        Some(options) => options,
        None => match repo.get_derived_data_config().enabled.blame_version {
            BlameVersion::V1 => BlameRoot::default_mapping(ctx, repo)?.options(),
            BlameVersion::V2 => RootBlameV2::default_mapping(ctx, repo)?.options(),
        },
    };
    let blobstore = repo.blobstore();
    fetch_content_for_blame_with_options(ctx, &blobstore, file_unode_id, options).await
}

/// Fetch the content of a file ready for blame.  If the file content is
/// too large or binary data is detected then the fetch may be rejected.
pub async fn fetch_content_for_blame_with_options(
    ctx: &CoreContext,
    blobstore: &RepoBlobstore,
    file_unode_id: FileUnodeId,
    options: BlameDeriveOptions,
) -> Result<FetchOutcome> {
    let file_unode = file_unode_id.load(ctx, blobstore).await?;
    let content_id = *file_unode.content_id();
    let (mut stream, size) =
        filestore::fetch_with_size(blobstore, ctx.clone(), &FetchKey::Canonical(content_id))
            .await?
            .ok_or_else(|| anyhow!("Missing content: {}", content_id))?;
    if size > options.filesize_limit {
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
