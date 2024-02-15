/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

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
use mononoke_types::NonRootMPath;
use thiserror::Error;

mod derive;
pub mod mapping;

pub use mapping::format_key;
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
    pub from_path: NonRootMPath,

    /// Unode ID of the file in the parent changeset.
    pub unode_id: FileUnodeId,
}

/// Given a bonsai changeset, find sources for all of the renames that
/// happened in this changeset.
///
/// Returns a mapping from paths in the current changeset to the source of the
/// rename in the parent changesets.
///
/// Pre-condition: RootUnodeManifestId has been derived for this bonsai
pub async fn find_unode_rename_sources(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
) -> Result<HashMap<NonRootMPath, UnodeRenameSource>, Error> {
    // Collect together a map of (source_path -> [dest_paths]) for each parent
    // changeset.
    let mut references: HashMap<ChangesetId, HashMap<&NonRootMPath, Vec<&NonRootMPath>>> =
        HashMap::new();
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
                .fetch_dependency::<RootUnodeManifestId>(ctx, csid)
                .await?;
            let from_paths: Vec<_> = paths.keys().cloned().cloned().collect();
            let unodes = mf_root
                .manifest_unode_id()
                .find_entries(ctx.clone(), blobstore, from_paths)
                .try_collect::<Vec<_>>()
                .await?;

            let mut sources = Vec::new();
            for (from_path, entry) in unodes {
                if let (Some(from_path), Some(unode_id)) =
                    (Option::<NonRootMPath>::from(from_path), entry.into_leaf())
                {
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

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use blobstore::Loadable;
    use bonsai_hg_mapping::BonsaiHgMapping;
    use bookmarks::Bookmarks;
    use borrowed::borrowed;
    use changeset_fetcher::ChangesetFetcher;
    use changesets::Changesets;
    use context::CoreContext;
    use fbinit::FacebookInit;
    use filenodes::Filenodes;
    use filestore::FilestoreConfig;
    use mononoke_types::NonRootMPath;
    use repo_blobstore::RepoBlobstore;
    use repo_derived_data::RepoDerivedData;
    use repo_derived_data::RepoDerivedDataRef;
    use repo_identity::RepoIdentity;
    use tests_utils::CreateCommitContext;

    use crate::RootUnodeManifestId;

    #[derive(Clone)]
    #[facet::container]
    pub(crate) struct TestRepo {
        #[facet]
        pub(crate) bonsai_hg_mapping: dyn BonsaiHgMapping,
        #[facet]
        pub(crate) bookmarks: dyn Bookmarks,
        #[facet]
        pub(crate) repo_blobstore: RepoBlobstore,
        #[facet]
        pub(crate) repo_derived_data: RepoDerivedData,
        #[facet]
        pub(crate) filestore_config: FilestoreConfig,
        #[facet]
        pub(crate) changeset_fetcher: dyn ChangesetFetcher,
        #[facet]
        pub(crate) changesets: dyn Changesets,
        #[facet]
        pub(crate) filenodes: dyn Filenodes,
        #[facet]
        pub(crate) repo_identity: RepoIdentity,
    }

    #[fbinit::test]
    async fn test_find_unode_rename_sources(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;
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

        let bonsai = c4.load(ctx, &repo.repo_blobstore).await?;
        let derivation_ctx = repo.repo_derived_data.manager().derivation_context(None);

        repo.repo_derived_data()
            .manager()
            .derive::<RootUnodeManifestId>(&ctx, c4, None)
            .await?;
        let renames = crate::find_unode_rename_sources(ctx, &derivation_ctx, &bonsai).await?;

        let check = |path: &str, parent_index: usize, from_path: &str| {
            let source = renames
                .get(&NonRootMPath::new(path).unwrap())
                .expect("path should exist");
            assert_eq!(source.parent_index, parent_index);
            assert_eq!(source.from_path, NonRootMPath::new(from_path).unwrap());
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
