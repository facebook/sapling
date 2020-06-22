/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use futures_ext::{BoxFuture, FutureExt, StreamExt};
use futures_old::{stream, Future, Stream};

use super::{CommitSyncOutcome, CommitSyncer};
use crate::types::{Source, Target};

use blobrepo::BlobRepo;
use blobrepo_hg::BlobRepoHg;
use blobstore::Loadable;
use bookmarks::BookmarkName;
use cloned::cloned;
use context::CoreContext;
use futures::{compat::Future01CompatExt, future::FutureExt as PreviewFutureExt};
use futures_util::{
    stream::{self as new_stream, StreamExt as NewStreamExt},
    try_join,
};
use manifest::{Entry, ManifestOps};
use mercurial_types::{FileType, HgFileNodeId, HgManifestId};
use mononoke_types::{ChangesetId, MPath};
use movers::Mover;
use ref_cast::RefCast;
use slog::{debug, error, info};
use std::collections::{HashMap, HashSet};
use std::fmt::Debug;
use synced_commit_mapping::SyncedCommitMapping;

pub type PathToFileNodeIdMapping = HashMap<MPath, (FileType, HgFileNodeId)>;
type SourceRepo = Source<BlobRepo>;
type TargetRepo = Target<BlobRepo>;

pub async fn verify_working_copy<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: CommitSyncer<M>,
    source_hash: ChangesetId,
) -> Result<(), Error> {
    let source_repo = commit_syncer.get_source_repo();
    let target_repo = commit_syncer.get_target_repo();

    let target_hash = get_synced_commit(ctx.clone(), &commit_syncer, source_hash).await?;
    info!(ctx.logger(), "target repo cs id: {}", target_hash);

    let moved_source_repo_entries = get_maybe_moved_filenode_ids(
        ctx.clone(),
        &source_repo,
        source_hash.clone(),
        if source_hash != target_hash {
            Some(commit_syncer.get_mover())
        } else {
            // No need to move any paths, because this commit was preserved as is
            None
        },
    );
    let target_repo_entries =
        get_maybe_moved_filenode_ids(ctx.clone(), &target_repo, target_hash.clone(), None);

    let (moved_source_repo_entries, target_repo_entries) =
        try_join!(moved_source_repo_entries, target_repo_entries)?;

    verify_filenode_mapping_equivalence(
        ctx,
        Source(source_hash),
        SourceRepo::ref_cast(source_repo),
        TargetRepo::ref_cast(target_repo),
        &Source(moved_source_repo_entries),
        &Target(target_repo_entries),
        commit_syncer.get_reverse_mover(),
    )
    .await
}

/// Given two `PathToFileNodeIdMapping`s, verify that they are
/// equivalent, save for paths rewritten into nothingness
/// by the `reverse_mover` (Note that the name `reverse_mover`
/// means that it moves paths from `target_repo` to `source_repo`)
async fn verify_filenode_mapping_equivalence<'a>(
    ctx: CoreContext,
    source_hash: Source<ChangesetId>,
    source_repo: &'a Source<BlobRepo>,
    target_repo: &'a Target<BlobRepo>,
    moved_source_repo_entries: &'a Source<PathToFileNodeIdMapping>,
    target_repo_entries: &'a Target<PathToFileNodeIdMapping>,
    reverse_mover: &'a Mover,
) -> Result<(), Error> {
    // If you are wondering, why the lifetime is needed,
    // in the function signature, see
    // https://github.com/rust-lang/rust/issues/63033
    compare_contents(
        ctx.clone(),
        (source_repo.clone(), moved_source_repo_entries),
        (target_repo.clone(), target_repo_entries),
        source_hash,
    )
    .await?;

    let mut extra_target_files_count = 0;
    for path in target_repo_entries.0.keys() {
        // "path" is not present in the source, however that might be expected - we use
        // reverse_mover to check that.
        if moved_source_repo_entries.0.get(&path).is_none() && reverse_mover(&path)?.is_some() {
            error!(
                ctx.logger(),
                "{:?} is present in {}, but not in {}",
                path,
                target_repo.0.name(),
                source_repo.0.name(),
            );
            extra_target_files_count += 1;
        }
    }

    if extra_target_files_count > 0 {
        return Err(format_err!(
            "{} files are present in {}, but not in {}",
            extra_target_files_count,
            target_repo.0.name(),
            source_repo.0.name(),
        ));
    }

    info!(ctx.logger(), "all is well!");
    Ok(())
}

