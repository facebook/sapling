/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

#![feature(option_flattening)]

use std::collections::{BTreeMap, HashMap};

use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobsync::copy_content;
use bookmark_renaming::{get_large_to_small_renamer, get_small_to_large_renamer, BookmarkRenamer};
use bookmarks::BookmarkName;
use context::CoreContext;
use failure::{err_msg, Error, Fail};
use futures::Future;
use futures_preview::{
    compat::Future01CompatExt,
    future::{FutureExt, TryFutureExt},
    stream::{futures_unordered::FuturesUnordered, TryStreamExt},
};
use maplit::hashmap;
use metaconfig_types::{CommitSyncConfig, PushrebaseParams};
use mononoke_types::{
    BonsaiChangeset, BonsaiChangesetMut, ChangesetId, FileChange, MPath, RepositoryId,
};
use movers::{get_large_to_small_mover, get_small_to_large_mover, Mover};
use pushrebase::{do_pushrebase_bonsai, OntoBookmarkParams, PushrebaseError};
use std::fmt;
use synced_commit_mapping::{
    EquivalentWorkingCopyEntry, SyncedCommitMapping, SyncedCommitMappingEntry,
    WorkingCopyEquivalence,
};

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(
        display = "Pushrebase of synced commit failed - check config for overlaps: {:?}",
        _0
    )]
    PushrebaseFailure(PushrebaseError),
    #[fail(
        display = "Remapped commit {} expected in target repo, but not present",
        _0
    )]
    MissingRemappedCommit(ChangesetId),
    #[fail(
        display = "Could not find a commit in the target repo with the same working copy as {}",
        _0
    )]
    SameWcSearchFail(ChangesetId),
    #[fail(display = "Parent commit {} hasn't been remapped", _0)]
    ParentNotRemapped(ChangesetId),
    #[fail(display = "Parent commit {} is not a sync candidate", _0)]
    ParentNotSyncCandidate(ChangesetId),
    #[fail(display = "Cannot choose working copy equivalent for {}", _0)]
    AmbiguousWorkingCopyEquivalent(ChangesetId),
}

async fn identity<T>(res: T) -> Result<T, Error> {
    Ok(res)
}

