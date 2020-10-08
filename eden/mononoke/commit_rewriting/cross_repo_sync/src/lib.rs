/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]
#![feature(trait_alias)]

use anyhow::{bail, format_err, Context, Error};
use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use blobsync::copy_content;
use bookmark_renaming::BookmarkRenamer;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use futures::future::try_join_all;
use futures::{
    compat::Future01CompatExt,
    future::{self, TryFutureExt},
    stream::{self, futures_unordered::FuturesUnordered, StreamExt, TryStreamExt},
};
use futures_old::Future;
use futures_old::Stream as StreamOld;
use live_commit_sync_config::LiveCommitSyncConfig;
use manifest::get_implicit_deletes;
use maplit::{hashmap, hashset};
use mercurial_types::HgManifestId;
use metaconfig_types::{CommitSyncConfig, CommitSyncConfigVersion, PushrebaseFlags};
use mononoke_types::{
    BonsaiChangeset, BonsaiChangesetMut, ChangesetId, FileChange, MPath, RepositoryId,
};
use movers::Mover;
use pushrebase::{do_pushrebase_bonsai, PushrebaseError};
use reachabilityindex::LeastCommonAncestorsHint;
use slog::{debug, info};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fmt;
use std::sync::Arc;
use synced_commit_mapping::{
    EquivalentWorkingCopyEntry, SyncedCommitMapping, SyncedCommitMappingEntry,
};
use thiserror::Error;
use topo_sort::sort_topological;

pub use commit_syncer_args::CommitSyncerArgs;
use merge_utils::get_version_for_merge;
use pushrebase_hook::CrossRepoSyncPushrebaseHook;
use types::{Source, Target};

mod commit_sync_data_provider;
pub mod commit_sync_outcome;
mod commit_syncer_args;
mod merge_utils;
mod pushrebase_hook;
pub mod types;
pub mod validation;

pub use crate::commit_sync_outcome::{
    commit_sync_outcome_exists, get_commit_sync_outcome, get_commit_sync_outcome_with_hint,
    get_plural_commit_sync_outcome, CandidateSelectionHint, CommitSyncOutcome,
    PluralCommitSyncOutcome,
};
pub use commit_sync_data_provider::{CommitSyncDataProvider, SyncData};

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Pushrebase of synced commit failed - check config for overlaps: {0:?}")]
    PushrebaseFailure(PushrebaseError),
    #[error("Remapped commit {0} expected in target repo, but not present")]
    MissingRemappedCommit(ChangesetId),
    #[error("Could not find a commit in the target repo with the same working copy as {0}")]
    SameWcSearchFail(ChangesetId),
    #[error("Parent commit {0} hasn't been remapped")]
    ParentNotRemapped(ChangesetId),
    #[error("Parent commit {0} is not a sync candidate")]
    ParentNotSyncCandidate(ChangesetId),
    #[error("Cannot choose working copy equivalent for {0}")]
    AmbiguousWorkingCopyEquivalent(ChangesetId),
}

async fn identity<T>(res: T) -> Result<T, Error> {
    Ok(res)
}

/// Get `HgManifestId`s for a set of `ChangesetId`s
/// This is needed for the purposes of implicit delete detection
async fn get_manifest_ids<I: IntoIterator<Item = ChangesetId>>(
    ctx: CoreContext,
    repo: &BlobRepo,
    bcs_ids: I,
) -> Result<Vec<HgManifestId>, Error> {
    try_join_all(bcs_ids.into_iter().map({
        |bcs_id| {
            cloned!(ctx, repo);
            async move {
                let cs_id = repo
                    .get_hg_from_bonsai_changeset(ctx.clone(), bcs_id)
                    .compat()
                    .await?;
                let hg_blob_changeset = cs_id.load(ctx, repo.blobstore()).await?;
                Ok(hg_blob_changeset.manifestid())
            }
        }
    }))
    .await
}

/// Take an iterator of file changes, which may contain implicit deletes
/// and produce a `BTreeMap` suitable to be used in the `BonsaiChangeset`,
/// without any implicit deletes.
fn minimize_file_change_set<FC, I: IntoIterator<Item = (MPath, Option<FC>)>>(
    file_changes: I,
) -> BTreeMap<MPath, Option<FC>> {
    let (adds, removes): (Vec<_>, Vec<_>) =
        file_changes.into_iter().partition(|(_, fc)| fc.is_some());
    let adds: HashMap<MPath, Option<FC>> = adds.into_iter().collect();

    let prefix_path_was_added = |removed_path: MPath| {
        removed_path
            .into_parent_dir_iter()
            .any(|parent_dir| adds.contains_key(&parent_dir))
    };

    let filtered_removes = removes
        .into_iter()
        .filter(|(ref mpath, _)| !prefix_path_was_added(mpath.clone()));
    let mut result: BTreeMap<_, _> = filtered_removes.collect();
    result.extend(adds.into_iter());
    result
}

/// Given a changeset and it's parents, get the list of file
/// changes, which arise from "implicit deletes" as opposed
/// to naive `MPath` rewriting in `cs.file_changes`. For
/// more information about implicit deletes, please see
/// `manifest/src/implici_deletes.rs`
async fn get_implicit_delete_file_changes<I: IntoIterator<Item = ChangesetId>>(
    ctx: CoreContext,
    cs: BonsaiChangesetMut,
    parent_changeset_ids: I,
    mover: Mover,
    source_repo: BlobRepo,
) -> Result<Vec<(MPath, Option<FileChange>)>, Error> {
    let parent_manifest_ids =
        get_manifest_ids(ctx.clone(), &source_repo, parent_changeset_ids).await?;
    let file_adds: Vec<_> = cs
        .file_changes
        .iter()
        .filter_map(|(mpath, maybe_file_change)| maybe_file_change.as_ref().map(|_| mpath.clone()))
        .collect();
    let store = source_repo.get_blobstore();
    let implicit_deletes: Vec<MPath> =
        get_implicit_deletes(ctx, store, file_adds, parent_manifest_ids)
            .collect()
            .compat()
            .await?;
    let maybe_renamed_implicit_deletes: Result<Vec<Option<MPath>>, _> =
        implicit_deletes.iter().map(|mpath| mover(mpath)).collect();
    let maybe_renamed_implicit_deletes: Vec<Option<MPath>> = maybe_renamed_implicit_deletes?;
    let implicit_delete_file_changes: Vec<_> = maybe_renamed_implicit_deletes
        .into_iter()
        .filter_map(|maybe_implicit_delete| maybe_implicit_delete)
        .map(|implicit_delete_mpath| (implicit_delete_mpath, None))
        .collect();

    Ok(implicit_delete_file_changes)
}