/// Get all the file filenode ids for a given commit,
/// potentially applying a `Mover` to all file paths
pub async fn get_maybe_moved_filenode_ids(
    ctx: CoreContext,
    repo: &BlobRepo,
    hash: ChangesetId,
    maybe_mover: Option<&Mover>,
) -> Result<PathToFileNodeIdMapping, Error> {
    let root_mf_id = fetch_root_mf_id(ctx.clone(), repo, hash).await?;
    let repo_entries = list_all_filenode_ids(ctx, repo, root_mf_id)
        .compat()
        .await?;
    if let Some(mover) = maybe_mover {
        move_all_paths(&repo_entries, mover)
    } else {
        Ok(repo_entries)
    }
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
    let (renamed_source_bookmarks, no_sync_outcome) = {
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
        if no_sync_outcome.contains(&target_book) {
            diff.push(BookmarkDiff::NoSyncOutcome {
                target_bookmark: target_book.clone(),
            });
            continue;
        }
        let corresponding_changesets = renamed_source_bookmarks.get(target_book);
        let remapped_source_cs_id = corresponding_changesets.map(|cs| cs.target_cs_id);
        let reverse_bookmark_renamer = commit_syncer.get_reverse_bookmark_renamer();
        if remapped_source_cs_id.is_none() && reverse_bookmark_renamer(target_book).is_none() {
            // Note that the reverse_bookmark_renamer check below is necessary because there
            // might be bookmark in the source repo that shouldn't be present in the target repo
            // at all. Without reverse_bookmark_renamer it's not possible to distinguish "bookmark
            // that shouldn't be in the target repo" and "bookmark that should be in the target
            // repo but is missing".
            continue;
        }

        if remapped_source_cs_id != Some(*target_cs_id) {
            diff.push(BookmarkDiff::InconsistentValue {
                target_bookmark: target_book.clone(),
                target_cs_id: target_cs_id.clone(),
                source_cs_id: corresponding_changesets.map(|cs| cs.source_cs_id),
            });
        }
    }

    // find all bookmarks that exist in source repo, but don't exist in target repo
    for (renamed_source_bookmark, corresponding_changesets) in renamed_source_bookmarks {
        if !target_bookmarks.contains_key(&renamed_source_bookmark) {
            diff.push(BookmarkDiff::MissingInTarget {
                target_bookmark: renamed_source_bookmark.clone(),
                source_cs_id: corresponding_changesets.source_cs_id,
            });
        }
    }

    Ok(diff)
}

pub fn list_all_filenode_ids(
    ctx: CoreContext,
    repo: &BlobRepo,
    mf_id: HgManifestId,
) -> BoxFuture<PathToFileNodeIdMapping, Error> {
    let repoid = repo.get_repoid();
    info!(
        ctx.logger(),
        "fetching filenode ids for {:?} in {}", mf_id, repoid,
    );
    mf_id
        .list_all_entries(ctx.clone(), repo.get_blobstore())
        .filter_map(move |(path, entry)| match entry {
            Entry::Leaf(leaf_payload) => {
                match path {
                    Some(path) => Some((path, leaf_payload)),
                    None => {
                        // Leaf shouldn't normally be None
                        None
                    }
                }
            }
            Entry::Tree(_) => None,
        })
        .collect_to::<HashMap<_, _>>()
        .inspect(move |res| {
            debug!(
                ctx.logger(),
                "fetched {} filenode ids for {}",
                res.len(),
                repoid,
            );
        })
        .boxify()
}

