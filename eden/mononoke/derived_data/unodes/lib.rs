/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Error;
use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use futures::{future, TryFutureExt, TryStreamExt};
use manifest::ManifestOps;
use mononoke_types::{BonsaiChangeset, ChangesetId, FileUnodeId, MPath};
use std::collections::HashMap;
use thiserror::Error;

mod derive;
mod mapping;

pub use mapping::{RootUnodeManifestId, RootUnodeManifestMapping};

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Invalid bonsai changeset: {0}")]
    InvalidBonsai(String),
}

/// Given bonsai changeset find unodes for all renames that happened in this changesest.
///
/// Returns mapping from paths in current changeset to file unodes in parents changesets
/// that were coppied to a given path.
pub async fn find_unode_renames(
    ctx: CoreContext,
    repo: BlobRepo,
    bonsai: &BonsaiChangeset,
) -> Result<HashMap<MPath, FileUnodeId>, Error> {
    let mut references: HashMap<ChangesetId, HashMap<MPath, MPath>> = HashMap::new();
    for (to_path, file_change) in bonsai.file_changes() {
        if let Some((from_path, csid)) = file_change.and_then(|fc| fc.copy_from()) {
            references
                .entry(*csid)
                .or_default()
                .insert(from_path.clone(), to_path.clone());
        }
    }

    let blobstore = repo.get_blobstore();
    let unodes = references.into_iter().map(|(csid, mut paths)| {
        cloned!(ctx, blobstore, repo);
        async move {
            let mf_root = RootUnodeManifestId::derive(&ctx, &repo, csid).await?;
            let from_paths: Vec<_> = paths.keys().cloned().collect();
            mf_root
                .manifest_unode_id()
                .clone()
                .find_entries(ctx, blobstore, from_paths)
                .map_ok(|(from_path, entry)| Some((from_path?, entry.into_leaf()?)))
                .try_filter_map(future::ok)
                .try_collect::<Vec<_>>()
                .map_ok(move |unodes| {
                    unodes
                        .into_iter()
                        .filter_map(|(from_path, unode_id)| {
                            Some((paths.remove(&from_path)?, unode_id))
                        })
                        .collect::<HashMap<_, _>>()
                })
                .await
        }
    });

    future::try_join_all(unodes)
        .map_ok(|unodes| unodes.into_iter().flatten().collect())
        .await
}
