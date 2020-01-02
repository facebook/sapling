/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use anyhow::{format_err, Error};
use futures::{stream, Future, Stream};
use futures_ext::{BoxFuture, FutureExt, StreamExt};

use super::{CommitSyncOutcome, CommitSyncer};
use blobrepo::BlobRepo;
use bookmark_renaming::BookmarkRenamer;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use futures_preview::{compat::Future01CompatExt, future::FutureExt as PreviewFutureExt};
use futures_util::{
    stream::{self as new_stream, StreamExt as NewStreamExt},
    try_join, TryStreamExt,
};
use manifest::{Entry, ManifestOps};
use mercurial_types::{HgFileNodeId, HgManifestId};
use mononoke_types::{ChangesetId, MPath};
use movers::Mover;
use slog::{debug, error, info};
use std::collections::{HashMap, HashSet};
use synced_commit_mapping::SyncedCommitMapping;

pub async fn verify_working_copy<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: CommitSyncer<M>,
    large_hash: ChangesetId,
) -> Result<(), Error> {
    let large_repo = commit_syncer.get_large_repo();
    let small_repo = commit_syncer.get_small_repo();

    let small_hash = get_synced_commit(ctx.clone(), &commit_syncer, large_hash).await?;
    info!(ctx.logger(), "small repo cs id: {}", small_hash);

    let moved_large_repo_entries = async {
        let large_root_mf_id =
            fetch_root_mf_id(ctx.clone(), large_repo.clone(), large_hash.clone()).await?;

        let large_repo_entries =
            list_all_filenode_ids(ctx.clone(), large_repo.clone(), large_root_mf_id)
                .compat()
                .await?;

        if large_hash == small_hash {
            // No need to move any paths, because this commit was preserved as is
            Ok(large_repo_entries)
        } else {
            move_all_paths(large_repo_entries, commit_syncer.get_mover())
        }
    };

    let small_repo_entries = async {
        let small_root_mf_id =
            fetch_root_mf_id(ctx.clone(), small_repo.clone(), small_hash.clone()).await?;

        list_all_filenode_ids(ctx.clone(), small_repo.clone(), small_root_mf_id)
            .compat()
            .await
    };

    let (moved_large_repo_entries, small_repo_entries) =
        try_join!(moved_large_repo_entries, small_repo_entries)?;

    compare_contents(
        ctx.clone(),
        (large_repo.clone(), &moved_large_repo_entries),
        (small_repo.clone(), &small_repo_entries),
        large_hash,
    )
    .await?;

    let mut missing_count = 0;
    for (path, _) in small_repo_entries {
        if moved_large_repo_entries.get(&path).is_none() {
            error!(
                ctx.logger(),
                "{:?} is present in small repo, but not in large", path
            );
            missing_count = missing_count + 1;
        }
    }

    if missing_count > 0 {
        return Err(format_err!(
            "{} files are present in small repo, but not in large",
            missing_count
        )
        .into());
    }

    info!(ctx.logger(), "all is well!");
    Ok(())
}