async fn compare_contents(
    ctx: CoreContext,
    (source_repo, source_filenodes): (Source<BlobRepo>, &Source<PathToFileNodeIdMapping>),
    (target_repo, target_filenodes): (Target<BlobRepo>, &Target<PathToFileNodeIdMapping>),
    source_hash: Source<ChangesetId>,
) -> Result<(), Error> {
    // Both of these sets have three-element tuples as their elements:
    // `(MPath, SourceThing, TargetThing)`, where `Thing` is a `FileType`
    // or a `HgFileNodeId` for different sets
    let mut different_filenodes = HashSet::new();
    let mut different_filetypes = HashSet::new();
    for (path, (target_file_type, target_filenode_id)) in &target_filenodes.0 {
        let maybe_source_type_and_filenode_id = &source_filenodes.0.get(&path);
        let (maybe_source_file_type, maybe_source_filenode_id) =
            match maybe_source_type_and_filenode_id {
                Some((source_file_type, source_filenode_id)) => {
                    (Some(source_file_type), Some(source_filenode_id))
                }
                None => (None, None),
            };

        if maybe_source_filenode_id != Some(&target_filenode_id) {
            match maybe_source_filenode_id {
                Some(source_filenode_id) => {
                    different_filenodes.insert((
                        path.clone(),
                        Source(*source_filenode_id),
                        Target(*target_filenode_id),
                    ));
                }
                None => {
                    return Err(format_err!(
                        "{:?} exists in {} but not in {}",
                        path,
                        target_repo.0.name(),
                        source_repo.0.name(),
                    ));
                }
            }
        }

        if maybe_source_file_type != Some(&target_file_type) {
            match maybe_source_file_type {
                Some(source_file_type) => {
                    different_filetypes.insert((
                        path.clone(),
                        Source(*source_file_type),
                        Target(*target_file_type),
                    ));
                }
                None => {
                    // This should really be unreachable, as we should've early
                    // exited on the previous iteration
                    return Err(format_err!(
                        "{:?} exists in {} but not in {}",
                        path,
                        target_repo.0.name(),
                        source_repo.0.name(),
                    ));
                }
            };
        }
    }

    report_different(
        &ctx,
        different_filetypes,
        &source_hash,
        "filetype",
        Source(source_repo.0.name()),
        Target(target_repo.0.name()),
    )?;

    info!(
        ctx.logger(),
        "found {} filenodes that are different, checking content...",
        different_filenodes.len(),
    );

    verify_filenodes_have_same_contents(
        &ctx,
        &target_repo,
        &source_repo,
        &source_hash,
        different_filenodes,
    )
    .await
}

pub async fn verify_filenodes_have_same_contents<
    // item is a tuple: (MPath, large filenode id, small filenode id)
    I: IntoIterator<Item = (MPath, Source<HgFileNodeId>, Target<HgFileNodeId>)>,
>(
    ctx: &CoreContext,
    target_repo: &Target<BlobRepo>,
    source_repo: &Source<BlobRepo>,
    source_hash: &Source<ChangesetId>,
    should_be_equivalent: I,
) -> Result<(), Error> {
    let fetched_content_ids = stream::iter_ok(should_be_equivalent)
        .map({
            cloned!(ctx, target_repo, source_repo);
            move |(path, source_filenode_id, target_filenode_id)| {
                debug!(
                    ctx.logger(),
                    "checking content for different filenodes: source {} vs target {}",
                    source_filenode_id,
                    target_filenode_id,
                );
                let f1 = source_filenode_id
                    .0
                    .load(ctx.clone(), source_repo.0.blobstore())
                    .map(|e| Source(e.content_id()));
                let f2 = target_filenode_id
                    .0
                    .load(ctx.clone(), target_repo.0.blobstore())
                    .map(|e| Target(e.content_id()));

                f1.join(f2).map(move |(c1, c2)| (path, c1, c2))
            }
        })
        .buffered(1000)
        .collect()
        .compat()
        .await?;

    let different_contents: Vec<_> = fetched_content_ids
        .into_iter()
        .filter(|(_mpath, c1, c2)| c1.0 != c2.0)
        .collect();

    report_different(
        ctx,
        different_contents,
        source_hash,
        "contents",
        Source(source_repo.0.name()),
        Target(target_repo.0.name()),
    )
}

/// Given a list of differences of a given type (`T`)
/// report them in the logs and return an appropriate result
pub fn report_different<
    T: Debug,
    E: ExactSizeIterator<Item = (MPath, Source<T>, Target<T>)>,
    I: IntoIterator<IntoIter = E, Item = <E as Iterator>::Item>,