/// Create a version of `cs` with `Mover` applied to all changes
/// The return value can be:
/// - `Err` if the rewrite failed
/// - `Ok(None)` if the rewrite decided that this commit should
///              not be present in the rewrite target
/// - `Ok(Some(rewritten))` for a successful rewrite, which should be
///                         present in the rewrite target
/// The notion that the commit "should not be present in the rewrite
/// target" means that the commit is not a merge and all of its changes
/// were rewritten into nothingness by the `Mover`.
///
/// Precondition: this function expects all `cs` parents to be present
/// in `remapped_parents` as keys, and their remapped versions as values.
pub async fn rewrite_commit(
    ctx: CoreContext,
    mut cs: BonsaiChangesetMut,
    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
    mover: Mover,
    source_repo: BlobRepo,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    if !cs.file_changes.is_empty() {
        let implicit_delete_file_changes = get_implicit_delete_file_changes(
            ctx.clone(),
            cs.clone(),
            remapped_parents.keys().cloned(),
            mover.clone(),
            source_repo,
        )
        .await?;

        let path_rewritten_changes: Result<BTreeMap<_, _>, _> = cs
            .file_changes
            .into_iter()
            .filter_map(|(path, change)| {
                // Just rewrite copy_from information, when we have it
                fn rewrite_copy_from(
                    copy_from: &(MPath, ChangesetId),
                    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
                    mover: Mover,
                ) -> Result<Option<(MPath, ChangesetId)>, Error> {
                    let (path, copy_from_commit) = copy_from;
                    let new_path = mover(&path)?;
                    let copy_from_commit = remapped_parents.get(copy_from_commit).ok_or(
                        Error::from(ErrorKind::MissingRemappedCommit(*copy_from_commit)),
                    )?;

                    // If the source path doesn't remap, drop this copy info.
                    Ok(new_path.map(|new_path| (new_path, *copy_from_commit)))
                }

                // Extract any copy_from information, and use rewrite_copy_from on it
                fn rewrite_file_change(
                    change: FileChange,
                    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
                    mover: Mover,
                ) -> Result<FileChange, Error> {
                    let new_copy_from = change
                        .copy_from()
                        .and_then(|copy_from| {
                            rewrite_copy_from(copy_from, remapped_parents, mover).transpose()
                        })
                        .transpose()?;

                    Ok(FileChange::with_new_copy_from(change, new_copy_from))
                }

                // Rewrite both path and changes
                fn do_rewrite(
                    path: MPath,
                    change: Option<FileChange>,
                    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
                    mover: Mover,
                ) -> Result<Option<(MPath, Option<FileChange>)>, Error> {
                    let new_path = mover(&path)?;
                    let change = change
                        .map(|change| rewrite_file_change(change, remapped_parents, mover.clone()))
                        .transpose()?;
                    Ok(new_path.map(|new_path| (new_path, change)))
                }
                do_rewrite(path, change, &remapped_parents, mover.clone()).transpose()
            })
            .collect();

        let mut path_rewritten_changes = path_rewritten_changes?;
        path_rewritten_changes.extend(implicit_delete_file_changes.into_iter());
        let path_rewritten_changes = minimize_file_change_set(path_rewritten_changes.into_iter());
        let is_merge = cs.parents.len() >= 2;

        // If all parent has < 2 commits then it's not a merge, and it was completely rewritten
        // out. In that case we can just discard it because there are not changes to the working copy.
        // However if it's a merge then we can't discard it, because even
        // though bonsai merge commit might not have file changes inside it can still change
        // a working copy. E.g. if p1 has fileA, p2 has fileB, then empty merge(p1, p2)
        // contains both fileA and fileB.
        if path_rewritten_changes.is_empty() && !is_merge {
            return Ok(None);
        } else {
            cs.file_changes = path_rewritten_changes;
        }
    }

    // Update hashes
    for commit in cs.parents.iter_mut() {
        let remapped = remapped_parents
            .get(commit)
            .ok_or(Error::from(ErrorKind::MissingRemappedCommit(*commit)))?;

        *commit = *remapped;
    }

    Ok(Some(cs))
}

/// Applies `Mover` to all paths in `cs`, dropping any entry whose path rewrites to `None`
/// E.g. adding a prefix can be done by a `Mover` that adds the prefix and returns `Some(path)`.
/// Removing a prefix would be like adding, but returning `None` if the path does not have the prefix
/// Additionally, changeset IDs are rewritten.
///
/// Precondition: *all* parents must already have been rewritten into the target repo. The
/// behaviour of this function is unpredictable if some parents have not yet been remapped
async fn remap_parents_and_rewrite_commit<'a, M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    cs: BonsaiChangesetMut,
    commit_syncer: &'a CommitSyncer<M>,
    source_repo: BlobRepo,
    parent_selection_hint: CandidateSelectionHint,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    let (_, _, mover) = commit_syncer.get_source_target_mover(&ctx)?;
    let remapped_parents = remap_parents(&ctx, &cs, commit_syncer, parent_selection_hint).await?;
    rewrite_commit(ctx.clone(), cs, &remapped_parents, mover, source_repo).await
}

async fn remap_parents<'a, M: SyncedCommitMapping + Clone + 'static>(
    ctx: &CoreContext,
    cs: &BonsaiChangesetMut,
    commit_syncer: &'a CommitSyncer<M>,
    hint: CandidateSelectionHint,
) -> Result<HashMap<ChangesetId, ChangesetId>, Error> {
    let mut remapped_parents = HashMap::new();
    for commit in &cs.parents {
        let maybe_sync_outcome = commit_syncer
            .get_commit_sync_outcome_with_hint(ctx.clone(), Source(*commit), hint.clone())
            .await?;
        let sync_outcome: Result<_, Error> =
            maybe_sync_outcome.ok_or(ErrorKind::ParentNotRemapped(*commit).into());
        let sync_outcome = sync_outcome?;

        use CommitSyncOutcome::*;
        let remapped_parent = match sync_outcome {
            RewrittenAs(cs_id, _) | EquivalentWorkingCopyAncestor(cs_id, _) => cs_id,
            Preserved => *commit,
            NotSyncCandidate => {
                return Err(ErrorKind::ParentNotSyncCandidate(*commit).into());
            }
        };

        remapped_parents.insert(*commit, remapped_parent);
    }

    Ok(remapped_parents)
}

pub async fn find_toposorted_unsynced_ancestors<M>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<M>,
    start_cs_id: ChangesetId,
) -> Result<Vec<ChangesetId>, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let source_repo = commit_syncer.get_source_repo().clone();

    let mut visited = hashset! { start_cs_id };
    let mut q = VecDeque::new();
    q.push_back(start_cs_id);

    let mut commits_to_backsync = HashMap::new();

    let mut traversed_num = 0;
    while let Some(cs_id) = q.pop_front() {
        traversed_num += 1;
        if traversed_num % 100 == 0 {
            info!(
                ctx.logger(),
                "traversed {} commits while backsyncing, starting from {}",
                traversed_num,
                start_cs_id
            );
        }

        if commit_syncer
            .commit_sync_outcome_exists(ctx.clone(), Source(cs_id))
            .await?
        {
            continue;
        } else {
            let parents = source_repo
                .get_changeset_parents_by_bonsai(ctx.clone(), cs_id)
                .compat()
                .await?;

            commits_to_backsync.insert(cs_id, parents.clone());

            q.extend(parents.into_iter().filter(|p| visited.insert(*p)));
        }
    }

    // sort_topological returns a list which contains both commits_to_backsync keys and
    // values (i.e. parents). We need only keys, so below we added a filter to remove parents
    //
    // TODO(stash): T60147215 change sort_topological logic to not return parents!
    let res = sort_topological(&commits_to_backsync).expect("unexpected cycle in commit graph!");

    Ok(res
        .into_iter()
        .filter(|r| commits_to_backsync.contains_key(r))
        .collect())
}

#[derive(Clone)]
pub enum CommitSyncRepos {
    LargeToSmall {
        large_repo: BlobRepo,
        small_repo: BlobRepo,
    },
    SmallToLarge {
        small_repo: BlobRepo,
        large_repo: BlobRepo,
    },
}

