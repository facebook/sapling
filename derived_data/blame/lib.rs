/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

mod derived;
pub use derived::{fetch_file_full_content, BlameRoot, BlameRootMapping};

#[cfg(test)]
mod tests;

use blobrepo::BlobRepo;
use blobstore::{Loadable, LoadableError};
use bytes::Bytes;
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use failure::{format_err, Error};
use futures::{future, Future};
use futures_ext::FutureExt;
use manifest::ManifestOps;
use mononoke_types::{
    blame::{Blame, BlameId, BlameMaybeRejected, BlameRejected},
    ChangesetId, MPath,
};
use std::sync::Arc;
use thiserror::Error;
use unodes::{RootUnodeManifestId, RootUnodeManifestMapping};

#[derive(Debug, Error)]
pub enum BlameError {
    #[error("{0}")]
    Rejected(BlameRejected),
    #[error("{0}")]
    Error(Error),
}

impl From<Error> for BlameError {
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

impl From<BlameRejected> for BlameError {
    fn from(rejected: BlameRejected) -> Self {
        Self::Rejected(rejected)
    }
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
                Err(blame_id) => {
                    let blame_mapping =
                        Arc::new(BlameRootMapping::new(repo.get_blobstore().boxed()));
                    BlameRoot::derive(ctx.clone(), repo.clone(), blame_mapping, csid)
                        .and_then(move |_| {
                            blame_id
                                .load(ctx.clone(), &repo.get_blobstore())
                                .from_err()
                                .and_then(|blame_maybe_rejected| blame_maybe_rejected.into_blame())
                                .map(move |blame| (blame_id, blame))
                        })
                        .from_err()
                        .right_future()
                }
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
    let unodes_mapping = Arc::new(RootUnodeManifestMapping::new(blobstore.clone()));
    RootUnodeManifestId::derive(ctx.clone(), repo, unodes_mapping, csid)
        .and_then({
            cloned!(ctx, blobstore, path);
            move |mf_root| {
                mf_root
                    .manifest_unode_id()
                    .clone()
                    .find_entry(ctx, blobstore, Some(path))
            }
        })
        .and_then({
            cloned!(path);
            move |entry_opt| {
                let entry = entry_opt.ok_or_else(|| format_err!("No such path: {}", path))?;
                match entry.into_leaf() {
                    None => Err(format_err!(
                        "Blame is not available for directories: {}",
                        path
                    )),
                    Some(file_unode_id) => Ok(BlameId::from(file_unode_id)),
                }
            }
        })
        .from_err()
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