pub fn rewrite_commit(
    mut cs: BonsaiChangesetMut,
    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
    rewrite_path: Mover,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    if !cs.file_changes.is_empty() {
        let path_rewritten_changes: Result<BTreeMap<_, _>, _> = cs
            .file_changes
            .into_iter()
            .filter_map(|(path, change)| {
                // Just rewrite copy_from information, when we have it
                fn rewrite_copy_from(
                    copy_from: &(MPath, ChangesetId),
                    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
                    rewrite_path: Mover,
                ) -> Result<Option<(MPath, ChangesetId)>, Error> {
                    let (path, copy_from_commit) = copy_from;
                    let new_path = rewrite_path(&path)?;
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
                    rewrite_path: Mover,
                ) -> Result<FileChange, Error> {
                    let new_copy_from = change
                        .copy_from()
                        .and_then(|copy_from| {
                            rewrite_copy_from(copy_from, remapped_parents, rewrite_path).transpose()
                        })
                        .transpose()?;

                    Ok(FileChange::with_new_copy_from(change, new_copy_from))
                }

                // Rewrite both path and changes
                fn do_rewrite(
                    path: MPath,
                    change: Option<FileChange>,
                    remapped_parents: &HashMap<ChangesetId, ChangesetId>,
                    rewrite_path: Mover,
                ) -> Result<Option<(MPath, Option<FileChange>)>, Error> {
                    let new_path = rewrite_path(&path)?;
                    let change = change
                        .map(|change| {
                            rewrite_file_change(change, remapped_parents, rewrite_path.clone())
                        })
                        .transpose()?;
                    Ok(new_path.map(|new_path| (new_path, change)))
                }
                do_rewrite(path, change, &remapped_parents, rewrite_path.clone()).transpose()
            })
            .collect();
        let path_rewritten_changes = path_rewritten_changes?;
        if !path_rewritten_changes.is_empty() {
            cs.file_changes = path_rewritten_changes;
        } else {
            return Ok(None);
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

async fn remap_changeset_id<'a, M: SyncedCommitMapping>(
    ctx: CoreContext,
    cs: ChangesetId,
    source_repo: &'a BlobRepo,
    target_repo: &'a BlobRepo,
    mapping: &'a M,
) -> Result<Option<ChangesetId>, Error> {
    mapping
        .get(
            ctx.clone(),
            source_repo.get_repoid(),
            cs,
            target_repo.get_repoid(),
        )
        .compat()
        .await
}

/// Applies `rewrite_path` to all paths in `cs`, dropping any entry whose path rewrites to `None`
/// E.g. adding a prefix can be done by a `rewrite` that adds the prefix and returns `Some(path)`.
/// Removing a prefix would be like adding, but returning `None` if the path does not have the prefix
/// Additionally, changeset IDs are rewritten.
///
/// Precondition: *all* parents must already have been rewritten into the target repo. The
/// behaviour of this function is unpredictable if some parents have not yet been remapped
async fn remap_parents_and_rewrite_commit<'a, M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    cs: BonsaiChangesetMut,
    commit_syncer: &'a CommitSyncer<M>,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    let (_, _, rewrite_path) = commit_syncer.get_source_target_mover();
    let mut remapped_parents = HashMap::new();
    for commit in &cs.parents {
        let maybe_sync_outcome = commit_syncer
            .get_commit_sync_outcome(ctx.clone(), *commit)
            .await?;
        let sync_outcome: Result<_, Error> =
            maybe_sync_outcome.ok_or(ErrorKind::ParentNotRemapped(*commit).into());
        let sync_outcome = sync_outcome?;

        use CommitSyncOutcome::*;
        let remapped_parent = match sync_outcome {
            RewrittenAs(cs_id) | EquivalentWorkingCopyAncestor(cs_id) => cs_id,
            Preserved => *commit,
            NotSyncCandidate => {
                return Err(ErrorKind::ParentNotSyncCandidate(*commit).into());
            }
        };

        remapped_parents.insert(*commit, remapped_parent);
    }

    rewrite_commit(cs, &remapped_parents, rewrite_path)
}

/// The state of a source repo commit in a target repo
#[derive(Debug, PartialEq)]
pub enum CommitSyncOutcome {
    /// Not suitable for syncing to this repo
    NotSyncCandidate,
    /// This commit is a 1:1 semantic mapping, but sync process rewrote it to a new ID.
    RewrittenAs(ChangesetId),
    /// This commit is exactly identical in the target repo
    Preserved,
    /// This commit is removed by the sync process, and the commit with the given ID has same content
    EquivalentWorkingCopyAncestor(ChangesetId),
}

#[derive(Clone)]
pub enum CommitSyncRepos {
    LargeToSmall {
        large_repo: BlobRepo,
        small_repo: BlobRepo,
        mover: Mover,
        bookmark_renamer: BookmarkRenamer,
    },
    SmallToLarge {
        small_repo: BlobRepo,
        large_repo: BlobRepo,
        mover: Mover,
        bookmark_renamer: BookmarkRenamer,
    },
}

#[derive(Clone)]
pub struct CommitSyncer<M> {
    // TODO: Finish refactor and remove pub
    pub mapping: M,
    pub repos: CommitSyncRepos,
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
    pub fn new(mapping: M, repos: CommitSyncRepos) -> Self {
        Self { mapping, repos }
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

    pub fn get_mover(&self) -> &Mover {
        self.repos.get_mover()
    }

    pub fn get_bookmark_renamer(&self) -> &BookmarkRenamer {
        self.repos.get_bookmark_renamer()
    }

    pub fn rename_bookmark(&self, bookmark: &BookmarkName) -> Option<BookmarkName> {
        self.repos.get_bookmark_renamer()(bookmark)
    }

    pub fn get_commit_sync_outcome_compat(
        self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
    ) -> impl Future<Item = Option<CommitSyncOutcome>, Error = Error> {
        async move { self.get_commit_sync_outcome(ctx, source_cs_id).await }
            .boxed()
            .compat()
    }

    pub async fn get_commit_sync_outcome(
        &self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
    ) -> Result<Option<CommitSyncOutcome>, Error> {
        let remapped = remap_changeset_id(
            ctx.clone(),
            source_cs_id,
            self.repos.get_source_repo(),
            self.repos.get_target_repo(),
            &self.mapping,
        )
        .await?;

        if let Some(cs_id) = remapped {
            // If we have a mapping for this commit, then it is already synced
            if cs_id == source_cs_id {
                return Ok(Some(CommitSyncOutcome::Preserved));
            } else {
                return Ok(Some(CommitSyncOutcome::RewrittenAs(cs_id)));
            }
        }

        let mapping = self.mapping.clone();
        let maybe_wc_equivalence = mapping
            .get_equivalent_working_copy(
                ctx.clone(),
                self.repos.get_source_repo().get_repoid(),
                source_cs_id,
                self.repos.get_target_repo().get_repoid(),
            )
            .compat()
            .await?;

        match maybe_wc_equivalence {
            None => Ok(None),
            Some(WorkingCopyEquivalence::NoWorkingCopy) => {
                Ok(Some(CommitSyncOutcome::NotSyncCandidate))
            }
            Some(WorkingCopyEquivalence::WorkingCopy(cs_id)) => {
                if source_cs_id == cs_id {
                    Ok(Some(CommitSyncOutcome::Preserved))
                } else {
                    Ok(Some(CommitSyncOutcome::EquivalentWorkingCopyAncestor(
                        cs_id,
                    )))
                }
            }
        }
    }

    pub fn sync_commit_compat(
        self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
    ) -> impl Future<Item = Option<ChangesetId>, Error = Error> {
        async move { self.sync_commit(ctx, source_cs_id).await }
            .boxed()
            .compat()
    }

    pub async fn sync_commit(
        &self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
    ) -> Result<Option<ChangesetId>, Error> {
        // Take most of below function sync_commit into here and delete. Leave pushrebase in next fn
        let (source_repo, _, _) = self.get_source_target_mover();

        let cs = source_repo
            .get_bonsai_changeset(ctx.clone(), source_cs_id)
            .compat()
            .await?;
        let parents: Vec<_> = cs.parents().collect();

        if parents.is_empty() {
            self.sync_commit_no_parents(ctx.clone(), cs).await
        } else if parents.len() == 1 {
            self.sync_commit_single_parent(ctx.clone(), cs).await
        } else {
            return Err(err_msg("only single-parent changesets are supported now"));
        }
    }

    pub fn preserve_commit_compat(
        self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
    ) -> impl Future<Item = (), Error = Error> {
        async move { self.preserve_commit(ctx, source_cs_id).await }
            .boxed()
            .compat()
    }

    /// The difference between `sync_commit()` and `preserve_commit()` is that `preserve_commit()`
    /// doesn't do any commit rewriting, and it requires all it's parents to be preserved.
    pub async fn preserve_commit(
        &self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
    ) -> Result<(), Error> {
        let (source_repo, target_repo, _) = self.get_source_target_mover();
        let cs = source_repo
            .get_bonsai_changeset(ctx.clone(), source_cs_id)
            .compat()
            .await?;

        for p in cs.parents() {
            let maybe_outcome = self.get_commit_sync_outcome(ctx.clone(), p).await?;
            let sync_outcome =
                maybe_outcome.ok_or(err_msg(format!("Parent commit {} is not synced yet", p)))?;

            if sync_outcome != CommitSyncOutcome::Preserved {
                return Err(err_msg(format!(
                    "trying to preserve a commit, but parent {} is not preserved",
                    p
                )));
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
        update_mapping(
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
    ) -> Result<Option<ChangesetId>, Error> {
        let source_cs_id = cs.get_changeset_id();
        let (source_repo, target_repo, rewrite_paths) = self.get_source_target_mover();

        match rewrite_commit(cs.into_mut(), &HashMap::new(), rewrite_paths)? {
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
                update_mapping(
                    ctx.clone(),
                    hashmap! { source_cs_id => frozen.get_changeset_id() },
                    &self,
                )
                .await?;
                Ok(Some(frozen.get_changeset_id()))
            }
            None => {
                self.update_wc_equivalence(ctx.clone(), source_cs_id, None)
                    .await?;
                Ok(None)
            }
        }
    }

    async fn sync_commit_single_parent(
        &self,
        ctx: CoreContext,
        cs: BonsaiChangeset,
    ) -> Result<Option<ChangesetId>, Error> {
        let source_cs_id = cs.get_changeset_id();
        let cs = cs.into_mut();
        let p = cs.parents[0];
        let (source_repo, target_repo, rewrite_paths) = self.get_source_target_mover();

        let maybe_parent_sync_outcome = self.get_commit_sync_outcome(ctx.clone(), p).await?;
        let parent_sync_outcome = maybe_parent_sync_outcome
            .ok_or(err_msg(format!("Parent commit {} is not synced yet", p)))?;

        use CommitSyncOutcome::*;
        match parent_sync_outcome {
            NotSyncCandidate => {
                // If there's not working copy for parent commit then there's no working
                // copy for child either.
                self.update_wc_equivalence(ctx.clone(), source_cs_id, None)
                    .await?;
                Ok(None)
            }
            RewrittenAs(remapped_p) | EquivalentWorkingCopyAncestor(remapped_p) => {
                let mut remapped_parents = HashMap::new();
                remapped_parents.insert(p, remapped_p);
                let maybe_rewritten = rewrite_commit(cs, &remapped_parents, rewrite_paths)?;
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
                        update_mapping(
                            ctx.clone(),
                            hashmap! { source_cs_id => frozen.get_changeset_id() },
                            &self,
                        )
                        .await?;
                        Ok(Some(frozen.get_changeset_id()))
                    }
                    None => {
                        // Source commit doesn't rewrite to any target commits.
                        // In that case equivalent working copy is the equivalent working
                        // copy of the parent
                        self.update_wc_equivalence(ctx.clone(), source_cs_id, Some(remapped_p))
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
                update_mapping(
                    ctx.clone(),
                    hashmap! { source_cs_id => source_cs_id },
                    &self,
                )
                .await?;
                Ok(Some(source_cs_id))
            }
        }
    }

    fn get_source_target_mover(&self) -> (BlobRepo, BlobRepo, Mover) {
        match self.repos.clone() {
            CommitSyncRepos::LargeToSmall {
                large_repo,
                small_repo,
                mover,
                bookmark_renamer: _,
            } => (large_repo, small_repo, mover),
            CommitSyncRepos::SmallToLarge {
                small_repo,
                large_repo,
                mover,
                bookmark_renamer: _,
            } => (small_repo, large_repo, mover),
        }
    }

    async fn update_wc_equivalence(
        &self,
        ctx: CoreContext,
        source_bcs_id: ChangesetId,
        maybe_target_bcs_id: Option<ChangesetId>,
    ) -> Result<(), Error> {
        let CommitSyncer { repos, mapping } = self.clone();
        let (source_repo, target_repo, source_is_large) = match repos {
            CommitSyncRepos::LargeToSmall {
                large_repo,
                small_repo,
                mover: _,
                bookmark_renamer: _,
            } => (large_repo, small_repo, true),
            CommitSyncRepos::SmallToLarge {
                small_repo,
                large_repo,
                mover: _,
                bookmark_renamer: _,
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
                    }
                } else {
                    EquivalentWorkingCopyEntry {
                        large_repo_id: target_repoid,
                        large_bcs_id: target_bcs_id,
                        small_repo_id: source_repoid,
                        small_bcs_id: Some(source_bcs_id),
                    }
                }
            }
            None => {
                if !source_is_large {
                    return Err(err_msg(
                        "unexpected wc equivalence update: small repo commit should always remap to large repo",
                    ));
                }
                EquivalentWorkingCopyEntry {
                    large_repo_id: source_repoid,
                    large_bcs_id: source_bcs_id,
                    small_repo_id: target_repoid,
                    small_bcs_id: None,
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
            CommitSyncRepos::LargeToSmall {
                large_repo,
                small_repo: _,
                mover: _,
                bookmark_renamer: _,
            } => large_repo,
            CommitSyncRepos::SmallToLarge {
                large_repo: _,
                small_repo,
                mover: _,
                bookmark_renamer: _,
            } => small_repo,
        }
    }

    pub fn get_target_repo(&self) -> &BlobRepo {
        match self {
            CommitSyncRepos::LargeToSmall {
                large_repo: _,
                small_repo,
                mover: _,
                bookmark_renamer: _,
            } => small_repo,
            CommitSyncRepos::SmallToLarge {
                large_repo,
                small_repo: _,
                mover: _,
                bookmark_renamer: _,
            } => large_repo,
        }
    }

    pub(crate) fn get_mover(&self) -> &Mover {
        match self {
            CommitSyncRepos::LargeToSmall {
                large_repo: _,
                small_repo: _,
                mover,
                bookmark_renamer: _,
            } => mover,
            CommitSyncRepos::SmallToLarge {
                large_repo: _,
                small_repo: _,
                mover,
                bookmark_renamer: _,
            } => mover,
        }
    }

    pub(crate) fn get_bookmark_renamer(&self) -> &BookmarkRenamer {
        match self {
            CommitSyncRepos::LargeToSmall {
                large_repo: _,
                small_repo: _,
                mover: _,
                bookmark_renamer,
            } => bookmark_renamer,
            CommitSyncRepos::SmallToLarge {
                large_repo: _,
                small_repo: _,
                mover: _,
                bookmark_renamer,
            } => bookmark_renamer,
        }
    }
}

pub fn upload_commits_compat(
    ctx: CoreContext,
    rewritten_list: Vec<BonsaiChangeset>,
    source_repo: BlobRepo,
    target_repo: BlobRepo,
) -> impl Future<Item = (), Error = Error> {
    upload_commits(ctx, rewritten_list, source_repo, target_repo)
        .boxed()
        .compat()
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
    let target_filestore_config = target_repo.get_filestore_config();
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

pub fn update_mapping_compat<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    mapped: HashMap<ChangesetId, ChangesetId>,
    config: CommitSyncer<M>,
) -> impl Future<Item = (), Error = Error> {
    async move { update_mapping(ctx, mapped, &config).await }
        .boxed()
        .compat()
}

pub async fn update_mapping<'a, M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    mapped: HashMap<ChangesetId, ChangesetId>,
    config: &'a CommitSyncer<M>,
) -> Result<(), Error> {
    let CommitSyncer { repos, mapping } = config.clone();
    let (source_repo, target_repo, source_is_large) = match repos {
        CommitSyncRepos::LargeToSmall {
            large_repo,
            small_repo,
            mover: _,
            bookmark_renamer: _,
        } => (large_repo, small_repo, true),
        CommitSyncRepos::SmallToLarge {
            small_repo,
            large_repo,
            mover: _,
            bookmark_renamer: _,
        } => (small_repo, large_repo, false),
    };

    let source_repoid = source_repo.get_repoid();
    let target_repoid = target_repo.get_repoid();

    for (from, to) in mapped {
        let entry = if source_is_large {
            SyncedCommitMappingEntry::new(source_repoid, from, target_repoid, to)
        } else {
            SyncedCommitMappingEntry::new(target_repoid, to, source_repoid, from)
        };
        mapping.add(ctx.clone(), entry).compat().await?;
    }
    Ok(())
}

/// Syncs `cs` from `source_repo` to `target_repo`, using `mapping` to rewrite commit hashes, and `rewrite_paths` to rewrite paths in the commit
/// Returns the ID of the resulting synced commit
pub async fn sync_commit<'a, M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    cs: BonsaiChangeset,
    commit_syncer: &'a CommitSyncer<M>,
    bookmark: BookmarkName,
) -> Result<Option<ChangesetId>, Error> {
    let CommitSyncer { repos, .. } = commit_syncer.clone();
    let hash = cs.get_changeset_id();
    let (source_repo, target_repo, _) = match repos.clone() {
        CommitSyncRepos::LargeToSmall {
            large_repo,
            small_repo,
            mover,
            bookmark_renamer: _,
        } => (large_repo, small_repo, mover),
        CommitSyncRepos::SmallToLarge {
            small_repo,
            large_repo,
            mover,
            bookmark_renamer: _,
        } => (small_repo, large_repo, mover),
    };

    // Rewrite the commit
    match remap_parents_and_rewrite_commit(ctx.clone(), cs.clone().into_mut(), commit_syncer)
        .await?
    {
        None => {
            let mut remapped_parents_outcome = vec![];
            for p in cs.parents() {
                let maybe_commit_sync_outcome = commit_syncer
                    .get_commit_sync_outcome(ctx.clone(), p)
                    .await?
                    .map(|sync_outcome| (sync_outcome, p));
                remapped_parents_outcome.extend(maybe_commit_sync_outcome.into_iter());
            }

            if remapped_parents_outcome.len() == 0 {
                commit_syncer
                    .update_wc_equivalence(ctx.clone(), hash, None)
                    .await?;
            } else if remapped_parents_outcome.len() == 1 {
                use CommitSyncOutcome::*;
                let (sync_outcome, parent) = &remapped_parents_outcome[0];
                let wc_equivalence = match sync_outcome {
                    NotSyncCandidate => None,
                    RewrittenAs(cs_id) | EquivalentWorkingCopyAncestor(cs_id) => Some(*cs_id),
                    Preserved => Some(*parent),
                };

                commit_syncer
                    .update_wc_equivalence(ctx.clone(), hash, wc_equivalence)
                    .await?;
            } else {
                return Err(
                    ErrorKind::AmbiguousWorkingCopyEquivalent(cs.get_changeset_id()).into(),
                );
            }

            Ok(None)
        }
        Some(rewritten) => {
            // Special case - commits with no parents (=> beginning of a repo) graft directly
            // to the bookmark, so that we can start a new sync with a fresh repo
            // Note that this won't work if the bookmark does not yet exist - don't do that
            let rewritten = {
                let mut rewritten = rewritten;
                if rewritten.parents.is_empty() {
                    target_repo
                        .get_bonsai_bookmark(ctx.clone(), &bookmark)
                        .map(|bookmark_cs| {
                            bookmark_cs.map(|bookmark_cs| rewritten.parents = vec![bookmark_cs]);
                        })
                        .compat()
                        .await?
                }
                rewritten
            };

            // Sync commit
            let frozen = rewritten.freeze()?;
            let rewritten_list = vec![frozen];
            upload_commits(
                ctx.clone(),
                rewritten_list.clone(),
                source_repo.clone(),
                target_repo.clone(),
            )
            .await?;

            let pushrebase_params = {
                let mut params = PushrebaseParams::default();
                params.rewritedates = false;
                params.forbid_p2_root_rebases = false;
                params.casefolding_check = false;
                params.recursion_limit = None;
                params
            };
            let bookmark = OntoBookmarkParams { bookmark };
            let pushrebase_res = do_pushrebase_bonsai(
                ctx.clone(),
                target_repo,
                pushrebase_params,
                bookmark,
                rewritten_list,
                None,
            )
            .compat()
            .await;
            let pushrebase_res =
                pushrebase_res.map_err(|e| Error::from(ErrorKind::PushrebaseFailure(e)))?;
            let pushrebased_changeset = pushrebase_res.head;
            update_mapping(
                ctx.clone(),
                hashmap! { hash => pushrebased_changeset },
                commit_syncer,
            )
            .await?;
            Ok(Some(pushrebased_changeset))
        }
    }
}

pub fn sync_commit_compat<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    cs: BonsaiChangeset,
    config: CommitSyncer<M>,
    bookmark: BookmarkName,
) -> impl Future<Item = Option<ChangesetId>, Error = Error> {
    async move { sync_commit(ctx, cs, &config, bookmark).await }
        .boxed()
        .compat()
}

pub struct Syncers<M: SyncedCommitMapping + Clone + 'static> {
    pub large_to_small: CommitSyncer<M>,
    pub small_to_large: CommitSyncer<M>,
}