impl CommitSyncRepos {
    /// Create a new instance of `CommitSyncRepos`
    /// The direction is determined by the `source_repo` and `target_repo`
    /// arguments, while `SmallToLarge` vs `LargeToSmall` - by the
    /// `CommitSyncConfig` field of the `source_or_target_repo_config` argument.
    /// The name `source_or_target_repo_config` is meant to signify that it
    /// makes no difference which one to pass.
    pub fn new(
        source_repo: BlobRepo,
        target_repo: BlobRepo,
        commit_sync_config: &CommitSyncConfig,
    ) -> Result<Self, Error> {
        let small_repo_id = if commit_sync_config.large_repo_id == source_repo.get_repoid()
            && commit_sync_config
                .small_repos
                .contains_key(&target_repo.get_repoid())
        {
            target_repo.get_repoid()
        } else if commit_sync_config.large_repo_id == target_repo.get_repoid()
            && commit_sync_config
                .small_repos
                .contains_key(&source_repo.get_repoid())
        {
            source_repo.get_repoid()
        } else {
            return Err(format_err!(
                "CommitSyncMapping incompatible with source repo {:?} and target repo {:?}",
                source_repo.get_repoid(),
                target_repo.get_repoid()
            ));
        };

        if source_repo.get_repoid() == small_repo_id {
            Ok(CommitSyncRepos::SmallToLarge {
                large_repo: target_repo.clone(),
                small_repo: source_repo.clone(),
            })
        } else {
            Ok(CommitSyncRepos::LargeToSmall {
                large_repo: source_repo.clone(),
                small_repo: target_repo.clone(),
            })
        }
    }
}

#[derive(Clone)]
pub struct CommitSyncer<M> {
    // TODO: Finish refactor and remove pub
    pub mapping: M,
    pub repos: CommitSyncRepos,
    pub commit_sync_data_provider: CommitSyncDataProvider,
}

impl<M> fmt::Debug for CommitSyncer<M>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let source_repo_id = self.get_source_repo_id();
        let target_repo_id = self.get_target_repo_id();
        write!(f, "CommitSyncer{{{}->{}}}", source_repo_id, target_repo_id)
    }
}