pub async fn find_bookmark_diff<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
) -> Result<Vec<BookmarkDiff>, Error> {
    let large_repo = commit_syncer.get_large_repo();
    let small_repo = commit_syncer.get_small_repo();
    let small_bookmarks = small_repo
        .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
        .map(|(bookmark, cs_id)| (bookmark.name().clone(), cs_id))
        .collect_to::<HashMap<_, _>>()
        .compat()
        .await?;

    let renamed_large_bookmarks = {
        let large_bookmarks = large_repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .map(|(bookmark, cs_id)| (bookmark.name().clone(), cs_id))
            .collect()
            .compat()
            .await?;

        // Renames bookmarks and also maps large cs ids to small cs ids
        rename_large_repo_bookmarks(
            ctx.clone(),
            &commit_syncer,
            commit_syncer.get_bookmark_renamer(),
            large_bookmarks,
        )
        .await?
    };

    // Compares small bookmarks (i.e. bookmarks from small repo) with large bookmarks.
    // Note that renamed_large_bookmarks are key value pairs where key is a renamed large repo
    // bookmark and value is a remapped large repo cs id.
    let mut diff = vec![];
    for (small_book, small_cs_id) in &small_bookmarks {
        // actual_small_cs_id is a commit in a small repo that corresponds to a commit
        // in a large repo which is pointed by this bookmark.
        let actual_small_cs_id = renamed_large_bookmarks.get(small_book);
        if actual_small_cs_id != Some(small_cs_id) {
            diff.push(BookmarkDiff::InconsistentValue {
                small_bookmark: small_book.clone(),
                expected_small_cs_id: small_cs_id.clone(),
                actual_small_cs_id: actual_small_cs_id.cloned(),
            });
        }
    }

    for renamed_large_book in renamed_large_bookmarks.keys() {
        if !small_bookmarks.contains_key(renamed_large_book) {
            diff.push(BookmarkDiff::ShouldBeDeleted {
                small_bookmark: renamed_large_book.clone(),
            });
        }
    }

    Ok(diff)
}

fn list_all_filenode_ids(
    ctx: CoreContext,
    repo: BlobRepo,
    mf_id: HgManifestId,
) -> BoxFuture<HashMap<Option<MPath>, HgFileNodeId>, Error> {
    info!(
        ctx.logger(),
        "fetching filenode ids for {}",
        repo.get_repoid()
    );
    mf_id
        .list_all_entries(ctx.clone(), repo.get_blobstore())
        .filter_map(move |(path, entry)| match entry {
            Entry::Leaf((_, filenode_id)) => Some((path, filenode_id)),
            Entry::Tree(_) => None,
        })
        .collect_to::<HashMap<_, _>>()
        .inspect(move |res| {
            debug!(
                ctx.logger(),
                "fetched {} filenode ids for {}",
                res.len(),
                repo.get_repoid()
            );
        })
        .boxify()
}

async fn compare_contents(
    ctx: CoreContext,
    (large_repo, large_filenodes): (BlobRepo, &HashMap<Option<MPath>, HgFileNodeId>),
    (small_repo, small_filenodes): (BlobRepo, &HashMap<Option<MPath>, HgFileNodeId>),
    large_hash: ChangesetId,
) -> Result<(), Error> {
    let mut different_filenodes = HashSet::new();
    for (path, left_filenode_id) in large_filenodes {
        let maybe_right_filenode_id = small_filenodes.get(&path);
        if maybe_right_filenode_id != Some(&left_filenode_id) {
            match maybe_right_filenode_id {
                Some(right_filenode_id) => {
                    different_filenodes.insert((
                        path.clone(),
                        *left_filenode_id,
                        *right_filenode_id,
                    ));
                }
                None => {
                    return Err(format_err!(
                        "{:?} exists in large repo but not in small repo",
                        path
                    ));
                }
            }
        }
    }

    info!(
        ctx.logger(),
        "found {} filenodes that are different, checking content...",
        different_filenodes.len(),
    );

    let fetched_content_ids = stream::iter_ok(different_filenodes)
        .map({
            cloned!(ctx, large_repo, small_repo);
            move |(path, left_filenode_id, right_filenode_id)| {
                debug!(
                    ctx.logger(),
                    "checking content for different filenodes: {} vs {}",
                    left_filenode_id,
                    right_filenode_id,
                );
                let f1 = large_repo.get_file_content_id(ctx.clone(), left_filenode_id);
                let f2 = small_repo.get_file_content_id(ctx.clone(), right_filenode_id);

                f1.join(f2).map(move |(c1, c2)| (path, c1, c2))
            }
        })
        .buffered(1000)
        .collect()
        .compat()
        .await?;

    for (path, small_content_id, large_content_id) in fetched_content_ids {
        if small_content_id != large_content_id {
            return Err(format_err!(
                "different contents for {:?}: {} vs {}, {}",
                path,
                small_content_id,
                large_content_id,
                large_hash,
            ));
        }
    }

    Ok(())
}

