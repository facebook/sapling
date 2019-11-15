/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![deny(warnings)]

use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use derived_data::BonsaiDerived;
use failure_ext::Error;
use futures::{future, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use manifest::ManifestOps;
use mononoke_types::{BonsaiChangeset, ChangesetId, FileUnodeId, MPath};
use std::{collections::HashMap, sync::Arc};
use thiserror::Error;

mod derive;
mod mapping;

pub use mapping::{RootUnodeManifestId, RootUnodeManifestMapping};

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Invalid bonsai changeset: {0}")]
    InvalidBonsai(String),
}

pub fn derive_unodes(ctx: CoreContext, repo: BlobRepo, cs_id: ChangesetId) -> BoxFuture<(), Error> {
    let unodes_derived_mapping = RootUnodeManifestMapping::new(repo.get_blobstore());
    RootUnodeManifestId::derive(ctx, repo, unodes_derived_mapping, cs_id)
        .map(|_| ())
        .boxify()
}

/// Given bonsai changeset find unodes for all renames that happened in this changesest.
///
/// Returns mapping from paths in current changeset to file unodes in parents changesets
/// that were coppied to a given path.
pub fn find_unode_renames(
    ctx: CoreContext,
    repo: BlobRepo,
    bonsai: &BonsaiChangeset,
) -> impl Future<Item = HashMap<MPath, FileUnodeId>, Error = Error> {
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
    let unodes_mapping = Arc::new(RootUnodeManifestMapping::new(blobstore.clone()));
    let unodes = references.into_iter().map(move |(csid, mut paths)| {
        RootUnodeManifestId::derive(ctx.clone(), repo.clone(), unodes_mapping.clone(), csid)
            .and_then({
                cloned!(ctx, blobstore);
                move |mf_root| {
                    let from_paths: Vec<_> = paths.keys().cloned().collect();
                    mf_root
                        .manifest_unode_id()
                        .clone()
                        .find_entries(ctx, blobstore, from_paths)
                        .filter_map(|(from_path, entry)| Some((from_path?, entry.into_leaf()?)))
                        .collect()
                        .map(move |unodes| {
                            unodes
                                .into_iter()
                                .filter_map(|(from_path, unode_id)| {
                                    Some((paths.remove(&from_path)?, unode_id))
                                })
                                .collect::<HashMap<_, _>>()
                        })
                }
            })
    });

    future::join_all(unodes).map(|unodes| unodes.into_iter().flatten().collect())
}