impl<M> CommitSyncer<M>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    pub fn new(
        mapping: M,
        repos: CommitSyncRepos,
        live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
    ) -> Self {
        let commit_sync_data_provider = CommitSyncDataProvider::Live(live_commit_sync_config);
        Self {
            mapping,
            repos,
            commit_sync_data_provider,
        }
    }

    pub fn get_source_repo(&self) -> &BlobRepo {
        self.repos.get_source_repo()
    }

    pub fn get_source_repo_id(&self) -> RepositoryId {
        self.get_source_repo().get_repoid()
    }

    pub fn get_target_repo(&self) -> &BlobRepo {
        self.repos.get_target_repo()
    }

    pub fn get_target_repo_id(&self) -> RepositoryId {
        self.get_target_repo().get_repoid()
    }

    pub fn get_large_repo(&self) -> &BlobRepo {
        use CommitSyncRepos::*;
        match self.repos {
            LargeToSmall { ref large_repo, .. } => large_repo,
            SmallToLarge { ref large_repo, .. } => large_repo,
        }
    }

    pub fn get_small_repo(&self) -> &BlobRepo {
        use CommitSyncRepos::*;
        match self.repos {
            LargeToSmall { ref small_repo, .. } => small_repo,
            SmallToLarge { ref small_repo, .. } => small_repo,
        }
    }

    pub fn get_mapping(&self) -> &M {
        &self.mapping
    }

    pub fn get_mover_by_version(&self, version: &CommitSyncConfigVersion) -> Result<Mover, Error> {
        let (source_repo, target_repo) = self.get_source_target();
        self.commit_sync_data_provider.get_mover(
            version,
            source_repo.get_repoid(),
            target_repo.get_repoid(),
        )
    }

    pub fn get_reverse_mover_by_version(
        &self,
        version: &CommitSyncConfigVersion,
    ) -> Result<Mover, Error> {
        let (source_repo, target_repo) = self.get_source_target();
        self.commit_sync_data_provider.get_reverse_mover(
            version,
            source_repo.get_repoid(),
            target_repo.get_repoid(),
        )
    }

    // TODO(stash): remove callers of this function
    #[allow(non_snake_case)]
    pub fn get_current_mover_DEPRECATED(&self, ctx: &CoreContext) -> Result<Mover, Error> {
        let (_, _, mover) = self.get_source_target_mover(ctx)?;
        Ok(mover)
    }

    // TODO(stash): remove callers of this function
    #[allow(non_snake_case)]
    pub fn get_current_reverse_mover_DEPRECATED(&self, ctx: &CoreContext) -> Result<Mover, Error> {
        let (source_repo, target_repo, version_name) = self.get_source_target_version(ctx)?;

        self.commit_sync_data_provider.get_reverse_mover(
            &version_name,
            source_repo.get_repoid(),
            target_repo.get_repoid(),
        )
    }

    pub fn get_bookmark_renamer(&self, ctx: &CoreContext) -> Result<BookmarkRenamer, Error> {
        let (source_repo, target_repo, version_name) = self.get_source_target_version(ctx)?;

        self.commit_sync_data_provider.get_bookmark_renamer(
            &version_name,
            source_repo.get_repoid(),
            target_repo.get_repoid(),
        )
    }

    pub fn get_reverse_bookmark_renamer(
        &self,
        ctx: &CoreContext,
    ) -> Result<BookmarkRenamer, Error> {
        let (source_repo, target_repo, version_name) = self.get_source_target_version(ctx)?;

        self.commit_sync_data_provider.get_reverse_bookmark_renamer(
            &version_name,
            source_repo.get_repoid(),
            target_repo.get_repoid(),
        )
    }

    pub fn get_current_version(&self, ctx: &CoreContext) -> Result<CommitSyncConfigVersion, Error> {
        let (_, _, version_name) = self.get_source_target_version(ctx)?;

        Ok(version_name)
    }

    pub fn rename_bookmark(
        &self,
        ctx: &CoreContext,
        bookmark: &BookmarkName,
    ) -> Result<Option<BookmarkName>, Error> {
        Ok(self.get_bookmark_renamer(ctx)?(bookmark))
    }

    pub async fn get_commit_sync_outcome(
        &self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
    ) -> Result<Option<CommitSyncOutcome>, Error> {
        get_commit_sync_outcome(
            &ctx,
            Source(self.repos.get_source_repo().get_repoid()),
            Target(self.repos.get_target_repo().get_repoid()),
            Source(source_cs_id),
            &self.mapping,
        )
        .await
    }

    pub async fn commit_sync_outcome_exists(
        &self,
        ctx: CoreContext,
        source_cs_id: Source<ChangesetId>,
    ) -> Result<bool, Error> {
        commit_sync_outcome_exists(
            &ctx,
            Source(self.repos.get_source_repo().get_repoid()),
            Target(self.repos.get_target_repo().get_repoid()),
            source_cs_id,
            &self.mapping,
        )
        .await
    }

    pub async fn get_commit_sync_outcome_with_hint(
        &self,
        ctx: CoreContext,
        source_cs_id: Source<ChangesetId>,
        hint: CandidateSelectionHint,
    ) -> Result<Option<CommitSyncOutcome>, Error> {
        get_commit_sync_outcome_with_hint(
            &ctx,
            Source(self.repos.get_source_repo().get_repoid()),
            Target(self.repos.get_target_repo().get_repoid()),
            source_cs_id,
            &self.mapping,
            hint,
        )
        .await
    }

    // This is the function that safely syncs a commit and all of its unsynced ancestors from a
    // source repo to target repo. If commit is already synced then it just does a lookup.
    // But safety comes with flexibility cost - not all of the syncs are allowed. For example,
    // syncing a *public* commit from a small repo to a large repo is not allowed:
    // 1) If small repo is the source of truth, then there should be only a single job that
    //    does this sync. Since this function can be used from many places and we have no
    //    way of ensuring only a single job does the sync, this sync is forbidden completely.
    // 2) If large repo is a source of truth, then there should never be a case with public
    //    commit in a small repo not having an equivalent in the large repo.
    pub async fn sync_commit(
        &self,
        ctx: &CoreContext,
        source_cs_id: ChangesetId,
        ancestor_selection_hint: CandidateSelectionHint,
    ) -> Result<Option<ChangesetId>, Error> {
        let unsynced_ancestors =
            find_toposorted_unsynced_ancestors(&ctx, self, source_cs_id).await?;

        let source_repo = self.repos.get_source_repo();

        let small_repo = self.get_small_repo();
        let source_repo_is_small = source_repo.get_repoid() == small_repo.get_repoid();

        if source_repo_is_small {
            let public_unsynced_ancestors = source_repo
                .get_phases()
                .get_public(
                    ctx.clone(),
                    unsynced_ancestors.clone(),
                    false, /* ephemeral_derive */
                )
                .compat()
                .await?;
            if !public_unsynced_ancestors.is_empty() {
                return Err(format_err!(
                    "unexpected sync lookup attempt - trying to sync \
                     a public commit from small repo to a large repo. Syncing public commits is \
                     only supported from a large repo to a small repo"
                ));
            }
        }

        for ancestor in unsynced_ancestors {
            self.unsafe_sync_commit(ctx.clone(), ancestor, ancestor_selection_hint.clone())
                .await?;
        }

        let commit_sync_outcome = self
            .get_commit_sync_outcome(ctx.clone(), source_cs_id)
            .await?
            .ok_or(format_err!(
                "was not able to remap a commit {}",
                source_cs_id
            ))?;
        use CommitSyncOutcome::*;
        let res = match commit_sync_outcome {
            NotSyncCandidate => None,
            RewrittenAs(cs_id, _) | EquivalentWorkingCopyAncestor(cs_id, _) => Some(cs_id),
            Preserved => Some(source_cs_id),
        };
        Ok(res)
    }

    /// Create a changeset, equivalent to `source_cs_id` in the target repo
    /// The difference between this function and `rewrite_commit` is that
    /// `rewrite_commit` does not know anything about the repo and only produces
    /// a `BonsaiChangesetMut` object, which later may or may not be uploaded
    /// into the repository.
    /// This function is prefixed with unsafe because it requires that ancestors commits are
    /// already synced and because syncing commit public commits from a small repo to a large repo
    /// using this function might lead to repo corruption.
    /// `parent_selection_hint` is used when remapping this commit's parents.
    /// See `CandidateSelectionHint` doctring for more details.
    pub async fn unsafe_sync_commit(
        &self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
        parent_mapping_selection_hint: CandidateSelectionHint,
    ) -> Result<Option<ChangesetId>, Error> {
        self.unsafe_sync_commit_impl(ctx, source_cs_id, parent_mapping_selection_hint, None)
            .await
    }

    /// Same as `unsafe_commit_sync`, but with a version override
    /// Normally commit sync logic would chose the `CommitSyncConfigVersion` to use
    /// from the parent commits. In tests it's convenient to just force the use
    /// of a different version instead of a lengthy crafting of "version boundary"
    /// commits.
    /// It is a dangerous override and should be used from tests.
    pub async fn test_unsafe_sync_commit_with_version_override(
        &self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
        parent_mapping_selection_hint: CandidateSelectionHint,
        sync_config_version_override: Option<CommitSyncConfigVersion>,
    ) -> Result<Option<ChangesetId>, Error> {
        self.unsafe_sync_commit_impl(
            ctx,
            source_cs_id,
            parent_mapping_selection_hint,
            sync_config_version_override,
        )
        .await
    }

    async fn unsafe_sync_commit_impl(
        &self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
        parent_mapping_selection_hint: CandidateSelectionHint,
        sync_config_version_override: Option<CommitSyncConfigVersion>,
    ) -> Result<Option<ChangesetId>, Error> {
        // Take most of below function unsafe_sync_commit into here and delete. Leave pushrebase in next fn
        let (source_repo, _) = self.get_source_target();

        debug!(
            ctx.logger(),
            "{:?}: unsafe_sync_commit called for {}, with hint: {:?}",
            self,
            source_cs_id,
            parent_mapping_selection_hint
        );

        let cs = source_cs_id
            .load(ctx.clone(), source_repo.blobstore())
            .await?;
        let parents: Vec<_> = cs.parents().collect();

        if parents.is_empty() {
            self.sync_commit_no_parents(ctx.clone(), cs, sync_config_version_override)
                .await
        } else if parents.len() == 1 {
            self.sync_commit_single_parent(
                ctx.clone(),
                cs,
                parent_mapping_selection_hint,
                sync_config_version_override,
            )
            .await
        } else {
            self.sync_merge(ctx.clone(), cs, sync_config_version_override)
                .await
        }
    }

    /// Rewrite a commit and creates in target repo if parents are already created.
    /// This is marked as unsafe since it might lead to repo corruption if used incorrectly.
    /// It can be used to import a merge commit from a new repo:
    ///
    ///```text
    ///     source repo:
    ///
    ///     O  <- master (common bookmark). Points to a merge commit that imports a new repo
    ///     | \
    ///     O   \
    ///          O  <- merge commit in the new repo we are trying to merge into master.
    ///         /  \   naive_sync_commit can be used to sync this commit
    ///```
    ///
    /// Normally this function is able to find the parents for the synced commit automatically
    /// but in case it can't then `maybe_parents` parameter allows us to overwrite parents of
    /// the synced commit.
    pub async fn unsafe_always_rewrite_sync_commit(
        &self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
        maybe_parents: Option<HashMap<ChangesetId, ChangesetId>>,
    ) -> Result<Option<ChangesetId>, Error> {
        let (source_repo, target_repo, mover) = self.get_source_target_mover(&ctx)?;
        let source_cs = source_cs_id
            .load(ctx.clone(), source_repo.blobstore())
            .await?;

        let source_cs = source_cs.clone().into_mut();
        let remapped_parents = match maybe_parents {
            Some(parents) => parents,
            None => remap_parents(&ctx, &source_cs, self, CandidateSelectionHint::Only).await?, // TODO: check if only is ok
        };

        let rewritten_commit = rewrite_commit(
            ctx.clone(),
            source_cs,
            &remapped_parents,
            mover,
            source_repo.clone(),
        )
        .await?;
        match rewritten_commit {
            None => {
                self.update_wc_equivalence(ctx.clone(), source_cs_id, None)
                    .await?;
                Ok(None)
            }
            Some(rewritten) => {
                // Sync commit
                let frozen = rewritten.freeze()?;
                let frozen_cs_id = frozen.get_changeset_id();
                upload_commits(ctx.clone(), vec![frozen], source_repo, target_repo.clone()).await?;

                update_mapping(
                    ctx.clone(),
                    hashmap! { source_cs_id => frozen_cs_id },
                    &self,
                )
                .await?;
                Ok(Some(frozen_cs_id))
            }
        }
    }

    /// This function is prefixed with unsafe because it requires that ancestors commits are
    /// already synced and because there should be exactly one sync job that uses this function
    /// for a (small repo -> large repo) pair.
    pub async fn unsafe_sync_commit_pushrebase(
        &self,
        ctx: CoreContext,
        source_cs: BonsaiChangeset,
        bookmark: BookmarkName,
        target_lca_hint: Target<Arc<dyn LeastCommonAncestorsHint>>,
    ) -> Result<Option<ChangesetId>, Error> {
        let hash = source_cs.get_changeset_id();
        let (source_repo, target_repo) = self.get_source_target();
        // TODO(stash): use the actual version that was used to remap commits
        let version_name = self.get_current_version(&ctx)?;

        let parent_selection_hint = CandidateSelectionHint::OnlyOrAncestorOfBookmark(
            Target(bookmark.clone()),
            Target(self.get_target_repo().clone()),
            target_lca_hint,
        );
        match remap_parents_and_rewrite_commit(
            ctx.clone(),
            source_cs.clone().into_mut(),
            self,
            source_repo.clone(),
            parent_selection_hint,
        )
        .await?
        {
            None => {
                let mut remapped_parents_outcome = vec![];
                for p in source_cs.parents() {
                    let maybe_commit_sync_outcome = self
                        .get_commit_sync_outcome(ctx.clone(), p)
                        .await?
                        .map(|sync_outcome| (sync_outcome, p));
                    remapped_parents_outcome.extend(maybe_commit_sync_outcome.into_iter());
                }

                if remapped_parents_outcome.is_empty() {
                    self.update_wc_equivalence(ctx.clone(), hash, None).await?;
                } else if remapped_parents_outcome.len() == 1 {
                    use CommitSyncOutcome::*;
                    let (sync_outcome, parent) = &remapped_parents_outcome[0];
                    let wc_equivalence = match sync_outcome {
                        NotSyncCandidate => None,
                        RewrittenAs(cs_id, _) | EquivalentWorkingCopyAncestor(cs_id, _) => {
                            Some(*cs_id)
                        }
                        Preserved => Some(*parent),
                    };

                    self.update_wc_equivalence(ctx.clone(), hash, wc_equivalence)
                        .await?;
                } else {
                    return Err(ErrorKind::AmbiguousWorkingCopyEquivalent(
                        source_cs.get_changeset_id(),
                    )
                    .into());
                }

                Ok(None)
            }
            Some(rewritten) => {
                // Sync commit
                let frozen = rewritten.freeze()?;
                let rewritten_list = hashset![frozen];
                upload_commits(
                    ctx.clone(),
                    rewritten_list.clone().into_iter().collect(),
                    source_repo.clone(),
                    target_repo.clone(),
                )
                .await?;

                let pushrebase_flags = {
                    let mut flags = PushrebaseFlags::default();
                    flags.rewritedates = false;
                    flags.forbid_p2_root_rebases = false;
                    flags.casefolding_check = false;
                    flags.recursion_limit = None;
                    flags
                };

                let pushrebase_res = do_pushrebase_bonsai(
                    &ctx,
                    &target_repo,
                    &pushrebase_flags,
                    &bookmark,
                    &rewritten_list,
                    None,
                    &[CrossRepoSyncPushrebaseHook::new(
                        hash,
                        self.repos.clone(),
                        version_name,
                    )],
                )
                .await;
                let pushrebase_res =
                    pushrebase_res.map_err(|e| Error::from(ErrorKind::PushrebaseFailure(e)))?;
                let pushrebased_changeset = pushrebase_res.head;
                Ok(Some(pushrebased_changeset))
            }
        }
    }

    /// The difference between `unsafe_sync_commit()` and `unsafe_preserve_commit()` is that `unsafe_preserve_commit()`
    /// doesn't do any commit rewriting, and it requires all it's parents to be preserved.
    /// It's prefixed with "unsafe_" because it doesn't apply any path rewriting.
    pub async fn unsafe_preserve_commit(
        &self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
    ) -> Result<(), Error> {
        let (source_repo, target_repo) = self.get_source_target();
        let cs = source_cs_id
            .load(ctx.clone(), source_repo.blobstore())
            .await?;

        for p in cs.parents() {
            let maybe_outcome = self.get_commit_sync_outcome(ctx.clone(), p).await?;
            let sync_outcome =
                maybe_outcome.ok_or(format_err!("Parent commit {} is not synced yet", p))?;

            if sync_outcome != CommitSyncOutcome::Preserved {
                bail!(
                    "trying to preserve a commit, but parent {} is not preserved",
                    p
                );
            }
        }

        upload_commits(
            ctx.clone(),
            vec![cs],
            source_repo.clone(),
            target_repo.clone(),
        )
        .await?;

        // update_mapping also updates working copy equivalence, so no need
        // to do it separately
        update_mapping_no_version_deprecated(
            ctx.clone(),
            hashmap! { source_cs_id => source_cs_id },
            &self,
        )
        .await
    }

    async fn sync_commit_no_parents(
        &self,
        ctx: CoreContext,
        cs: BonsaiChangeset,
        sync_config_version_override: Option<CommitSyncConfigVersion>,
    ) -> Result<Option<ChangesetId>, Error> {
        let source_cs_id = cs.get_changeset_id();
        let (source_repo, target_repo, mover, version) = match sync_config_version_override {
            None => {
                let version = self.get_current_version(&ctx)?;
                let (sr, tr, mv) = self.get_source_target_mover(&ctx)?;
                (sr, tr, mv, version)
            }
            Some(version_override) => {
                let (source_repo, target_repo) = self.get_source_target();
                let mover = self.get_mover_by_version(&version_override)?;
                (source_repo, target_repo, mover, version_override)
            }
        };

        match rewrite_commit(
            ctx.clone(),
            cs.into_mut(),
            &HashMap::new(),
            mover,
            source_repo.clone(),
        )
        .await?
        {
            Some(rewritten) => {
                let frozen = rewritten.freeze()?;
                upload_commits(
                    ctx.clone(),
                    vec![frozen.clone()],
                    source_repo.clone(),
                    target_repo.clone(),
                )
                .await?;

                // update_mapping also updates working copy equivalence, so no need
                // to do it separately
                update_mapping_with_version(
                    ctx.clone(),
                    hashmap! { source_cs_id => frozen.get_changeset_id() },
                    &self,
                    &version,
                )
                .await?;
                Ok(Some(frozen.get_changeset_id()))
            }
            None => {
                self.update_wc_equivalence_with_version(ctx.clone(), source_cs_id, None, version)
                    .await?;
                Ok(None)
            }
        }
    }

    async fn sync_commit_single_parent(
        &self,
        ctx: CoreContext,
        cs: BonsaiChangeset,
        parent_mapping_selection_hint: CandidateSelectionHint,
        sync_config_version_override: Option<CommitSyncConfigVersion>,
    ) -> Result<Option<ChangesetId>, Error> {
        let source_cs_id = cs.get_changeset_id();
        let cs = cs.into_mut();
        let p = cs.parents[0];
        let (source_repo, target_repo) = self.get_source_target();

        let maybe_parent_sync_outcome = self
            .get_commit_sync_outcome_with_hint(
                ctx.clone(),
                Source(p),
                parent_mapping_selection_hint,
            )
            .await?;

        let parent_sync_outcome = maybe_parent_sync_outcome
            .ok_or(format_err!("Parent commit {} is not synced yet", p))?;

        use CommitSyncOutcome::*;
        match parent_sync_outcome {
            NotSyncCandidate => {
                // If there's not working copy for parent commit then there's no working
                // copy for child either.
                self.update_wc_equivalence(ctx.clone(), source_cs_id, None)
                    .await?;
                Ok(None)
            }
            RewrittenAs(remapped_p, maybe_version)
            | EquivalentWorkingCopyAncestor(remapped_p, maybe_version) => {
                let version = match (sync_config_version_override, maybe_version) {
                    (Some(version_override), _) => version_override,
                    (None, Some(version)) => version,
                    (None, None) => {
                        // TODO(stash): this case should go away:
                        // RewrittenAs and EquivalentWorkingCopyAncestor should not store
                        // optional mapping
                        self.get_current_version(&ctx)?
                    }
                };

                let rewrite_paths = self.get_mover_by_version(&version)?;

                let mut remapped_parents = HashMap::new();
                remapped_parents.insert(p, remapped_p);
                let maybe_rewritten = rewrite_commit(
                    ctx.clone(),
                    cs,
                    &remapped_parents,
                    rewrite_paths,
                    source_repo.clone(),
                )
                .await?;
                match maybe_rewritten {
                    Some(rewritten) => {
                        let frozen = rewritten.freeze()?;
                        upload_commits(
                            ctx.clone(),
                            vec![frozen.clone()],
                            source_repo.clone(),
                            target_repo.clone(),
                        )
                        .await?;

                        // update_mapping also updates working copy equivalence, so no need
                        // to do it separately
                        update_mapping_with_version(
                            ctx.clone(),
                            hashmap! { source_cs_id => frozen.get_changeset_id() },
                            &self,
                            &version,
                        )
                        .await?;
                        Ok(Some(frozen.get_changeset_id()))
                    }
                    None => {
                        // Source commit doesn't rewrite to any target commits.
                        // In that case equivalent working copy is the equivalent working
                        // copy of the parent
                        self.update_wc_equivalence_with_version(
                            ctx.clone(),
                            source_cs_id,
                            Some(remapped_p),
                            version,
                        )
                        .await?;
                        Ok(None)
                    }
                }
            }
            Preserved => {
                let frozen = cs.freeze()?;
                upload_commits(
                    ctx.clone(),
                    vec![frozen],
                    source_repo.clone(),
                    target_repo.clone(),
                )
                .await?;

                // update_mapping also updates working copy equivalence, so no need
                // to do it separately
                update_mapping_no_version_deprecated(
                    ctx.clone(),
                    hashmap! { source_cs_id => source_cs_id },
                    &self,
                )
                .await?;
                Ok(Some(source_cs_id))
            }
        }
    }


    /// Get `CommitSyncConfigVersion` to use while remapping a
    /// merge commit (`source_cs_id`)
    /// The idea is to derive this version from the `parent_outcomes`
    /// according to the following rules:
    /// - all `NotSyncCandidate` parents are ignored
    /// - `Preserved` parents are prohibited (sync fails), and it
    ///   is expected that such cases are handled by the caller in a
    ///   separate flow
    /// - all `RewrittenAs` and `EquivalentWorkingCopyAncestor`
    ///   parents have the same (non-None) version associated
    async fn get_mover_to_use_for_merge<'a>(
        &'a self,
        ctx: &CoreContext,
        source_cs_id: ChangesetId,
        parent_outcomes: impl IntoIterator<Item = &'a CommitSyncOutcome>,
    ) -> Result<(Mover, CommitSyncConfigVersion), Error> {
        let version = get_version_for_merge(
            source_cs_id,
            parent_outcomes,
            self.get_current_version(ctx)?,
        )?;

        let mover = self.get_mover_by_version(&version)?;
        Ok((mover, version))
    }

    // See more details about the algorithm in https://fb.quip.com/s8fYAOxEohtJ
    // A few important notes:
    // 1) Merges are synced only in LARGE -> SMALL direction.
    // 2) If a large repo merge has any parent after big merge, then this merge will appear
    //    in all small repos
    async fn sync_merge(
        &self,
        ctx: CoreContext,
        cs: BonsaiChangeset,
        _sync_config_version_override: Option<CommitSyncConfigVersion>,
    ) -> Result<Option<ChangesetId>, Error> {
        if let CommitSyncRepos::SmallToLarge { .. } = self.repos {
            bail!("syncing merge commits is supported only in large to small direction");
        }

        let source_cs_id = cs.get_changeset_id();
        let cs = cs.into_mut();

        let parent_outcomes = stream::iter(cs.parents.clone().into_iter().map(|p| {
            self.get_commit_sync_outcome(ctx.clone(), p)
                .and_then(move |maybe_outcome| {
                    match maybe_outcome {
                        Some(outcome) => future::ok((p, outcome)),
                        None => future::err(format_err!("{} does not have CommitSyncOutcome", p)),
                    }
                })
        }));

        let sync_outcomes = parent_outcomes
            .buffered(100)
            .try_collect::<Vec<_>>()
            .await?;

        if sync_outcomes
            .iter()
            .all(|(_, outcome)| outcome == &CommitSyncOutcome::Preserved)
        {
            // All parents being `Preserved` means that merge happens
            // purely in the pre-big-merge area of the repo, so it can
            // just be safely preserved.
            self.unsafe_preserve_commit(ctx.clone(), source_cs_id)
                .await?;
            return Ok(Some(source_cs_id));
        }

        // We can have both NotSyncCandidate and Preserved, see example below
        //
        // "X" - NotSyncCandidate
        // "P", "R" - already synced commits (preserved or rewritten)
        // "A", "B" - new commits to sync
        //
        //   R
        //   |
        //   BM  <- Big merge
        //  / \
        // P  X   B <- Merge commit, has NotSyncCandidate and Preserved
        //    | / |
        //    X   A <- this commit can be preserved (e.g. if it touches shared directory)
        //
        //
        // In the case like that let's mark a commit as NotSyncCandidate
        if sync_outcomes.iter().all(|(_, outcome)| {
            outcome == &CommitSyncOutcome::Preserved
                || outcome == &CommitSyncOutcome::NotSyncCandidate
        }) {
            self.update_wc_equivalence(ctx.clone(), source_cs_id, None)
                .await?;
            return Ok(None);
        }

        // At this point we know that there's at least one parent after big merge. However we still
        // might have a parent that's NotSyncCandidate
        //
        //   B
        //   | \
        //   |  \
        //   R   X  <- new repo was merged, however this repo was not synced at all.
        //   |   |
        //   |   ...
        //   ...
        //   BM  <- Big merge
        //  / \
        //  ...
        //
        // This parents will be completely removed. However when these parents are removed
        // we also need to be careful to strip all copy info
        let new_parents: HashMap<_, _> = sync_outcomes
            .iter()
            .filter_map(|(p, outcome)| {
                use CommitSyncOutcome::*;
                match outcome {
                    EquivalentWorkingCopyAncestor(cs_id, _) | RewrittenAs(cs_id, _) => {
                        Some((*p, *cs_id))
                    }
                    Preserved => Some((*p, *p)),
                    NotSyncCandidate => None,
                }
            })
            .collect();

        let cs = self.strip_removed_parents(cs, new_parents.keys().collect())?;

        if !new_parents.is_empty() {
            let (mover, version) = self
                .get_mover_to_use_for_merge(
                    &ctx,
                    source_cs_id,
                    sync_outcomes.iter().map(|(_, outcome)| outcome),
                )
                .await
                .context("failed getting a mover to use for merge rewriting")?;

            match rewrite_commit(
                ctx.clone(),
                cs,
                &new_parents,
                mover,
                self.get_source_repo().clone(),
            )
            .await?
            {
                Some(rewritten) => {
                    let target_cs_id = self
                        .upload_rewritten_and_update_mapping(
                            ctx.clone(),
                            source_cs_id,
                            rewritten,
                            version,
                        )
                        .await?;
                    Ok(Some(target_cs_id))
                }
                None => {
                    // We should end up in this branch only if we have a single
                    // parent, because merges are never skipped during rewriting
                    let parent_cs_id = new_parents
                        .values()
                        .next()
                        .ok_or(Error::msg("logic merge: cannot find merge parent"))?;
                    self.update_wc_equivalence(ctx.clone(), source_cs_id, Some(*parent_cs_id))
                        .await?;
                    Ok(Some(*parent_cs_id))
                }
            }
        } else {
            // All parents of the merge commit are NotSyncCandidate, mark it as NotSyncCandidate
            // as well
            self.update_wc_equivalence(ctx.clone(), source_cs_id, None)
                .await?;
            Ok(None)
        }
    }

    // Rewrites a commit and uploads it
    async fn upload_rewritten_and_update_mapping(
        &self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
        rewritten: BonsaiChangesetMut,
        version: CommitSyncConfigVersion,
    ) -> Result<ChangesetId, Error> {
        let (source_repo, target_repo, _) = self.get_source_target_mover(&ctx)?;

        let frozen = rewritten.freeze()?;
        let target_cs_id = frozen.get_changeset_id();
        upload_commits(
            ctx.clone(),
            vec![frozen],
            source_repo.clone(),
            target_repo.clone(),
        )
        .await?;

        // update_mapping also updates working copy equivalence, so no need
        // to do it separately
        update_mapping_with_version(
            ctx.clone(),
            hashmap! { source_cs_id =>  target_cs_id},
            &self,
            &version,
        )
        .await?;
        return Ok(target_cs_id);
    }

    // Some of the parents were removed - we need to remove copy-info that's not necessary
    // anymore
    fn strip_removed_parents(
        &self,
        mut source_cs: BonsaiChangesetMut,
        new_source_parents: Vec<&ChangesetId>,
    ) -> Result<BonsaiChangesetMut, Error> {
        source_cs
            .parents
            .retain(|p| new_source_parents.contains(&&*p));

        for (_, maybe_file_change) in source_cs.file_changes.iter_mut() {
            let new_file_change = if let Some(file_change) = maybe_file_change {
                match file_change.copy_from() {
                    Some((_, parent)) if !new_source_parents.contains(&parent) => {
                        Some(FileChange::new(
                            file_change.content_id(),
                            file_change.file_type(),
                            file_change.size(),
                            None,
                        ))
                    }
                    _ => Some(file_change.clone()),
                }
            } else {
                None
            };

            *maybe_file_change = new_file_change;
        }

        Ok(source_cs)
    }

    fn get_source_target_mover(
        &self,
        ctx: &CoreContext,
    ) -> Result<(BlobRepo, BlobRepo, Mover), Error> {
        let (source_repo, target_repo, version_name) = self.get_source_target_version(ctx)?;
        let mover = self.commit_sync_data_provider.get_mover(
            &version_name,
            source_repo.get_repoid(),
            target_repo.get_repoid(),
        )?;

        Ok((source_repo, target_repo, mover))
    }

    fn get_source_target_version(
        &self,
        ctx: &CoreContext,
    ) -> Result<(BlobRepo, BlobRepo, CommitSyncConfigVersion), Error> {
        let (source, target) = self.get_source_target();
        let version = self
            .commit_sync_data_provider
            .get_current_version(ctx, source.get_repoid())?;
        Ok((source, target, version))
    }

    fn get_source_target(&self) -> (BlobRepo, BlobRepo) {
        match self.repos.clone() {
            CommitSyncRepos::LargeToSmall {
                large_repo,
                small_repo,
                ..
            } => (large_repo, small_repo),
            CommitSyncRepos::SmallToLarge {
                small_repo,
                large_repo,
                ..
            } => (small_repo, large_repo),
        }
    }

    async fn update_wc_equivalence(
        &self,
        ctx: CoreContext,
        source_bcs_id: ChangesetId,
        maybe_target_bcs_id: Option<ChangesetId>,
    ) -> Result<(), Error> {
        // TODO(stash, ikostia): use the real version that was used to remap a commit
        let version_name = self.get_current_version(&ctx)?;
        self.update_wc_equivalence_with_version(
            ctx,
            source_bcs_id,
            maybe_target_bcs_id,
            version_name,
        )
        .await
    }

    async fn update_wc_equivalence_with_version(
        &self,
        ctx: CoreContext,
        source_bcs_id: ChangesetId,
        maybe_target_bcs_id: Option<ChangesetId>,
        version_name: CommitSyncConfigVersion,
    ) -> Result<(), Error> {
        let CommitSyncer { repos, mapping, .. } = self.clone();
        let (source_repo, target_repo, source_is_large) = match repos {
            CommitSyncRepos::LargeToSmall {
                large_repo,
                small_repo,
                ..
            } => (large_repo, small_repo, true),
            CommitSyncRepos::SmallToLarge {
                small_repo,
                large_repo,
                ..
            } => (small_repo, large_repo, false),
        };

        let source_repoid = source_repo.get_repoid();
        let target_repoid = target_repo.get_repoid();

        let wc_entry = match maybe_target_bcs_id {
            Some(target_bcs_id) => {
                if source_is_large {
                    EquivalentWorkingCopyEntry {
                        large_repo_id: source_repoid,
                        large_bcs_id: source_bcs_id,
                        small_repo_id: target_repoid,
                        small_bcs_id: Some(target_bcs_id),
                        version_name: Some(version_name),
                    }
                } else {
                    EquivalentWorkingCopyEntry {
                        large_repo_id: target_repoid,
                        large_bcs_id: target_bcs_id,
                        small_repo_id: source_repoid,
                        small_bcs_id: Some(source_bcs_id),
                        version_name: Some(version_name),
                    }
                }
            }
            None => {
                if !source_is_large {
                    bail!(
                        "unexpected wc equivalence update: small repo commit should always remap to large repo"
                    );
                }
                EquivalentWorkingCopyEntry {
                    large_repo_id: source_repoid,
                    large_bcs_id: source_bcs_id,
                    small_repo_id: target_repoid,
                    small_bcs_id: None,
                    version_name: Some(version_name),
                }
            }
        };

        mapping
            .insert_equivalent_working_copy(ctx.clone(), wc_entry)
            .map(|_| ())
            .compat()
            .await
    }
}

