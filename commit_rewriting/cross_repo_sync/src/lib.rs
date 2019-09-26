// Copyright (c) 2019-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::BTreeMap;

use blobrepo::{save_bonsai_changesets, BlobRepo};
use blobsync::copy_content;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use failure::{Error, Fail};
use futures::Future;
use futures_preview::{
    compat::Future01CompatExt,
    future::{ok, try_join, FutureExt, TryFutureExt},
    stream::{futures_unordered::FuturesUnordered, TryStreamExt},
};
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
/// Applies `rewrite_path` to all paths in `cs`, dropping any entry whose path rewrites to `None`
/// E.g. adding a prefix can be done by a `rewrite` that adds the prefix and returns `Some(path)`.
/// Removing a prefix would be like adding, but returning `None` if the path does not have the prefix
/// Additionally, changeset IDs are rewritten, and the post-rewrite changeset IDs are returned for
/// verification (e.g. to ensure that all changeset IDs have been correctly rewritten into IDs
/// that will be present in the target repo after a cross-repo sync)
async fn rewrite_commit<M: SyncedCommitMapping>(
    ctx: CoreContext,
    mut cs: BonsaiChangesetMut,
    source_repo_id: RepositoryId,
    target_repo_id: RepositoryId,
    mapping: &M,
    rewrite_path: Mover,
) -> Result<Option<(BonsaiChangesetMut, Vec<ChangesetId>)>, Error> {
    let mut changesets: Vec<ChangesetId> = Vec::new();
    // Empty commits should always sync as-is; there is no path rewriting involved here.
    if !cs.file_changes.is_empty() {
        let mut copyfrom_remap = FuturesUnordered::new();
        // Update the changelist
        let mut new_changes = BTreeMap::new();

        let path_rewritten_changes = cs.file_changes.into_iter().filter_map(|(path, change)| {
            // Just rewrite copy_from information, when we have it
            fn rewrite_copy_from(
                copy_from: &(MPath, ChangesetId),
                rewrite_path: Mover,
            ) -> Result<Option<(MPath, ChangesetId)>, Error> {
                let (path, cs) = copy_from;
                let new_path = rewrite_path(&path)?;
                Ok(new_path.map(|new_path| (new_path, *cs)))
            }

            // Extract any copy_from information, and use rewrite_copy_from on it
            fn rewrite_file_change(
                change: FileChange,
                rewrite_path: Mover,
            ) -> Result<(FileChange, Option<(MPath, ChangesetId)>), Error> {
                let new_copy_from = change
                    .copy_from()
                    .and_then(|copy_from| rewrite_copy_from(copy_from, rewrite_path).transpose())
                    .transpose()?;
                Ok((change, new_copy_from))
            }

            // Rewrite both path and changes
            fn do_rewrite(
                path: MPath,
                change: Option<FileChange>,
                rewrite_path: Mover,
            ) -> Result<Option<(MPath, Option<(FileChange, Option<(MPath, ChangesetId)>)>)>, Error>
            {
                let new_path = rewrite_path(&path)?;
                let change = change
                    .map(|change| rewrite_file_change(change, rewrite_path.clone()))
                    .transpose()?;
                Ok(new_path.map(|new_path| (new_path, change)))
            }
            do_rewrite(path, change, rewrite_path.clone()).transpose()
        });

        for rewritten_change in path_rewritten_changes {
            // Now rewrite copy_from hashes. Any hash that can't remap is kept as-is
            // This is to aid with syncing to/from imported repos. We assume that
            // commits that don't remap were from before the merge, and rely on the caller
            // verifying that all commits are actually in the destination repo.
            match rewritten_change {
                Err(e) => return Err(e),
                Ok((path, change)) => match change {
                    None => {
                        new_changes.insert(path, None);
                    }
                    Some((old_change, None)) => {
                        new_changes.insert(path, Some(old_change));
                    }
                    Some((old_change, Some((copy_path, commit)))) => {
                        cloned!(ctx);
                        let remap_fut = async move {
                            let remapped_commit = mapping
                                .get(ctx, source_repo_id, commit, target_repo_id)
                                .compat()
                                .await?;
                            // If it doesn't remap, we will optimistically assume that the
                            // target is already in the repo - this is passed out
                            // to the caller to validate, as Mercurial has trouble if it's not true
                            let new_changeset = remapped_commit.unwrap_or(commit);
                            let new_change = FileChange::with_new_copy_from(
                                old_change,
                                Some((copy_path, new_changeset)),
                            );
                            let res: Result<_, Error> = Ok((path, new_change));
                            res
                        };
                        copyfrom_remap.push(remap_fut)
                    }
                },
            }
        }

        copyfrom_remap
            .try_for_each_concurrent(100, {
                |(path, change)| {
                    if let Some((_, changeset)) = change.copy_from() {
                        changesets.push(changeset.clone());
                    }
                    new_changes.insert(path, Some(change));
                    ok(())
                }
            })
            .await?;
        // Empty change after rewriting, but not before, so we filtered everything out. Just return
        if new_changes.is_empty() {
            return Ok(None);
        }
        cs.file_changes = new_changes;
    }

    // Update hashes
    for commit in cs.parents.iter_mut() {
        let remapped_commit = mapping
            .get(ctx.clone(), source_repo_id, *commit, target_repo_id)
            .compat()
            .await?;
        // This will be passed out to the caller to validate, as Mercurial has trouble if
        // the parent is missing in the repo
        // TODO(T54125963): Walk backwards when this happens, to find a valid parent in the child repo
        let changeset = remapped_commit.unwrap_or(*commit);
        changesets.push(changeset);
        *commit = changeset;
    }
    Ok(Some((cs, changesets)))
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
    // Rewrite the commit
    match rewrite_commit(
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
            // Upload files
            let files_to_sync: Vec<_> = rewritten
                .file_changes
                .values()
                .filter_map(|opt_change| opt_change.as_ref().map(|change| change.content_id()))
                .collect();
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
            try_join(
                uploader.try_for_each_concurrent(100, identity),
                changesets_check.try_for_each_concurrent(100, identity),
            )
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
            save_bonsai_changesets(rewritten_list.clone(), ctx.clone(), target_repo.clone())
                .compat()
                .await?;

            let pushrebase_params = {
                let mut params = PushrebaseParams::default();
                params.rewritedates = false;
                params.forbid_p2_root_rebases = false;
                params.casefolding_check = false;
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
            let changeset = pushrebase_res.head;
            let entry = if source_is_large {
                SyncedCommitMappingEntry::new(source_repoid, hash, target_repoid, changeset)
            } else {
                SyncedCommitMappingEntry::new(target_repoid, changeset, source_repoid, hash)
            };
            mapping.add(ctx, entry).compat().await?;
            Ok(Some(changeset))
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
