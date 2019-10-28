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
use bookmarks::BookmarkName;
use context::CoreContext;
use failure::{Error, Fail};
use futures::Future;
use futures_preview::{
    compat::{Future01CompatExt, Stream01CompatExt},
    future::{self, FutureExt, TryFutureExt},
    stream::{futures_unordered::FuturesUnordered, StreamExt as _, TryStreamExt},
};
use maplit::hashmap;
use metaconfig_types::PushrebaseParams;
use mononoke_types::{BonsaiChangeset, BonsaiChangesetMut, ChangesetId, FileChange, MPath};
use movers::Mover;
use pushrebase::{do_pushrebase_bonsai, OntoBookmarkParams, PushrebaseError};
use revset::AncestorsNodeStream;
use synced_commit_mapping::{SyncedCommitMapping, SyncedCommitMappingEntry};

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

async fn search_for_same_wc<'a, M: SyncedCommitMapping>(
    ctx: CoreContext,
    cs: ChangesetId,
    source_repo: &'a BlobRepo,
    target_repo: &'a BlobRepo,
    mapping: &'a M,
) -> Result<Option<ChangesetId>, Error> {
    let mut candidate_commits =
        AncestorsNodeStream::new(ctx.clone(), &source_repo.get_changeset_fetcher(), cs)
            .compat()
            .and_then(|candidate| {
                remap_changeset_id(ctx.clone(), candidate, source_repo, target_repo, mapping)
            })
            .try_skip_while(|r| future::ok(r.is_none()))
            .boxed();

    Ok(candidate_commits.try_next().await?.flatten())
}

/// Applies `rewrite_path` to all paths in `cs`, dropping any entry whose path rewrites to `None`
/// E.g. adding a prefix can be done by a `rewrite` that adds the prefix and returns `Some(path)`.
/// Removing a prefix would be like adding, but returning `None` if the path does not have the prefix
/// Additionally, changeset IDs are rewritten, and the post-rewrite changeset IDs are returned for
/// verification (e.g. to ensure that all changeset IDs have been correctly rewritten into IDs
/// that will be present in the target repo after a cross-repo sync)
///
/// Precondition: *all* parents must already have been rewritten into the target repo. The
/// behaviour of this function is unpredictable if some parents have not yet been remapped
async fn remap_parents_and_rewrite_commit<'a, M: SyncedCommitMapping>(
    ctx: CoreContext,
    mut cs: BonsaiChangesetMut,
    source_repo: &'a BlobRepo,
    target_repo: &'a BlobRepo,
    mapping: &'a M,
    rewrite_path: Mover,
) -> Result<Option<BonsaiChangesetMut>, Error> {
    let mut remapped_parents = HashMap::new();
    for commit in cs.parents.iter_mut() {
        let remapped_parent =
            search_for_same_wc(ctx.clone(), *commit, source_repo, target_repo, mapping)
                .await?
                .ok_or(Error::from(ErrorKind::SameWcSearchFail(*commit)))?;
        remapped_parents.insert(*commit, remapped_parent);
    }

    rewrite_commit(cs, &remapped_parents, rewrite_path)
}

/// The state of a source repo commit in a target repo
#[derive(Debug)]
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
    },
    SmallToLarge {
        small_repo: BlobRepo,
        large_repo: BlobRepo,
        mover: Mover,
    },
}

#[derive(Clone)]
pub struct CommitSyncConfig<M> {
    // TODO: Finish refactor and remove pub
    pub mapping: M,
    pub repos: CommitSyncRepos,
}