impl CommitSyncRepos {
    pub fn get_source_repo(&self) -> &BlobRepo {
        match self {
            CommitSyncRepos::LargeToSmall { large_repo, .. } => large_repo,
            CommitSyncRepos::SmallToLarge { small_repo, .. } => small_repo,
        }
    }

    pub fn get_target_repo(&self) -> &BlobRepo {
        match self {
            CommitSyncRepos::LargeToSmall { small_repo, .. } => small_repo,
            CommitSyncRepos::SmallToLarge { large_repo, .. } => large_repo,
        }
    }
}

pub async fn upload_commits(
    ctx: CoreContext,
    rewritten_list: Vec<BonsaiChangeset>,
    source_repo: BlobRepo,
    target_repo: BlobRepo,
) -> Result<(), Error> {
    let mut files_to_sync = vec![];
    for rewritten in &rewritten_list {
        let rewritten_mut = rewritten.clone().into_mut();
        let new_files_to_sync = rewritten_mut
            .file_changes
            .values()
            .filter_map(|opt_change| opt_change.as_ref().map(|change| change.content_id()));
        files_to_sync.extend(new_files_to_sync);
    }

    let source_blobstore = source_repo.get_blobstore();
    let target_blobstore = target_repo.get_blobstore();
    let target_filestore_config = target_repo.filestore_config();
    let uploader: FuturesUnordered<_> = files_to_sync
        .into_iter()
        .map({
            |content_id| {
                copy_content(
                    ctx.clone(),
                    source_blobstore.clone(),
                    target_blobstore.clone(),
                    target_filestore_config.clone(),
                    content_id,
                )
                .compat()
            }
        })
        .collect();
    uploader.try_for_each_concurrent(100, identity).await?;
    save_bonsai_changesets(rewritten_list.clone(), ctx.clone(), target_repo.clone())
        .compat()
        .await?;
    Ok(())
}

