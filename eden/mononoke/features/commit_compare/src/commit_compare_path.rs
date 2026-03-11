/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;

use anyhow::Result;
use anyhow::anyhow;
use futures::try_join;
use mononoke_api::ChangesetContext;
use mononoke_api::ChangesetPathDiffContext;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::MononokeError;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use mononoke_types::path::MPath;
use source_control as source_control_thrift;

use crate::conversions::convert_copy_info;
use crate::conversions::to_file_path_info;
use crate::conversions::to_tree_path_info;
use crate::identity::map_commit_identity;

/// Intermediate type for a single commit-compare path diff result.
pub enum CommitComparePath {
    File(source_control_thrift::CommitCompareFile),
    Tree(source_control_thrift::CommitCompareTree),
}

impl CommitComparePath {
    /// The main path that this comparison applies to.
    pub fn path(&self) -> Result<&str> {
        match self {
            CommitComparePath::File(file) => file
                .base_file
                .as_ref()
                .or(file.other_file.as_ref())
                .map(|file| file.path.as_str())
                .ok_or_else(|| anyhow!("programming error, file entry has no file")),
            CommitComparePath::Tree(tree) => tree
                .base_tree
                .as_ref()
                .or(tree.other_tree.as_ref())
                .map(|tree| tree.path.as_str())
                .ok_or_else(|| anyhow!("programming error, tree entry has no tree")),
        }
    }

    pub async fn from_path_diff(
        path_diff: ChangesetPathDiffContext<Repo>,
        schemes: &BTreeSet<source_control_thrift::CommitIdentityScheme>,
    ) -> Result<Self> {
        if path_diff.is_file() {
            let (base_file, other_file): (
                Option<source_control_thrift::FilePathInfo>,
                Option<source_control_thrift::FilePathInfo>,
            ) = try_join!(
                to_file_path_info(path_diff.get_new_content()),
                to_file_path_info(path_diff.get_old_content())
            )?;
            let copy_info = convert_copy_info(path_diff.copy_info());
            let (other_file, subtree_source) = match (
                path_diff.get_old_content(),
                path_diff.subtree_copy_dest_path(),
                other_file,
            ) {
                (Some(other), Some(replacement_path), Some(mut other_file)) => {
                    let source_commit_ids = map_commit_identity(other.changeset(), schemes).await?;
                    let source_path =
                        std::mem::replace(&mut other_file.path, replacement_path.to_string());
                    (
                        Some(other_file),
                        Some(source_control_thrift::CommitCompareSubtreeSource {
                            source_commit_ids,
                            source_path,
                            ..Default::default()
                        }),
                    )
                }
                (_, _, other_file) => (other_file, None),
            };
            Ok(CommitComparePath::File(
                source_control_thrift::CommitCompareFile {
                    base_file,
                    other_file,
                    copy_info,
                    subtree_source,
                    ..Default::default()
                },
            ))
        } else {
            let (base_tree, other_tree) = try_join!(
                to_tree_path_info(path_diff.get_new_content()),
                to_tree_path_info(path_diff.get_old_content())
            )?;
            Ok(CommitComparePath::Tree(
                source_control_thrift::CommitCompareTree {
                    base_tree,
                    other_tree,
                    ..Default::default()
                },
            ))
        }
    }
}

/// Helper for commit_compare to add mutable rename information if appropriate.
pub async fn add_mutable_renames(
    base_changeset: &mut ChangesetContext<Repo>,
    params: &source_control_thrift::CommitCompareParams,
) -> Result<()> {
    if params.follow_mutable_file_history.unwrap_or(false) {
        if let Some(paths) = &params.paths {
            let paths: Vec<_> = paths
                .iter()
                .map(MPath::try_from)
                .collect::<Result<Vec<_>, _>>()
                .map_err(|error| MononokeError::InvalidRequest(error.to_string()))?;
            base_changeset
                .add_mutable_renames(paths.into_iter())
                .await?;
        }
    }
    Ok(())
}

/// Given a base changeset, find the "other" changeset from parent information
/// including mutable history if appropriate.
pub async fn find_commit_compare_parent(
    repo: &RepoContext<Repo>,
    base_changeset: &mut ChangesetContext<Repo>,
    params: &source_control_thrift::CommitCompareParams,
) -> Result<Option<ChangesetContext<Repo>>> {
    let commit_parents = base_changeset.parents().await?;
    let mut other_changeset_id = commit_parents.first().copied();

    if params.follow_mutable_file_history.unwrap_or(false) {
        let mutable_parents = base_changeset.mutable_parents();

        if mutable_parents.len() > 1 {
            return Err(MononokeError::InvalidRequest(
                "multiple different mutable parents in supplied paths".to_string(),
            )
            .into());
        }
        if let Some(Some(parent)) = mutable_parents.into_iter().next() {
            other_changeset_id = Some(parent);
        }
    }

    match other_changeset_id {
        None => Ok(None),
        Some(other_changeset_id) => {
            let other_changeset = repo
                .changeset(ChangesetSpecifier::Bonsai(other_changeset_id))
                .await?
                .ok_or_else(|| anyhow!("other changeset is missing"))?;
            Ok(Some(other_changeset))
        }
    }
}
