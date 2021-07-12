/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![type_length_limit = "1441792"]

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
use blobstore::{Loadable, LoadableError};
use bytes::Bytes;
use context::CoreContext;
use derived_data::{BonsaiDerived, BonsaiDerivedMapping, DeriveError};
use manifest::ManifestOps;
use metaconfig_types::BlameVersion;
use mononoke_types::blame::{Blame, BlameId, BlameMaybeRejected, BlameRejected};
use mononoke_types::blame_v2::BlameV2Id;
use mononoke_types::{ChangesetId, MPath};
use thiserror::Error;
use unodes::RootUnodeManifestId;

pub use compat::CompatBlame;
pub use fetch::{fetch_content_for_blame, FetchOutcome};
pub use mapping_v1::{BlameRoot, BlameRootMapping};
pub use mapping_v2::{RootBlameV2, RootBlameV2Mapping};

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
) -> Result<CompatBlame, BlameError> {
    let blame_version = repo.get_derived_data_config().enabled.blame_version;
    let root_unode = match blame_version {
        BlameVersion::V1 => {
            BlameRoot::derive(ctx, repo, csid).await?;
            RootUnodeManifestId::derive(&ctx, &repo, csid).await?
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
        .ok_or_else(|| BlameError::IsDirectory(path))?;
    match blame_version {
        BlameVersion::V1 => {
            let blame = BlameId::from(file_unode_id).load(ctx, &blobstore).await?;
            Ok(CompatBlame::V1(blame))
        }
        BlameVersion::V2 => {
            let blame = BlameV2Id::from(file_unode_id).load(ctx, &blobstore).await?;
            Ok(CompatBlame::V2(blame))
        }
    }
}

/// Fetch content and blame for a file with specified file path
///
/// Blame will be derived if it is not available yet.
pub async fn fetch_blame(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csid: ChangesetId,
    path: MPath,
) -> Result<(Bytes, Blame), BlameError> {
    let (blame_id, blame) = match fetch_blame_if_derived(ctx, repo, csid, path).await? {
        Ok((blame_id, blame)) => (blame_id, blame),
        Err(blame_id) => {
            BlameRoot::derive(ctx, repo, csid).await?;
            match blame_id
                .load(ctx, repo.blobstore())
                .await
                .map_err(Error::from)?
            {
                BlameMaybeRejected::Blame(blame) => (blame_id, blame),
                BlameMaybeRejected::Rejected(reason) => return Err(BlameError::Rejected(reason)),
            }
        }
    };
    let mapping = BlameRoot::default_mapping(ctx, repo)?;
    // TODO(mbthomas): remove file content fetching - the caller can fetch the
    // content if they want it.
    let content = fetch_content_for_blame(ctx, repo, blame_id.into(), mapping.options())
        .await
        .map_err(BlameError::Error)?
        .into_bytes()
        .map_err(BlameError::Rejected)?;
    Ok((content, blame))
}

async fn fetch_blame_if_derived(
    ctx: &CoreContext,
    repo: &BlobRepo,
    csid: ChangesetId,
    path: MPath,
) -> Result<Result<(BlameId, Blame), BlameId>, BlameError> {
    let blobstore = repo.get_blobstore();
    let mf_root = RootUnodeManifestId::derive(&ctx, &repo, csid).await?;
    let entry_opt = mf_root
        .manifest_unode_id()
        .clone()
        .find_entry(ctx.clone(), blobstore.clone(), Some(path.clone()))
        .await?;
    let entry = entry_opt.ok_or_else(|| BlameError::NoSuchPath(path.clone()))?;
    let blame_id = match entry.into_leaf() {
        None => return Err(BlameError::IsDirectory(path)),
        Some(file_unode_id) => BlameId::from(file_unode_id),
    };
    match blame_id.load(ctx, &blobstore).await {
        Ok(BlameMaybeRejected::Blame(blame)) => Ok(Ok((blame_id, blame))),
        Ok(BlameMaybeRejected::Rejected(reason)) => Err(BlameError::Rejected(reason)),
        Err(LoadableError::Error(error)) => Err(error.into()),
        Err(LoadableError::Missing(_)) => Ok(Err(blame_id)),
    }
}