async fn update_mapping_no_version_deprecated<'a, M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    mapped: HashMap<ChangesetId, ChangesetId>,
    syncer: &'a CommitSyncer<M>,
) -> Result<(), Error> {
    // TODO(stash, ikostia) kill this fn when Preserved is fully dead
    let entries: Vec<_> = mapped
        .into_iter()
        .map(|(from, to)| create_synced_commit_mapping_entry(from, to, &syncer.repos, None))
        .collect();

    syncer
        .mapping
        .add_bulk(ctx.clone(), entries)
        .compat()
        .await?;
    Ok(())
}

// TODO(stash, ikostia) - replace all usages with update_mapping_with_version and
// remove this function
pub async fn update_mapping<'a, M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    mapped: HashMap<ChangesetId, ChangesetId>,
    syncer: &'a CommitSyncer<M>,
) -> Result<(), Error> {
    // TODO(stash): we shouldn't always use current version, but rather pass the actual version
    // that was used to remap a commit
    let version_name = syncer.get_current_version(&ctx)?;
    let entries: Vec<_> = mapped
        .into_iter()
        .map(|(from, to)| {
            cloned!(version_name);
            create_synced_commit_mapping_entry(from, to, &syncer.repos, Some(version_name))
        })
        .collect();
    syncer
        .mapping
        .add_bulk(ctx.clone(), entries)
        .compat()
        .await?;
    Ok(())
}

