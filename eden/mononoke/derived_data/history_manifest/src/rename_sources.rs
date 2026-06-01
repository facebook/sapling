/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use cloned::cloned;
use context::CoreContext;
use derived_data_manager::DerivationContext;
use futures::TryFutureExt;
use futures::TryStreamExt;
use futures::future;
use manifest::ManifestOps;
use manifest::PathTree;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::MPath;
use mononoke_types::NonRootMPath;
use mononoke_types::subtree_change::SubtreeChange;
use mononoke_types::typed_hash::HistoryManifestFileId;
use unodes::SubtreeCopySource;
use unodes::SubtreeMergeSource;
use unodes::SubtreeOpSource;

use crate::RootHistoryManifestDirectoryId;

/// A rename source for a file that was copied by commit copy-info,
/// resolved via the history manifest tree.
#[derive(Debug, Clone)]
pub struct HmCopyInfoSource {
    /// Index of the parent changeset in the list of parents in the bonsai
    /// changeset.
    pub parent_index: usize,

    /// Path of the file in the parent changeset (i.e., the path it was
    /// renamed from).
    pub from_path: NonRootMPath,

    /// History manifest file ID of the file in the parent changeset.
    pub history_manifest_file_id: HistoryManifestFileId,
}

/// A rename source for a file, resolved via the history manifest.
#[derive(Debug, Clone)]
pub enum HmRenameSource {
    CopyInfo(HmCopyInfoSource),
    SubtreeCopy(SubtreeCopySource),
    SubtreeMerge(SubtreeMergeSource),
}

pub struct HmRenameSources {
    pub copy_info: HashMap<NonRootMPath, HmCopyInfoSource>,
    pub subtree_ops: PathTree<Option<SubtreeOpSource>>,
}

fn subtree_op_to_hm_rename_source(source: &SubtreeOpSource, suffix: &MPath) -> HmRenameSource {
    match source {
        SubtreeOpSource::Copy(source) => HmRenameSource::SubtreeCopy(SubtreeCopySource {
            parent: source.parent,
            from_path: source.from_path.join(suffix),
        }),
        SubtreeOpSource::Merge(source) => HmRenameSource::SubtreeMerge(SubtreeMergeSource {
            parent: source.parent,
            from_path: source.from_path.join(suffix),
        }),
    }
}

impl HmRenameSources {
    pub fn get(&self, path: &NonRootMPath) -> Option<HmRenameSource> {
        if let Some((source_to_path, Some(source))) = self
            .subtree_ops
            .get_nearest_parent(path.into(), Option::is_some)
        {
            let path: &MPath = path.into();
            let suffix = path.remove_prefix_component(&source_to_path);
            Some(subtree_op_to_hm_rename_source(source, &suffix))
        } else {
            self.copy_info.get(path).map(|source| {
                HmRenameSource::CopyInfo(HmCopyInfoSource {
                    parent_index: source.parent_index,
                    from_path: source.from_path.clone(),
                    history_manifest_file_id: source.history_manifest_file_id,
                })
            })
        }
    }
}

/// Given a bonsai changeset, find sources for all of the renames that
/// happened in this changeset, using the history manifest tree.
///
/// Returns a mapping from paths in the current changeset to the source of the
/// rename in the parent changesets.
///
/// Pre-condition: RootHistoryManifestDirectoryId has been derived for all
/// parent changesets.
pub async fn find_hm_rename_sources(
    ctx: &CoreContext,
    derivation_ctx: &DerivationContext,
    bonsai: &BonsaiChangeset,
) -> Result<HmRenameSources, Error> {
    // Collect together a map of (source_path -> [dest_paths]) for each parent
    // changeset.
    let mut references: HashMap<ChangesetId, HashMap<&NonRootMPath, Vec<&NonRootMPath>>> =
        HashMap::new();
    for (to_path, file_change) in bonsai.file_changes() {
        if let Some((from_path, csid)) = file_change.copy_from() {
            references
                .entry(*csid)
                .or_default()
                .entry(from_path)
                .or_default()
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
            let hm_root = derivation_ctx
                .fetch_dependency::<RootHistoryManifestDirectoryId>(ctx, csid)
                .await?;
            let from_paths: Vec<_> = paths.keys().cloned().cloned().collect();
            let entries = hm_root
                .0
                .find_entries(ctx.clone(), blobstore, from_paths)
                .try_collect::<Vec<_>>()
                .await?;

            let mut sources = Vec::new();
            for (from_path, entry) in entries {
                if let (Some(from_path), Some(hm_file_id)) =
                    (Option::<NonRootMPath>::from(from_path), entry.into_leaf())
                {
                    if let Some(to_paths) = paths.remove(&from_path) {
                        for to_path in to_paths {
                            sources.push((
                                to_path.clone(),
                                HmCopyInfoSource {
                                    parent_index,
                                    from_path: from_path.clone(),
                                    history_manifest_file_id: hm_file_id,
                                },
                            ));
                        }
                    }
                }
            }
            anyhow::Ok(sources)
        }
    });

    let copy_info = future::try_join_all(sources_futs)
        .map_ok(|sources| sources.into_iter().flatten().collect())
        .await?;

    let subtree_ops = PathTree::from_iter(bonsai.subtree_changes().iter().map(
        |(to_path, change)| match change {
            SubtreeChange::SubtreeCopy(copy) => (
                to_path.clone(),
                Some(SubtreeOpSource::Copy(SubtreeCopySource {
                    parent: copy.from_cs_id,
                    from_path: copy.from_path.clone(),
                })),
            ),
            SubtreeChange::SubtreeDeepCopy(copy) => (
                to_path.clone(),
                Some(SubtreeOpSource::Copy(SubtreeCopySource {
                    parent: copy.from_cs_id,
                    from_path: copy.from_path.clone(),
                })),
            ),
            SubtreeChange::SubtreeMerge(merge) => (
                to_path.clone(),
                Some(SubtreeOpSource::Merge(SubtreeMergeSource {
                    parent: merge.from_cs_id,
                    from_path: merge.from_path.clone(),
                })),
            ),
            SubtreeChange::SubtreeImport(_) => (to_path.clone(), None),
            SubtreeChange::SubtreeCrossRepoMerge(_) => (to_path.clone(), None),
        },
    ));

    Ok(HmRenameSources {
        copy_info,
        subtree_ops,
    })
}