>(
    ctx: &CoreContext,
    different_things: I,
    source_hash: &Source<ChangesetId>,
    name: &str,
    source_repo_name: Source<&String>,
    target_repo_name: Target<&String>,
) -> Result<(), Error> {
    let mut different_things = different_things.into_iter();
    let len = different_things.len();
    if len > 0 {
        // The very first value is preserved for error formatting
        let (mpath, source_thing, target_thing) = match different_things.next() {
            None => unreachable!("length of iterator is guaranteed to be >0"),
            Some((mpath, source_thing, target_thing)) => (mpath, source_thing, target_thing),
        };

        // And we also want a debug print of it
        debug!(
            ctx.logger(),
            "Different {} for path {:?}: {}: {:?} {}: {:?}",
            name,
            mpath,
            source_repo_name,
            source_thing,
            target_repo_name,
            target_thing
        );

        let mut rest_of_different_things = different_things.take(9);
        while let Some((mpath, source_thing, target_thing)) = rest_of_different_things.next() {
            debug!(
                ctx.logger(),
                "Different {} for path {:?}: {}: {:?} {}: {:?}",
                name,
                mpath,
                source_repo_name,
                source_thing,
                target_repo_name,
                target_thing
            );
        }

        Err(format_err!(
            "Found {} files with different {} in {} cs {} (example: {:?})",
            len,
            name,
            source_repo_name,
            source_hash,
            (mpath, source_thing, target_thing),
        ))
    } else {
        Ok(())
    }
}

pub fn move_all_paths(
    filenodes: &PathToFileNodeIdMapping,
    mover: &Mover,
) -> Result<PathToFileNodeIdMapping, Error> {
    let mut moved_large_repo_entries = HashMap::new();
    for (path, filenode_id) in filenodes {
        let moved_path = mover(&path)?;
        if let Some(moved_path) = moved_path {
            moved_large_repo_entries.insert(moved_path, filenode_id.clone());
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

pub async fn fetch_root_mf_id(
    ctx: CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
) -> Result<HgManifestId, Error> {
    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), cs_id)
        .compat()
        .await?;
    let changeset = hg_cs_id.load(ctx, repo.blobstore()).compat().await?;
    Ok(changeset.manifestid())
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum BookmarkDiff {
    InconsistentValue {
        target_bookmark: BookmarkName,
        target_cs_id: ChangesetId,
        source_cs_id: Option<ChangesetId>,
    },
    MissingInTarget {
        target_bookmark: BookmarkName,
        source_cs_id: ChangesetId,
    },
    NoSyncOutcome {
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
            MissingInTarget {
                target_bookmark, ..
            } => target_bookmark,
            NoSyncOutcome { target_bookmark } => target_bookmark,
        }
    }
}

struct CorrespondingChangesets {
    source_cs_id: ChangesetId,
    target_cs_id: ChangesetId,
}

async fn rename_and_remap_bookmarks<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
    bookmarks: impl IntoIterator<Item = (BookmarkName, ChangesetId)>,
) -> Result<
    (
        HashMap<BookmarkName, CorrespondingChangesets>,
        HashSet<BookmarkName>,
    ),
    Error,