pub async fn update_mapping_with_version<'a, M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    mapped: HashMap<ChangesetId, ChangesetId>,
    syncer: &'a CommitSyncer<M>,
    version_name: &CommitSyncConfigVersion,
) -> Result<(), Error> {
    let entries: Vec<_> = mapped
        .into_iter()
        .map(|(from, to)| {
            create_synced_commit_mapping_entry(from, to, &syncer.repos, Some(version_name.clone()))
        })
        .collect();
    syncer
        .mapping
        .add_bulk(ctx.clone(), entries)
        .compat()
        .await?;
    Ok(())
}

pub fn create_synced_commit_mapping_entry(
    from: ChangesetId,
    to: ChangesetId,
    repos: &CommitSyncRepos,
    version_name: Option<CommitSyncConfigVersion>,
) -> SyncedCommitMappingEntry {
    let (source_repo, target_repo, source_is_large) = match repos {
        CommitSyncRepos::LargeToSmall {
            large_repo,
            small_repo,
            ..
        } => (large_repo, small_repo, true),
        CommitSyncRepos::SmallToLarge {
            small_repo,
            large_repo,
            ..
        } => (small_repo, large_repo, false),
    };

    let source_repoid = source_repo.get_repoid();
    let target_repoid = target_repo.get_repoid();

    if source_is_large {
        SyncedCommitMappingEntry::new(source_repoid, from, target_repoid, to, version_name)
    } else {
        SyncedCommitMappingEntry::new(target_repoid, to, source_repoid, from, version_name)
    }
}