impl<M> CommitSyncConfig<M>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    pub fn new(mapping: M, repos: CommitSyncRepos) -> Self {
        Self { mapping, repos }
    }

    pub fn get_source_repo(&self) -> &BlobRepo {
        self.repos.get_source_repo()
    }

    pub fn get_target_repo(&self) -> &BlobRepo {
        self.repos.get_target_repo()
    }

    pub fn get_mapping(&self) -> &M {
        &self.mapping
    }

    pub fn get_mover(&self) -> &Mover {
        self.repos.get_mover()
    }

    pub fn get_commit_sync_outcome_compat(
        self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
    ) -> impl Future<Item = Option<CommitSyncOutcome>, Error = Error> {
        async fn get_commit_sync_outcome_wrapper<M>(
            this: CommitSyncConfig<M>,
            ctx: CoreContext,
            source_cs_id: ChangesetId,
        ) -> Result<Option<CommitSyncOutcome>, Error>
        where
            M: SyncedCommitMapping + Clone + 'static,
        {
            this.get_commit_sync_outcome(ctx, source_cs_id).await
        }
        get_commit_sync_outcome_wrapper(self, ctx, source_cs_id)
            .boxed()
            .compat()
    }

    /// Check to see if this commit should exist in the target repo
    /// Either the source repo is small or the source cs has files that should be present in the target repo
    async fn should_be_present_in_target(
        &self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
    ) -> Result<bool, Error> {
        if !self.repos.source_is_large() {
            return Ok(true);
        }

        let source_cs = self
            .get_source_repo()
            .get_bonsai_changeset(ctx.clone(), source_cs_id)
            .compat()
            .await?;

        let remapped_files: Result<Vec<_>, Error> = source_cs
            .file_changes()
            .map(|(path, _)| self.get_mover()(path))
            .collect();

        let remapped_files = remapped_files?;

        // A commit will be synced if it touches no files at all, or if at least one
        // file is kept in the target repo
        Ok(remapped_files.is_empty()
            || remapped_files
                .into_iter()
                .any(|opt_path| opt_path.is_some()))
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

        let should_be_present_in_target = self
            .should_be_present_in_target(ctx.clone(), source_cs_id)
            .await?;

        // Do an ancestor walk to find out what outcome (if any) is in place
        // TODO(T56351515): Replace this by recursion on parents, to correctly handle merges
        let mut ancestors = AncestorsNodeStream::new(
            ctx.clone(),
            &self.get_source_repo().get_changeset_fetcher(),
            source_cs_id,
        )
        .compat()
        // Skip the commit we're considering - it's always the first to be returned, and we know it doesn't remap
        .skip(1);

        while let Some(ancestor) = ancestors.try_next().await? {
            if let Some(remapped_ancestor) = remap_changeset_id(
                ctx.clone(),
                ancestor,
                self.repos.get_source_repo(),
                self.repos.get_target_repo(),
                &self.mapping,
            )
            .await?
            {
                // We have an ancestor that's been synced.
                // If that ancestor is unchanged, then we need to be synced, too, regardless of what files we change
                // TODO(T56351515): This will only apply if all parents are unchanged in a merge situation
                if remapped_ancestor == ancestor {
                    return Ok(None);
                }
                // If we change files in the target repo, then we need to be synced, too.
                else if should_be_present_in_target {
                    return Ok(None);
                }
                // Finally, if the ancestor has been rewritten, but we don't change the target repo,
                // then we have the same working copy as that ancestor in the target repo
                // TODO(T56351515): Is this true in a merge situation?
                else {
                    return Ok(Some(CommitSyncOutcome::EquivalentWorkingCopyAncestor(
                        remapped_ancestor,
                    )));
                }
            } else {
                // If this ancestor should be synced, but isn't, then we need to be synced, too
                if self
                    .should_be_present_in_target(ctx.clone(), ancestor)
                    .await?
                {
                    return Ok(None);
                }
            }
        }

        // The commit does not belong to this sync DAG - don't sync it
        Ok(Some(CommitSyncOutcome::NotSyncCandidate))
    }

    pub fn sync_commit_compat(
        self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
    ) -> impl Future<Item = Option<ChangesetId>, Error = Error> {
        async fn sync_commit_compat_wrapper<M>(
            this: CommitSyncConfig<M>,
            ctx: CoreContext,
            source_cs_id: ChangesetId,
        ) -> Result<Option<ChangesetId>, Error>
        where
            M: SyncedCommitMapping + Clone + 'static,
        {
            this.sync_commit(ctx, source_cs_id).await
        }
        sync_commit_compat_wrapper(self, ctx, source_cs_id)
            .boxed()
            .compat()
    }

    pub async fn sync_commit(
        &self,
        ctx: CoreContext,
        source_cs_id: ChangesetId,
    ) -> Result<Option<ChangesetId>, Error> {
        // Take most of below function sync_commit into here and delete. Leave pushrebase in next fn
        let repos = self.repos.clone();
        let mapping = self.mapping.clone();
        let (source_repo, target_repo, rewrite_paths) = match repos.clone() {
            CommitSyncRepos::LargeToSmall {
                large_repo,
                small_repo,
                mover,
            } => (large_repo, small_repo, mover),
            CommitSyncRepos::SmallToLarge {
                small_repo,
                large_repo,
                mover,
            } => (small_repo, large_repo, mover),
        };

        let cs = source_repo
            .get_bonsai_changeset(ctx.clone(), source_cs_id)
            .compat()
            .await?;
        // Rewrite the commit
        match remap_parents_and_rewrite_commit(
            ctx.clone(),
            cs.into_mut(),
            &source_repo,
            &target_repo,
            &mapping,
            rewrite_paths,
        )
        .await?
        {
            None => Ok(None),
            Some(rewritten) => {
                // Sync commit
                let frozen = rewritten.freeze()?;
                let rewritten_cs_id = frozen.get_changeset_id();
                let rewritten_list = vec![frozen];
                upload_commits(
                    ctx.clone(),
                    rewritten_list.clone(),
                    source_repo.clone(),
                    target_repo.clone(),
                )
                .await?;

                update_mapping(
                    ctx.clone(),
                    hashmap! { source_cs_id => rewritten_cs_id },
                    &self,
                )
                .await?;
                Ok(Some(rewritten_cs_id))
            }
        }
    }
}

