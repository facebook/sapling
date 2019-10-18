/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::{BTreeMap, HashMap};

use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobsync::copy_content;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use failure::{err_msg, Error, Fail};
use futures::Future;
use futures_preview::{
    compat::Future01CompatExt,
    future::{FutureExt, TryFutureExt},
    stream::{futures_unordered::FuturesUnordered, TryStreamExt},
};
use maplit::hashmap;
use metaconfig_types::PushrebaseParams;
use mononoke_types::{
    BonsaiChangeset, BonsaiChangesetMut, ChangesetId, FileChange, MPath, RepositoryId,
};
use movers::Mover;
use pushrebase::{do_pushrebase_bonsai, OntoBookmarkParams, PushrebaseError};
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
                    let copy_from_commit =
                        remapped_parents
                            .get(copy_from_commit)
                            .ok_or(err_msg(format!(
                                "Copy from commit not found: {}",
                                copy_from_commit
                            )))?;
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
        let remapped = remapped_parents.get(commit).ok_or(err_msg(format!(
            "mapping for parent commit {} not found!",
            commit
        )))?;

        *commit = *remapped;
    }

    Ok(Some(cs))
}

/// Applies `rewrite_path` to all paths in `cs`, dropping any entry whose path rewrites to `None`
/// E.g. adding a prefix can be done by a `rewrite` that adds the prefix and returns `Some(path)`.
/// Removing a prefix would be like adding, but returning `None` if the path does not have the prefix
/// Additionally, changeset IDs are rewritten, and the post-rewrite changeset IDs are returned for
/// verification (e.g. to ensure that all changeset IDs have been correctly rewritten into IDs
/// that will be present in the target repo after a cross-repo sync)
async fn remap_parents_and_rewrite_commit<M: SyncedCommitMapping>(
    ctx: CoreContext,
    mut cs: BonsaiChangesetMut,
    source_repo_id: RepositoryId,
    target_repo_id: RepositoryId,
    mapping: &M,
    rewrite_path: Mover,
) -> Result<Option<(BonsaiChangesetMut, Vec<ChangesetId>)>, Error> {
    let mut changesets_to_check: Vec<ChangesetId> = Vec::new();
    let mut remapped_parents = HashMap::new();
    for commit in cs.parents.iter_mut() {
        let remapped_commit = mapping
            .get(ctx.clone(), source_repo_id, *commit, target_repo_id)
            .compat()
            .await?;
        // If it doesn't remap, we will optimistically assume that the
        // target is already in the repo - this is passed out
        // to the caller to validate, as Mercurial has trouble if it's not true
        let remapped_commit = remapped_commit.unwrap_or(*commit);
        changesets_to_check.push(remapped_commit.clone());
        remapped_parents.insert(commit.clone(), remapped_commit);
    }

    let cs = rewrite_commit(cs, &remapped_parents, rewrite_path)?;
    Ok(cs.map(|cs| (cs, changesets_to_check)))
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
    pub fn get_source_repo(&self) -> &BlobRepo {
        match self {
            CommitSyncRepos::LargeToSmall {
                large_repo,
                small_repo: _,
            } => large_repo,
            CommitSyncRepos::SmallToLarge {
                large_repo: _,
                small_repo,
            } => small_repo,
        }
    }

    pub fn get_target_repo(&self) -> &BlobRepo {
        match self {
            CommitSyncRepos::LargeToSmall {
                large_repo: _,
                small_repo,
            } => small_repo,
            CommitSyncRepos::SmallToLarge {
                large_repo,
                small_repo: _,
            } => large_repo,
        }
    }

    pub fn get_source_repo_id(&self) -> RepositoryId {
        self.get_source_repo().get_repoid()
    }

    pub fn get_target_repo_id(&self) -> RepositoryId {
        self.get_target_repo().get_repoid()
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
    repos: CommitSyncRepos,
    mapping: M,
) -> impl Future<Item = (), Error = Error> {
    update_mapping(ctx, mapped, repos, mapping).boxed().compat()
}

pub async fn update_mapping<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    mapped: HashMap<ChangesetId, ChangesetId>,
    repos: CommitSyncRepos,
    mapping: M,
) -> Result<(), Error> {
    let (source_repo, target_repo, source_is_large) = match repos {
        CommitSyncRepos::LargeToSmall {
            large_repo,
            small_repo,
        } => (large_repo, small_repo, true),
        CommitSyncRepos::SmallToLarge {
            small_repo,
            large_repo,
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
pub async fn sync_commit<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    cs: BonsaiChangeset,
    repos: CommitSyncRepos,
    bookmark: BookmarkName,
    mapping: M,
    rewrite_paths: Mover,
) -> Result<Option<ChangesetId>, Error> {
    let hash = cs.get_changeset_id();
    let (source_repo, target_repo) = match repos.clone() {
        CommitSyncRepos::LargeToSmall {
            large_repo,
            small_repo,
        } => (large_repo, small_repo),
        CommitSyncRepos::SmallToLarge {
            small_repo,
            large_repo,
        } => (small_repo, large_repo),
    };
    let source_repoid = source_repo.get_repoid();
    let target_repoid = target_repo.get_repoid();
    // Rewrite the commit
    match remap_parents_and_rewrite_commit(
        ctx.clone(),
        cs.into_mut(),
        source_repoid,
        target_repoid,
        &mapping,
        rewrite_paths,
    )
    .await?
    {
        None => Ok(None),
        Some((rewritten, changesets)) => {
            // And check changesets are all in target
            let changesets_check: FuturesUnordered<_> = changesets
                .into_iter()
                .map({
                    |cs| {
                        cloned!(ctx, cs, target_repo);
                        async move {
                            if !target_repo
                                .changeset_exists_by_bonsai(ctx, cs)
                                .compat()
                                .await?
                            {
                                Err(ErrorKind::MissingRemappedCommit(cs).into())
                            } else {
                                Ok(())
                            }
                        }
                    }
                })
                .collect();
            changesets_check
                .try_for_each_concurrent(100, identity)
                .await?;

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
                repos,
                mapping,
            )
            .await?;
            Ok(Some(pushrebased_changeset))
        }
    }
}

pub fn sync_commit_compat<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    cs: BonsaiChangeset,
    repos: CommitSyncRepos,
    bookmark: BookmarkName,
    mapping: M,
    rewrite_paths: Mover,
) -> impl Future<Item = Option<ChangesetId>, Error = Error> {
    sync_commit(ctx, cs, repos, bookmark, mapping, rewrite_paths)
        .boxed()
        .compat()
}
