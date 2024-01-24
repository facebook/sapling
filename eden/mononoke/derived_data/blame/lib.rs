/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "1441792"]

mod batch_v2;
mod derive_v2;
mod fetch;
mod mapping_v2;

#[cfg(test)]
mod tests;

use anyhow::Error;
use blobstore::Loadable;
use blobstore::LoadableError;
use context::CoreContext;
use derived_data::BonsaiDerived;
use derived_data::DerivationError;
pub use fetch::fetch_content_for_blame;
pub use fetch::FetchOutcome;
use manifest::ManifestOps;
pub use mapping_v2::format_key;
pub use mapping_v2::RootBlameV2;
use metaconfig_types::BlameVersion;
use mononoke_types::blame_v2::BlameRejected;
use mononoke_types::blame_v2::BlameV2;
use mononoke_types::blame_v2::BlameV2Id;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::NonRootMPath;
use repo_blobstore::RepoBlobstoreArc;
use repo_derived_data::RepoDerivedDataRef;
use thiserror::Error;

pub const DEFAULT_BLAME_FILESIZE_LIMIT: u64 = 10 * 1024 * 1024;

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct BlameDeriveOptions {
    filesize_limit: u64,
    blame_version: BlameVersion,
}

impl Default for BlameDeriveOptions {
    fn default() -> Self {
        BlameDeriveOptions {
            filesize_limit: DEFAULT_BLAME_FILESIZE_LIMIT,
            blame_version: BlameVersion::default(),
        }
    }
}

#[derive(Debug, Error)]
pub enum BlameError {
    #[error("No such path: {0}")]
    NoSuchPath(NonRootMPath),
    #[error("Blame is not available for directories: {0}")]
    IsDirectory(NonRootMPath),
    #[error(transparent)]
    Rejected(#[from] BlameRejected),
    #[error(transparent)]
    DerivationError(#[from] DerivationError),
    #[error(transparent)]
    LoadableError(#[from] LoadableError),
    #[error(transparent)]
    Error(#[from] Error),
}

/// Fetch the blame for a file.  Blame will be derived if necessary.
pub async fn fetch_blame_v2(
    ctx: &CoreContext,
    repo: impl RepoBlobstoreArc + RepoDerivedDataRef + Sync + Send + Copy,
    csid: ChangesetId,
    path: NonRootMPath,
) -> Result<(BlameV2, FileUnodeId), BlameError> {
    let root_unode = RootBlameV2::derive(ctx, &repo, csid).await?.root_manifest();
    let blobstore = repo.repo_blobstore();
    let file_unode_id = root_unode
        .manifest_unode_id()
        .clone()
        .find_entry(ctx.clone(), blobstore.clone(), path.clone().into())
        .await?
        .ok_or_else(|| BlameError::NoSuchPath(path.clone()))?
        .into_leaf()
        .ok_or(BlameError::IsDirectory(path))?;
    let blame = BlameV2Id::from(file_unode_id).load(ctx, &blobstore).await?;
    Ok((blame, file_unode_id))
}