impl CommitSyncRepos {
    pub fn get_source_repo(&self) -> &BlobRepo {
        match self {
            CommitSyncRepos::LargeToSmall {
                large_repo,
                small_repo: _,
                mover: _,
            } => large_repo,
            CommitSyncRepos::SmallToLarge {
                large_repo: _,
                small_repo,
                mover: _,
            } => small_repo,
        }
    }

    pub fn get_target_repo(&self) -> &BlobRepo {
        match self {
            CommitSyncRepos::LargeToSmall {
                large_repo: _,
                small_repo,
                mover: _,
            } => small_repo,
            CommitSyncRepos::SmallToLarge {
                large_repo,
                small_repo: _,
                mover: _,
            } => large_repo,
        }
    }

    pub(crate) fn source_is_large(&self) -> bool {
        match self {
            CommitSyncRepos::LargeToSmall { .. } => true,
            CommitSyncRepos::SmallToLarge { .. } => false,
        }
    }

    pub(crate) fn get_mover(&self) -> &Mover {
        match self {
            CommitSyncRepos::LargeToSmall {
                large_repo: _,
                small_repo: _,
                mover,
            } => mover,
            CommitSyncRepos::SmallToLarge {
                large_repo: _,
                small_repo: _,
                mover,
            } => mover,
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
    config: CommitSyncConfig<M>,
) -> impl Future<Item = (), Error = Error> {
    async fn update_mapping_compat_wrapper<M: SyncedCommitMapping + Clone + 'static>(
        ctx: CoreContext,
        mapped: HashMap<ChangesetId, ChangesetId>,
        config: CommitSyncConfig<M>,
    ) -> Result<(), Error> {
        update_mapping(ctx, mapped, &config).await
    }

    update_mapping_compat_wrapper(ctx, mapped, config)
        .boxed()
        .compat()
}

pub async fn update_mapping<'a, M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    mapped: HashMap<ChangesetId, ChangesetId>,
    config: &'a CommitSyncConfig<M>,
) -> Result<(), Error> {
    let CommitSyncConfig { repos, mapping } = config.clone();
    let (source_repo, target_repo, source_is_large) = match repos {
        CommitSyncRepos::LargeToSmall {
            large_repo,
            small_repo,
            mover: _,
        } => (large_repo, small_repo, true),
        CommitSyncRepos::SmallToLarge {
            small_repo,
            large_repo,
            mover: _,
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
    config: &'a CommitSyncConfig<M>,
    bookmark: BookmarkName,
) -> Result<Option<ChangesetId>, Error> {
    let CommitSyncConfig { repos, mapping } = config.clone();
    let hash = cs.get_changeset_id();
    let (source_repo, target_repo, rewrite_paths) = match repos.clone() {
        CommitSyncRepos::LargeToSmall {
            large_repo,
            small_repo,
            mover,
        } => (large_repo, small_repo, mover),
        CommitSyncRepos::SmallToLarge {
            small_repo,
            large_repo,
            mover,
        } => (small_repo, large_repo, mover),
    };

    // Rewrite the commit
    match remap_parents_and_rewrite_commit(
        ctx.clone(),
        cs.into_mut(),
        &source_repo,
        &target_repo,
        &mapping,
        rewrite_paths,
    )
    .await?
    {
        None => Ok(None),
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
                config,
            )
            .await?;
            Ok(Some(pushrebased_changeset))
        }
    }
}

pub fn sync_commit_compat<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    cs: BonsaiChangeset,
    config: CommitSyncConfig<M>,
    bookmark: BookmarkName,
) -> impl Future<Item = Option<ChangesetId>, Error = Error> {
    async fn sync_commit_compat_wrapper<M: SyncedCommitMapping + Clone + 'static>(
        ctx: CoreContext,
        cs: BonsaiChangeset,
        config: CommitSyncConfig<M>,
        bookmark: BookmarkName,
    ) -> Result<Option<ChangesetId>, Error> {
        sync_commit(ctx, cs, &config, bookmark).await
    }

    sync_commit_compat_wrapper(ctx, cs, config, bookmark)
        .boxed()
        .compat()
}