fn move_all_paths(
    filenodes: HashMap<Option<MPath>, HgFileNodeId>,
    mover: &Mover,
) -> Result<HashMap<Option<MPath>, HgFileNodeId>, Error> {
    let mut moved_large_repo_entries = HashMap::new();
    for (path, filenode_id) in filenodes {
        if let Some(path) = path {
            let moved_path = mover(&path)?;
            if let Some(moved_path) = moved_path {
                moved_large_repo_entries.insert(Some(moved_path), filenode_id);
            }
        }
    }

    Ok(moved_large_repo_entries)
}

async fn get_synced_commit<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
    hash: ChangesetId,
) -> Result<ChangesetId, Error> {
    let maybe_sync_outcome = commit_syncer
        .get_commit_sync_outcome(ctx.clone(), hash)
        .await?;
    let sync_outcome = maybe_sync_outcome.ok_or(format_err!(
        "No sync outcome for {} in {:?}",
        hash,
        commit_syncer
    ))?;

    use CommitSyncOutcome::*;
    match sync_outcome {
        NotSyncCandidate => {
            return Err(format_err!("{} does not remap in small repo", hash).into());
        }
        RewrittenAs(cs_id) | EquivalentWorkingCopyAncestor(cs_id) => Ok(cs_id),
        Preserved => Ok(hash),
    }
}

async fn fetch_root_mf_id(
    ctx: CoreContext,
    repo: BlobRepo,
    cs_id: ChangesetId,
) -> Result<HgManifestId, Error> {
    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
        .compat()
        .await?;
    let changeset = repo
        .get_changeset_by_changesetid(ctx.clone(), hg_cs_id)
        .compat()
        .await?;
    Ok(changeset.manifestid())
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BookmarkDiff {
    InconsistentValue {
        small_bookmark: BookmarkName,
        expected_small_cs_id: ChangesetId,
        actual_small_cs_id: Option<ChangesetId>,
    },
    ShouldBeDeleted {
        small_bookmark: BookmarkName,
    },
}

impl BookmarkDiff {
    pub fn small_bookmark(&self) -> &BookmarkName {
        use BookmarkDiff::*;
        match self {
            InconsistentValue { small_bookmark, .. } => small_bookmark,
            ShouldBeDeleted { small_bookmark } => small_bookmark,
        }
    }
}

async fn rename_large_repo_bookmarks<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
    bookmark_renamer: &BookmarkRenamer,
    large_repo_bookmarks: impl IntoIterator<Item = (BookmarkName, ChangesetId)>,
) -> Result<HashMap<BookmarkName, ChangesetId>, Error> {
    let mut renamed_large_repo_bookmarks = vec![];
    for (bookmark, cs_id) in large_repo_bookmarks {
        if let Some(bookmark) = bookmark_renamer(&bookmark) {
            let maybe_sync_outcome = commit_syncer
                .get_commit_sync_outcome(ctx.clone(), cs_id)
                .map(move |maybe_sync_outcome| {
                    let maybe_sync_outcome = maybe_sync_outcome?;
                    use CommitSyncOutcome::*;
                    let remapped_cs_id = match maybe_sync_outcome {
                        Some(Preserved) => cs_id,
                        Some(RewrittenAs(cs_id)) | Some(EquivalentWorkingCopyAncestor(cs_id)) => {
                            cs_id
                        }
                        Some(NotSyncCandidate) => {
                            return Err(format_err!("{} is not a sync candidate", cs_id));
                        }
                        None => {
                            return Err(format_err!("{} is not remapped for {}", cs_id, bookmark));
                        }
                    };
                    Ok((bookmark, remapped_cs_id))
                })
                .boxed();
            renamed_large_repo_bookmarks.push(maybe_sync_outcome);
        }
    }

    let large_repo_bookmarks = new_stream::iter(renamed_large_repo_bookmarks)
        .buffer_unordered(100)
        .try_collect::<HashMap<_, _>>()
        .await?;

    Ok(large_repo_bookmarks)
}
