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

/// This function returns what bookmarks are different between a source repo and a target repo.
/// Note that this is not just a trivial comparison, because this function also remaps all the
/// commits and renames bookmarks appropriately e.g. bookmark 'book' in source repo
/// might be renamed to bookmark 'prefix/book' in target repo, and commit A to which bookmark 'book'
/// points can be remapped to commit B in the target repo.
///
///  Source repo                Target repo
///
///   A <- "book"      <----->    B <- "prefix/book"
///   |                           |
///  ...                         ...
///
pub async fn find_bookmark_diff<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
) -> Result<Vec<BookmarkDiff>, Error> {
    let source_repo = commit_syncer.get_source_repo();
    let target_repo = commit_syncer.get_target_repo();

    let target_bookmarks = target_repo
        .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
        .map(|(bookmark, cs_id)| (bookmark.name().clone(), cs_id))
        .collect_to::<HashMap<_, _>>()
        .compat()
        .await?;

    // 'renamed_source_bookmarks' - take all the source bookmarks, rename the bookmarks, remap
    // the commits.
    let renamed_source_bookmarks = {
        let source_bookmarks = source_repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .map(|(bookmark, cs_id)| (bookmark.name().clone(), cs_id))
            .collect()
            .compat()
            .await?;

        // Renames bookmarks and also maps large cs ids to small cs ids
        rename_and_remap_bookmarks(ctx.clone(), &commit_syncer, source_bookmarks).await?
    };

    let mut diff = vec![];
    for (target_book, target_cs_id) in &target_bookmarks {
        let actual_target_cs_id = renamed_source_bookmarks.get(target_book);
        let reverse_bookmark_renamer = commit_syncer.get_reverse_bookmark_renamer();
        if actual_target_cs_id.is_none() && reverse_bookmark_renamer(target_book).is_none() {
            // Note that the reverse_bookmark_renamer check below is necessary because there
            // might be bookmark in the source repo that shouldn't be present in the target repo
            // at all. Without reverse_bookmark_renamer it's not possible to distinguish "bookmark
            // that shouldn't be in the target repo" and "bookmark that should be in the target
            // repo but is missing".
            continue;
        }

        if actual_target_cs_id != Some(target_cs_id) {
            diff.push(BookmarkDiff::InconsistentValue {
                target_bookmark: target_book.clone(),
                expected_target_cs_id: target_cs_id.clone(),
                actual_target_cs_id: actual_target_cs_id.cloned(),
            });
        }
    }

    // find all bookmarks that exist in source repo, but don't exist in target repo
    for renamed_source_bookmark in renamed_source_bookmarks.keys() {
        if !target_bookmarks.contains_key(renamed_source_bookmark) {
            diff.push(BookmarkDiff::ShouldBeDeleted {
                target_bookmark: renamed_source_bookmark.clone(),
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
        target_bookmark: BookmarkName,
        expected_target_cs_id: ChangesetId,
        actual_target_cs_id: Option<ChangesetId>,
    },
    ShouldBeDeleted {
        target_bookmark: BookmarkName,
    },
}

impl BookmarkDiff {
    pub fn target_bookmark(&self) -> &BookmarkName {
        use BookmarkDiff::*;
        match self {
            InconsistentValue {
                target_bookmark, ..
            } => target_bookmark,
            ShouldBeDeleted { target_bookmark } => target_bookmark,
        }
    }
}

async fn rename_and_remap_bookmarks<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
    bookmarks: impl IntoIterator<Item = (BookmarkName, ChangesetId)>,
) -> Result<HashMap<BookmarkName, ChangesetId>, Error> {
    let bookmark_renamer = commit_syncer.get_bookmark_renamer();

    let mut renamed_and_remapped_bookmarks = vec![];
    for (bookmark, cs_id) in bookmarks {
        if let Some(renamed_bookmark) = bookmark_renamer(&bookmark) {
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
                    Ok((renamed_bookmark, remapped_cs_id))
                })
                .boxed();
            renamed_and_remapped_bookmarks.push(maybe_sync_outcome);
        }
    }

    new_stream::iter(renamed_and_remapped_bookmarks)
        .buffer_unordered(100)
        .try_collect::<HashMap<_, _>>()
        .await
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::CommitSyncRepos;
    use bookmark_renaming::BookmarkRenamer;
    use bookmarks::BookmarkName;
    use fbinit::FacebookInit;
    use fixtures::linear;
    use futures::stream::Stream;
    use metaconfig_types::CommitSyncDirection;
    use mononoke_types::{MPath, RepositoryId};
    use revset::AncestorsNodeStream;
    use sql_ext::SqlConstructors;
    use std::sync::Arc;
    // To support async tests
    use synced_commit_mapping::{SqlSyncedCommitMapping, SyncedCommitMappingEntry};
    use tests_utils::bookmark;
    use tokio_preview as tokio;

    fn identity_mover(v: &MPath) -> Result<Option<MPath>, Error> {
        Ok(Some(v.clone()))
    }

    fn get_small_to_large_renamer() -> BookmarkRenamer {
        Arc::new(|bookmark_name: &BookmarkName| -> Option<BookmarkName> {
            let master = BookmarkName::new("master").unwrap();
            if bookmark_name == &master {
                Some(master)
            } else {
                Some(BookmarkName::new(format!("prefix/{}", bookmark_name)).unwrap())
            }
        })
    }

    fn get_large_to_small_renamer() -> BookmarkRenamer {
        Arc::new(|bookmark_name: &BookmarkName| -> Option<BookmarkName> {
            let master = BookmarkName::new("master").unwrap();
            if bookmark_name == &master {
                Some(master)
            } else {
                let prefix = "prefix/";
                let name = format!("{}", bookmark_name);
                if name.starts_with(prefix) {
                    Some(BookmarkName::new(&name[prefix.len()..]).unwrap())
                } else {
                    None
                }
            }
        })
    }

    #[fbinit::test]
    async fn test_bookmark_diff_with_renamer(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let commit_syncer = init(
            fb,
            get_large_to_small_renamer(),
            get_small_to_large_renamer(),
            CommitSyncDirection::LargeToSmall,
        )
        .await?;

        let small_repo = commit_syncer.get_small_repo();
        let large_repo = commit_syncer.get_large_repo();

        let another_hash = "607314ef579bd2407752361ba1b0c1729d08b281";
        bookmark(&ctx, &small_repo, "newbook")
            .set_to(another_hash)
            .await?;
        bookmark(&ctx, &large_repo, "prefix/newbook")
            .set_to(another_hash)
            .await?;
        let actual_diff = find_bookmark_diff(ctx.clone(), &commit_syncer).await?;
        assert!(actual_diff.is_empty());

        bookmark(&ctx, &small_repo, "somebook")
            .set_to(another_hash)
            .await?;
        bookmark(&ctx, &large_repo, "somebook")
            .set_to(another_hash)
            .await?;

        let actual_diff = find_bookmark_diff(ctx.clone(), &commit_syncer).await?;
        assert!(!actual_diff.is_empty());

        Ok(())
    }

    #[fbinit::test]
    async fn test_bookmark_small_to_large(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let commit_syncer = init(
            fb,
            get_small_to_large_renamer(),
            get_large_to_small_renamer(),
            CommitSyncDirection::SmallToLarge,
        )
        .await?;

        let large_repo = commit_syncer.get_large_repo();

        // This bookmark is not present in the small repo, and it shouldn't be.
        // In that case
        bookmark(&ctx, &large_repo, "bookmarkfromanothersmallrepo")
            .set_to("master")
            .await?;

        let actual_diff = find_bookmark_diff(ctx.clone(), &commit_syncer).await?;
        assert_eq!(actual_diff, vec![]);
        Ok(())
    }

    async fn init(
        fb: FacebookInit,
        bookmark_renamer: BookmarkRenamer,
        reverse_bookmark_renamer: BookmarkRenamer,
        direction: CommitSyncDirection,
    ) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
        let ctx = CoreContext::test_mock(fb);
        let small_repo = linear::getrepo_with_id(fb, RepositoryId::new(0));
        let large_repo = linear::getrepo_with_id(fb, RepositoryId::new(1));

        let master = BookmarkName::new("master")?;
        let maybe_master_val = small_repo
            .get_bonsai_bookmark(ctx.clone(), &master)
            .compat()
            .await?;

        let master_val = maybe_master_val.ok_or(Error::msg("master not found"))?;
        let changesets =
            AncestorsNodeStream::new(ctx.clone(), &small_repo.get_changeset_fetcher(), master_val)
                .collect()
                .compat()
                .await?;

        let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();
        for cs_id in changesets {
            mapping
                .add(
                    ctx.clone(),
                    SyncedCommitMappingEntry {
                        large_repo_id: large_repo.get_repoid(),
                        small_repo_id: small_repo.get_repoid(),
                        small_bcs_id: cs_id,
                        large_bcs_id: cs_id,
                    },
                )
                .compat()
                .await?;
        }

        let repos = match direction {
            CommitSyncDirection::LargeToSmall => CommitSyncRepos::LargeToSmall {
                small_repo: small_repo.clone(),
                large_repo: large_repo.clone(),
                mover: Arc::new(identity_mover),
                bookmark_renamer,
                reverse_bookmark_renamer,
            },
            CommitSyncDirection::SmallToLarge => CommitSyncRepos::SmallToLarge {
                small_repo: small_repo.clone(),
                large_repo: large_repo.clone(),
                mover: Arc::new(identity_mover),
                bookmark_renamer,
                reverse_bookmark_renamer,
            },
        };

        Ok(CommitSyncer::new(mapping, repos))
    }
}
