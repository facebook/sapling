/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::future;
use futures::TryFutureExt;
use futures::TryStreamExt;
use manifest::ManifestOps;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::FileUnodeId;
use mononoke_types::MPath;
use std::collections::HashMap;
use thiserror::Error;

mod derive;
mod mapping;

pub use mapping::RootUnodeManifestId;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Invalid bonsai changeset: {0}")]
    InvalidBonsai(String),
}

/// A rename source for a file that is renamed.
#[derive(Debug, Clone)]
pub struct UnodeRenameSource {
    /// Index of the parent changeset in the list of parents in the bonsai
    /// changeset.
    pub parent_index: usize,

    /// Path of the file in the parent changeset (i.e., the path it was
    /// renamed from).
    pub from_path: MPath,

    /// Unode ID of the file in the parent changeset.
    pub unode_id: FileUnodeId,
}

/// Given a bonsai changeset, find sources for all of the renames that
/// happened in this changeset.
///
/// Returns a mapping from paths in the current changeset to the source of the
/// rename in the parent changesets.
pub async fn find_unode_rename_sources(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
) -> Result<HashMap<MPath, UnodeRenameSource>, Error> {
    // Collect together a map of (source_path -> [dest_paths]) for each parent
    // changeset.
    let mut references: HashMap<ChangesetId, HashMap<&MPath, Vec<&MPath>>> = HashMap::new();
    for (to_path, file_change) in bonsai.file_changes() {
        if let Some((from_path, csid)) = file_change.copy_from() {
            references
                .entry(*csid)
                .or_insert_with(HashMap::new)
                .entry(from_path)
                .or_insert_with(Vec::new)
                .push(to_path);
        }
    }

    let blobstore = derivation_ctx.blobstore();
    let sources_futs = references.into_iter().map(|(csid, mut paths)| {
        cloned!(blobstore);
        async move {
            let parent_index = bonsai.parents().position(|p| p == csid).ok_or_else(|| {
                anyhow!(
                    "bonsai changeset {} contains invalid copy from parent: {}",
                    bonsai.get_changeset_id(),
                    csid
                )
            })?;
            let mf_root = derivation_ctx
                .derive_dependency::<RootUnodeManifestId>(ctx, csid)
                .await?;
            let from_paths: Vec<_> = paths.keys().cloned().cloned().collect();
            let unodes = mf_root
                .manifest_unode_id()
                .find_entries(ctx.clone(), blobstore, from_paths)
                .try_collect::<Vec<_>>()
                .await?;

            let mut sources = Vec::new();
            for (from_path, entry) in unodes {
                if let (Some(from_path), Some(unode_id)) = (from_path, entry.into_leaf()) {
                    if let Some(to_paths) = paths.remove(&from_path) {
                        for to_path in to_paths {
                            sources.push((
                                to_path.clone(),
                                UnodeRenameSource {
                                    parent_index,
                                    from_path: from_path.clone(),
                                    unode_id,
                                },
                            ));
                        }
                    }
                }
            }
            Ok(sources)
        }
    });

    future::try_join_all(sources_futs)
        .map_ok(|unodes| unodes.into_iter().flatten().collect())
        .await
}

/// Given bonsai changeset find unodes for all renames that happened in this changesest.
///
/// Returns mapping from paths in current changeset to file unodes in parents changesets
/// that were coppied to a given path.
///
/// This version of the function is incorrect: it fails to take into account
/// files that are copied multiple times.  The function is retained for
/// blame_v1 compatibility.
pub async fn find_unode_renames_incorrect_for_blame_v1(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
) -> Result<HashMap<MPath, FileUnodeId>, Error> {
    let mut references: HashMap<ChangesetId, HashMap<MPath, MPath>> = HashMap::new();
    for (to_path, file_change) in bonsai.file_changes() {
        if let Some((from_path, csid)) = file_change.copy_from() {
            references
                .entry(*csid)
                .or_default()
                .insert(from_path.clone(), to_path.clone());
        }
    }

    let unodes = references.into_iter().map(|(csid, mut paths)| async move {
        let mf_root = derivation_ctx
            .derive_dependency::<RootUnodeManifestId>(ctx, csid)
            .await?;
        let from_paths: Vec<_> = paths.keys().cloned().collect();
        let blobstore = derivation_ctx.blobstore();
        mf_root
            .manifest_unode_id()
            .clone()
            .find_entries(ctx.clone(), blobstore.clone(), from_paths)
            .map_ok(|(from_path, entry)| Some((from_path?, entry.into_leaf()?)))
            .try_filter_map(future::ok)
            .try_collect::<Vec<_>>()
            .map_ok(move |unodes| {
                unodes
                    .into_iter()
                    .filter_map(|(from_path, unode_id)| Some((paths.remove(&from_path)?, unode_id)))
                    .collect::<HashMap<_, _>>()
            })
            .await
    });

    future::try_join_all(unodes)
        .map_ok(|unodes| unodes.into_iter().flatten().collect())
        .await
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use blobrepo::BlobRepo;
    use blobstore::Loadable;
    use borrowed::borrowed;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use mononoke_types::MPath;
    use repo_derived_data::RepoDerivedDataRef;
    use tests_utils::CreateCommitContext;

    #[fbinit::test]
    async fn test_find_unode_rename_sources(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: BlobRepo = test_repo_factory::build_empty(fb)?;
        borrowed!(ctx, repo);

        let c1 = CreateCommitContext::new_root(ctx, repo)
            .add_file("file1", "content")
            .commit()
            .await?;
        let c2 = CreateCommitContext::new(ctx, repo, vec![c1])
            .add_file("file2", "content")
            .commit()
            .await?;
        let c3 = CreateCommitContext::new(ctx, repo, vec![c1])
            .add_file("file3", "content")
            .commit()
            .await?;
        let c4 = CreateCommitContext::new(ctx, repo, vec![c2, c3])
            .add_file_with_copy_info("file1a", "content a", (c2, "file1"))
            .delete_file("file1")
            .add_file_with_copy_info("file2a", "content a", (c2, "file2"))
            .add_file_with_copy_info("file2b", "content b", (c2, "file2"))
            .add_file_with_copy_info("file3a", "content a", (c3, "file3"))
            .add_file_with_copy_info("file3b", "content b", (c3, "file3"))
            .commit()
            .await?;

        let bonsai = c4.load(ctx, repo.blobstore()).await?;
        let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);
        let renames = crate::find_unode_rename_sources(ctx, &derivation_ctx, &bonsai).await?;

        let check = |path: &str, parent_index: usize, from_path: &str| {
            let source = renames
                .get(&MPath::new(path).unwrap())
                .expect("path should exist");
            assert_eq!(source.parent_index, parent_index);
            assert_eq!(source.from_path, MPath::new(from_path).unwrap());
        };

        check("file1a", 0, "file1");
        check("file2a", 0, "file2");
        check("file2b", 0, "file2");
        check("file3a", 1, "file3");
        check("file3b", 1, "file3");

        assert_eq!(renames.len(), 5);

        Ok(())
    }
}
