/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![type_length_limit = "1441792"]

mod derived;
pub use derived::{fetch_file_full_content, BlameRoot, BlameRootMapping, BLAME_FILESIZE_LIMIT};

#[cfg(test)]
mod tests;

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::{Loadable, LoadableError};
use bytes::Bytes;
use context::CoreContext;
use derived_data::{BonsaiDerived, BonsaiDerivedMapping, DeriveError};
use manifest::ManifestOps;
use mononoke_types::{
    blame::{Blame, BlameId, BlameMaybeRejected, BlameRejected},
    ChangesetId, MPath,
};
use thiserror::Error;
use unodes::RootUnodeManifestId;

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
    Error(#[from] Error),
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
    let content = derived::fetch_file_full_content(ctx, repo, blame_id.into(), mapping.options())
        .await
        .map_err(BlameError::Error)?
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
