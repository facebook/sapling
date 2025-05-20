/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use context::CoreContext;
use futures::TryStreamExt;
use futures::future::try_join_all;
use manifest::get_implicit_deletes;
use mononoke_types::ChangesetId;
use mononoke_types::FileChange;
use mononoke_types::NonRootMPath;
use mononoke_types::SkeletonManifestId;
use skeleton_manifest::RootSkeletonManifestId;
use sorted_vector_map::SortedVectorMap;

use crate::types::MultiMover;
use crate::types::Repo;

/// Given a changeset and it's parents, get the list of file
/// changes, which arise from "implicit deletes" as opposed
/// to naive `NonRootMPath` rewriting in `cs.file_changes`. For
/// more information about implicit deletes, please see
/// `manifest/src/implici_deletes.rs`
pub async fn get_renamed_implicit_deletes<'a, I: IntoIterator<Item = ChangesetId>>(
    ctx: &'a CoreContext,
    file_changes: Vec<(&NonRootMPath, &FileChange)>,
    parent_changeset_ids: I,
    mover: Arc<dyn MultiMover + 'a>,
    source_repo: &'a impl Repo,
) -> Result<Vec<Vec<NonRootMPath>>, Error> {
    let parent_manifest_ids =
        get_skeleton_manifest_ids(ctx, source_repo, parent_changeset_ids).await?;

    // Get all the paths that were added or modified and thus are capable of
    // implicitly deleting existing directories.
    let paths_added: Vec<_> = file_changes
        .into_iter()
        .filter(|&(_mpath, file_change)| file_change.is_changed())
        .map(|(mpath, _file_change)| mpath.clone())
        .collect();

    let store = source_repo.repo_blobstore().clone();
    let implicit_deletes: Vec<NonRootMPath> =
        get_implicit_deletes(ctx, store, paths_added, parent_manifest_ids)
            .try_collect()
            .await?;
    implicit_deletes
        .iter()
        .map(|mpath| mover.multi_move_path(mpath))
        .collect()
}

/// Take an iterator of file changes, which may contain implicit deletes
/// and produce a `SortedVectorMap` suitable to be used in the `BonsaiChangeset`,
/// without any implicit deletes.
pub(crate) fn minimize_file_change_set<I: IntoIterator<Item = (NonRootMPath, FileChange)>>(
    file_changes: I,
) -> SortedVectorMap<NonRootMPath, FileChange> {
    let (adds, removes): (Vec<_>, Vec<_>) = file_changes
        .into_iter()
        .partition(|(_, fc)| fc.is_changed());
    let mut adds: SortedVectorMap<NonRootMPath, FileChange> = adds.into_iter().collect();

    let prefix_path_was_added = |removed_path: NonRootMPath| {
        removed_path
            .into_non_root_ancestors()
            .any(|parent_dir| adds.contains_key(&parent_dir))
    };

    let filtered_removes = removes
        .into_iter()
        .filter(|(mpath, _)| !prefix_path_was_added(mpath.clone()));
    let mut result: SortedVectorMap<_, _> = filtered_removes.collect();
    result.append(&mut adds);
    result
}

/// Get `SkeletonManifestId`s for a set of `ChangesetId`s
/// This is needed for the purposes of implicit delete detection
async fn get_skeleton_manifest_ids<'a, I: IntoIterator<Item = ChangesetId>>(
    ctx: &'a CoreContext,
    repo: &'a impl Repo,
    bcs_ids: I,
) -> Result<Vec<SkeletonManifestId>, Error> {
    try_join_all(bcs_ids.into_iter().map({
        |bcs_id| async move {
            let repo_derived_data = repo.repo_derived_data();

            let root_skeleton_manifest_id = repo_derived_data
                .derive::<RootSkeletonManifestId>(ctx, bcs_id)
                .await?;

            Ok(root_skeleton_manifest_id.into_skeleton_manifest_id())
        }
    }))
    .await
}