> {
    let bookmark_renamer = commit_syncer.get_bookmark_renamer();

    let mut renamed_and_remapped_bookmarks = vec![];
    for (bookmark, cs_id) in bookmarks {
        if let Some(renamed_bookmark) = bookmark_renamer(&bookmark) {
            let maybe_sync_outcome = commit_syncer
                .get_commit_sync_outcome(ctx.clone(), cs_id)
                .map(move |maybe_sync_outcome| {
                    let maybe_sync_outcome = maybe_sync_outcome?;
                    use CommitSyncOutcome::*;
                    let maybe_remapped_cs_id = match maybe_sync_outcome {
                        Some(Preserved) => Some(cs_id),
                        Some(RewrittenAs(cs_id)) | Some(EquivalentWorkingCopyAncestor(cs_id)) => {
                            Some(cs_id)
                        }
                        Some(NotSyncCandidate) => {
                            return Err(format_err!("{} is not a sync candidate", cs_id));
                        }
                        None => None,
                    };
                    let maybe_corresponding_changesets =
                        maybe_remapped_cs_id.map(|target_cs_id| CorrespondingChangesets {
                            source_cs_id: cs_id,
                            target_cs_id,
                        });
                    Ok((renamed_bookmark, maybe_corresponding_changesets))
                })
                .boxed();
            renamed_and_remapped_bookmarks.push(maybe_sync_outcome);
        }
    }

    let mut s = new_stream::iter(renamed_and_remapped_bookmarks).buffer_unordered(100);
    let mut remapped_bookmarks = HashMap::new();
    let mut no_sync_outcome = HashSet::new();

    while let Some(item) = s.next().await {
        let (renamed_bookmark, maybe_corresponding_changesets) = item?;
        match maybe_corresponding_changesets {
            Some(corresponding_changesets) => {
                remapped_bookmarks.insert(renamed_bookmark, corresponding_changesets);
            }
            None => {
                no_sync_outcome.insert(renamed_bookmark);
            }
        }
    }

    Ok((remapped_bookmarks, no_sync_outcome))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::CommitSyncRepos;
    use bookmark_renaming::BookmarkRenamer;
    use bookmarks::BookmarkName;
    use fbinit::FacebookInit;
    use fixtures::linear;
    use futures_old::stream::Stream;
    use metaconfig_types::CommitSyncDirection;
    use mononoke_types::{MPath, RepositoryId};
    use revset::AncestorsNodeStream;
    use sql_construct::SqlConstruct;
    use std::sync::Arc;
    // To support async tests
    use synced_commit_mapping::{SqlSyncedCommitMapping, SyncedCommitMappingEntry};
    use tests_utils::{bookmark, CreateCommitContext};

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
    fn test_bookmark_diff_with_renamer(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new()?;
        runtime.block_on_std(test_bookmark_diff_with_renamer_impl(fb))
    }

    async fn test_bookmark_diff_with_renamer_impl(fb: FacebookInit) -> Result<(), Error> {
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
    fn test_bookmark_small_to_large(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new()?;
        runtime.block_on_std(test_bookmark_small_to_large_impl(fb))
    }

    async fn test_bookmark_small_to_large_impl(fb: FacebookInit) -> Result<(), Error> {
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

    #[fbinit::test]
    fn test_bookmark_no_sync_outcome(fb: FacebookInit) -> Result<(), Error> {
        let mut runtime = tokio_compat::runtime::Runtime::new()?;
        runtime.block_on_std(test_bookmark_no_sync_outcome_impl(fb))
    }

    async fn test_bookmark_no_sync_outcome_impl(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let commit_syncer = init(
            fb,
            get_small_to_large_renamer(),
            get_large_to_small_renamer(),
            CommitSyncDirection::LargeToSmall,
        )
        .await?;

        let large_repo = commit_syncer.get_large_repo();

        let commit = CreateCommitContext::new(&ctx, &large_repo, vec!["master"])
            .add_file("somefile", "ololo")
            .commit()
            .await?;
        // This bookmark is not present in the small repo, and it shouldn't be.
        // In that case
        bookmark(&ctx, &large_repo, "master").set_to(commit).await?;

        let actual_diff = find_bookmark_diff(ctx.clone(), &commit_syncer).await?;
        assert_eq!(
            actual_diff,
            vec![BookmarkDiff::NoSyncOutcome {
                target_bookmark: BookmarkName::new("master")?,
            }]
        );
        Ok(())
    }

    async fn init(
        fb: FacebookInit,
        bookmark_renamer: BookmarkRenamer,
        reverse_bookmark_renamer: BookmarkRenamer,
        direction: CommitSyncDirection,
    ) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
        let ctx = CoreContext::test_mock(fb);
        let small_repo = linear::getrepo_with_id(fb, RepositoryId::new(0)).await;
        let large_repo = linear::getrepo_with_id(fb, RepositoryId::new(1)).await;

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
                        version_name: None,
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
                reverse_mover: Arc::new(identity_mover),
                bookmark_renamer,
                reverse_bookmark_renamer,
                version_name: "TEST_VERSION_NAME".to_string(),
            },
            CommitSyncDirection::SmallToLarge => CommitSyncRepos::SmallToLarge {
                small_repo: small_repo.clone(),
                large_repo: large_repo.clone(),
                mover: Arc::new(identity_mover),
                reverse_mover: Arc::new(identity_mover),
                bookmark_renamer,
                reverse_bookmark_renamer,
                version_name: "TEST_VERSION_NAME".to_string(),
            },
        };

        Ok(CommitSyncer::new(mapping, repos))
    }
}