pub struct Syncers<M: SyncedCommitMapping + Clone + 'static> {
    pub large_to_small: CommitSyncer<M>,
    pub small_to_large: CommitSyncer<M>,
}

pub fn create_commit_syncers<M>(
    small_repo: BlobRepo,
    large_repo: BlobRepo,
    mapping: M,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<Syncers<M>, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let small_to_large_commit_sync_repos = CommitSyncRepos::SmallToLarge {
        small_repo: small_repo.clone(),
        large_repo: large_repo.clone(),
    };

    let large_to_small_commit_sync_repos = CommitSyncRepos::LargeToSmall {
        small_repo,
        large_repo,
    };

    let commit_sync_data_provider = CommitSyncDataProvider::Live(live_commit_sync_config);
    let large_to_small_commit_syncer = CommitSyncer {
        mapping: mapping.clone(),
        repos: large_to_small_commit_sync_repos,
        commit_sync_data_provider: commit_sync_data_provider.clone(),
    };
    let small_to_large_commit_syncer = CommitSyncer {
        mapping,
        repos: small_to_large_commit_sync_repos,
        commit_sync_data_provider,
    };

    Ok(Syncers {
        large_to_small: large_to_small_commit_syncer,
        small_to_large: small_to_large_commit_syncer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use fbinit::FacebookInit;
    use maplit::btreemap;

    fn path(p: &str) -> MPath {
        MPath::new(p).unwrap()
    }

    fn verify_minimized(changes: Vec<(&str, Option<()>)>, expected: BTreeMap<&str, Option<()>>) {
        let changes: Vec<_> = changes.into_iter().map(|(p, c)| (path(p), c)).collect();
        let minimized = minimize_file_change_set(changes);
        let expected: BTreeMap<MPath, Option<()>> =
            expected.into_iter().map(|(p, c)| (path(p), c)).collect();
        assert_eq!(expected, minimized);
    }

    #[fbinit::test]
    fn test_minimize_file_change_set(_fb: FacebookInit) {
        verify_minimized(
            vec![("a", Some(())), ("a", None)],
            btreemap! { "a" => Some(())},
        );
        verify_minimized(vec![("a", Some(()))], btreemap! { "a" => Some(())});
        verify_minimized(vec![("a", None)], btreemap! { "a" => None});
        // directories are deleted implicitly, so explicit deletes are
        // minimized away
        verify_minimized(
            vec![("a/b", None), ("a/c", None), ("a", Some(()))],
            btreemap! { "a" => Some(()) },
        );
        // files, replaced with a directy at a longer path are not
        // deleted implicitly, so they aren't minimized away
        verify_minimized(
            vec![("a", None), ("a/b", Some(()))],
            btreemap! { "a" => None, "a/b" => Some(()) },
        );
    }
}