pub fn create_commit_syncers<M>(
    small_repo: BlobRepo,
    large_repo: BlobRepo,
    commit_sync_config: &CommitSyncConfig,
    mapping: M,
) -> Result<Syncers<M>, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let small_repo_id = small_repo.get_repoid();

    let small_to_large_mover = get_small_to_large_mover(commit_sync_config, small_repo_id)?;
    let large_to_small_mover = get_large_to_small_mover(commit_sync_config, small_repo_id)?;

    let small_to_large_renamer = get_small_to_large_renamer(commit_sync_config, small_repo_id)?;
    let large_to_small_renamer = get_large_to_small_renamer(commit_sync_config, small_repo_id)?;

    let small_to_large_commit_sync_repos = CommitSyncRepos::SmallToLarge {
        small_repo: small_repo.clone(),
        large_repo: large_repo.clone(),
        mover: small_to_large_mover.clone(),
        bookmark_renamer: small_to_large_renamer.clone(),
    };

    let large_to_small_commit_sync_repos = CommitSyncRepos::LargeToSmall {
        small_repo,
        large_repo,
        mover: large_to_small_mover,
        bookmark_renamer: large_to_small_renamer,
    };

    let large_to_small_commit_syncer = CommitSyncer {
        mapping: mapping.clone(),
        repos: large_to_small_commit_sync_repos,
    };
    let small_to_large_commit_syncer = CommitSyncer {
        mapping,
        repos: small_to_large_commit_sync_repos,
    };

    Ok(Syncers {
        large_to_small: large_to_small_commit_syncer,
        small_to_large: small_to_large_commit_syncer,
    })
}
