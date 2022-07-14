/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![type_length_limit = "1441792"]

mod batch_v2;
mod compat;
mod derive_v1;
mod derive_v2;
mod fetch;
mod mapping_v1;
mod mapping_v2;

#[cfg(test)]
mod tests;

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::Loadable;
use blobstore::LoadableError;
use context::CoreContext;
use derived_data::BonsaiDerived;
use derived_data::DeriveError;
use manifest::ManifestOps;
use metaconfig_types::BlameVersion;
use mononoke_types::blame::BlameId;
use mononoke_types::blame::BlameRejected;
use mononoke_types::blame_v2::BlameV2Id;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::MPath;
use thiserror::Error;
use unodes::RootUnodeManifestId;

pub use compat::CompatBlame;
pub use fetch::fetch_content_for_blame;
pub use fetch::FetchOutcome;
pub use mapping_v1::BlameRoot;
pub use mapping_v2::RootBlameV2;

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
    NoSuchPath(MPath),
    #[error("Blame is not available for directories: {0}")]
    IsDirectory(MPath),
    #[error(transparent)]
    Rejected(#[from] BlameRejected),
    #[error(transparent)]
    DeriveError(#[from] DeriveError),
    #[error(transparent)]
    LoadableError(#[from] LoadableError),
    #[error(transparent)]
    Error(#[from] Error),
}

/// Fetch the blame for a file.  Blame will be derived if necessary.
pub async fn fetch_blame_compat(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csid: ChangesetId,
    path: MPath,
) -> Result<(CompatBlame, FileUnodeId), BlameError> {
    let blame_version = repo.get_active_derived_data_types_config().blame_version;
    let root_unode = match blame_version {
        BlameVersion::V1 => {
            BlameRoot::derive(ctx, repo, csid).await?;
            RootUnodeManifestId::derive(ctx, repo, csid).await?
        }
        BlameVersion::V2 => {
            let root_blame = RootBlameV2::derive(ctx, repo, csid).await?;
            root_blame.root_manifest()
        }
    };
    let blobstore = repo.get_blobstore();
    let file_unode_id = root_unode
        .manifest_unode_id()
        .clone()
        .find_entry(ctx.clone(), blobstore.clone(), Some(path.clone()))
        .await?
        .ok_or_else(|| BlameError::NoSuchPath(path.clone()))?
        .into_leaf()
        .ok_or(BlameError::IsDirectory(path))?;
    match blame_version {
        BlameVersion::V1 => {
            let blame = BlameId::from(file_unode_id).load(ctx, &blobstore).await?;
            Ok((CompatBlame::V1(blame), file_unode_id))
        }
        BlameVersion::V2 => {
            let blame = BlameV2Id::from(file_unode_id).load(ctx, &blobstore).await?;
            Ok((CompatBlame::V2(blame), file_unode_id))
        }
    }
}
