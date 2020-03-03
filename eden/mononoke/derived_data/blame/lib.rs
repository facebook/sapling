/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![type_length_limit = "1441792"]

mod derived;
pub use derived::{fetch_file_full_content, BlameRoot, BlameRootMapping};

#[cfg(test)]
mod tests;

use anyhow::Error;
use blobrepo::BlobRepo;
use blobstore::{Loadable, LoadableError};
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derived_data::{BonsaiDerived, DeriveError};
use futures_ext::FutureExt;
use futures_old::{future, Future};
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
    #[error("{0}")]
    Rejected(#[from] BlameRejected),
    #[error("{0}")]
    DeriveError(#[from] DeriveError),
    #[error("{0}")]
    Error(#[from] Error),
}

/// Fetch content and blame for a file with specified file path
///
/// Blame will be derived if it is not available yet.
pub fn fetch_blame(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: MPath,
) -> impl Future<Item = (Bytes, Blame), Error = BlameError> {
    fetch_blame_if_derived(ctx.clone(), repo.clone(), csid, path)
        .and_then({
            cloned!(ctx, repo);
            move |result| match result {
                Ok((blame_id, blame)) => future::ok((blame_id, blame)).left_future(),
                Err(blame_id) => BlameRoot::derive(ctx.clone(), repo.clone(), csid)
                    .from_err()
                    .and_then(move |_| {
                        blame_id
                            .load(ctx.clone(), repo.blobstore())
                            .from_err()
                            .and_then(|blame_maybe_rejected| blame_maybe_rejected.into_blame())
                            .map(move |blame| (blame_id, blame))
                            .from_err()
                    })
                    .right_future(),
            }
        })
        .and_then(move |(blame_id, blame)| {
            derived::fetch_file_full_content(ctx, repo.get_blobstore().boxed(), blame_id.into())
                .and_then(|result| result.map_err(Error::from))
                .map(|content| (content, blame))
                .from_err()
        })
}

fn fetch_blame_if_derived(
    ctx: CoreContext,
    repo: BlobRepo,
    csid: ChangesetId,
    path: MPath,
) -> impl Future<Item = Result<(BlameId, Blame), BlameId>, Error = BlameError> {
    let blobstore = repo.get_blobstore();
    RootUnodeManifestId::derive(ctx.clone(), repo, csid)
        .from_err()
        .and_then({
            cloned!(ctx, blobstore, path);
            move |mf_root| {
                mf_root
                    .manifest_unode_id()
                    .clone()
                    .find_entry(ctx, blobstore, Some(path))
            }
        })
        .from_err()
        .and_then({
            cloned!(path);
            move |entry_opt| {
                let entry = entry_opt.ok_or_else(|| BlameError::NoSuchPath(path.clone()))?;
                match entry.into_leaf() {
                    None => Err(BlameError::IsDirectory(path)),
                    Some(file_unode_id) => Ok(BlameId::from(file_unode_id)),
                }
            }
        })
        .and_then({
            cloned!(ctx, blobstore);
            move |blame_id| {
                blame_id
                    .load(ctx.clone(), &blobstore)
                    .then(move |result| match result {
                        Ok(BlameMaybeRejected::Blame(blame)) => Ok(Ok((blame_id, blame))),
                        Ok(BlameMaybeRejected::Rejected(reason)) => Err(reason.into()),
                        Err(LoadableError::Error(error)) => Err(error.into()),
                        Err(LoadableError::Missing(_)) => Ok(Err(blame_id)),
                    })
            }
        })
}