#[cfg(test)]
mod tests {
    use anyhow::Result;
    use blobstore::Loadable;
    use bonsai_hg_mapping::BonsaiHgMapping;
    use bookmarks::Bookmarks;
    use commit_graph::CommitGraph;
    use commit_graph::CommitGraphWriter;
    use context::CoreContext;
    use derivation_queue_thrift::DerivationPriority;
    use fbinit::FacebookInit;
    use filestore::FilestoreConfig;
    use mononoke_macros::mononoke;
    use repo_blobstore::RepoBlobstore;
    use repo_blobstore::RepoBlobstoreRef;
    use repo_derived_data::RepoDerivedData;
    use repo_derived_data::RepoDerivedDataRef;
    use repo_identity::RepoIdentity;
    use tests_utils::CreateCommitContext;

    use super::*;

    #[facet::container]
    struct TestRepo(
        dyn BonsaiHgMapping,
        dyn Bookmarks,
        CommitGraph,
        dyn CommitGraphWriter,
        RepoDerivedData,
        RepoBlobstore,
        FilestoreConfig,
        RepoIdentity,
    );

    #[mononoke::fbinit_test]
    async fn test_find_hm_rename_sources(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo: TestRepo = test_repo_factory::build_empty(ctx.fb).await?;

        let c1 = CreateCommitContext::new_root(&ctx, &repo)
            .add_file("file1", "content")
            .commit()
            .await?;
        let c2 = CreateCommitContext::new(&ctx, &repo, vec![c1])
            .add_file("file2", "content")
            .commit()
            .await?;
        let c3 = CreateCommitContext::new(&ctx, &repo, vec![c1])
            .add_file("file3", "content")
            .commit()
            .await?;
        let c4 = CreateCommitContext::new(&ctx, &repo, vec![c2, c3])
            .add_file_with_copy_info("file1a", "content a", (c2, "file1"))
            .delete_file("file1")
            .add_file_with_copy_info("file2a", "content a", (c2, "file2"))
            .add_file_with_copy_info("file2b", "content b", (c2, "file2"))
            .add_file_with_copy_info("file3a", "content a", (c3, "file3"))
            .add_file_with_copy_info("file3b", "content b", (c3, "file3"))
            .commit()
            .await?;

        let bonsai = c4.load(&ctx, repo.repo_blobstore()).await?;
        let derivation_ctx = repo.repo_derived_data().manager().derivation_context(None);

        repo.repo_derived_data()
            .manager()
            .derive::<RootHistoryManifestDirectoryId>(&ctx, c4, None, DerivationPriority::LOW)
            .await?;
        let renames = find_hm_rename_sources(&ctx, &derivation_ctx, &bonsai).await?;

        let check = |path: &str, expected_parent_index: usize, expected_from_path: &str| {
            let source = renames
                .get(&NonRootMPath::new(path).unwrap())
                .expect("path should exist");
            match source {
                HmRenameSource::CopyInfo(copy) => {
                    assert_eq!(copy.parent_index, expected_parent_index);
                    assert_eq!(
                        copy.from_path,
                        NonRootMPath::new(expected_from_path).unwrap()
                    );
                }
                _ => panic!("expected CopyInfo rename source for {path}"),
            }
        };

        check("file1a", 0, "file1");
        check("file2a", 0, "file2");
        check("file2b", 0, "file2");
        check("file3a", 1, "file3");
        check("file3b", 1, "file3");

        assert_eq!(renames.copy_info.len(), 5);
        assert_eq!(
            renames
                .subtree_ops
                .into_iter()
                .filter(|(_path, source)| source.is_some())
                .count(),
            0
        );

        Ok(())
    }
}
