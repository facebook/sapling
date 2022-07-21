/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::format_err;
use anyhow::Error;

use super::CommitSyncConfigVersion;
use super::CommitSyncOutcome;
use super::CommitSyncer;
use crate::types::Source;
use crate::types::Target;

use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use context::CoreContext;
use derived_data::BonsaiDerived;
use fsnodes::RootFsnodeId;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::try_join;
use futures::TryStreamExt;
use live_commit_sync_config::LiveCommitSyncConfig;
use manifest::ManifestOps;
use mercurial_types::FileType;
use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::MPath;
use movers::Mover;
use ref_cast::RefCast;
use slog::debug;
use slog::error;
use slog::info;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Debug;
use std::sync::Arc;
use synced_commit_mapping::SyncedCommitMapping;

type SourceRepo = Source<BlobRepo>;
type TargetRepo = Target<BlobRepo>;

pub async fn verify_working_copy<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: CommitSyncer<M>,
    source_hash: ChangesetId,
) -> Result<(), Error> {
    let source_repo = commit_syncer.get_source_repo();
    let target_repo = commit_syncer.get_target_repo();

    let (target_hash, version) =
        get_synced_commit(ctx.clone(), &commit_syncer, source_hash).await?;

    info!(
        ctx.logger(),
        "target repo cs id: {}, mapping version: {}", target_hash, version
    );

    verify_working_copy_inner(
        &ctx,
        Source::ref_cast(source_repo),
        Target::ref_cast(target_repo),
        Source(source_hash),
        Target(target_hash),
        &commit_syncer.get_mover_by_version(&version).await?,
        &commit_syncer.get_reverse_mover_by_version(&version).await?,
        PrefixesToVisit::default(),
    )
    .await
}

// This method uses CommitSyncConfig to minimize the number of manifest traversals.
pub async fn verify_working_copy_fast_path<'a, M: SyncedCommitMapping + Clone + 'static>(
    ctx: &'a CoreContext,
    commit_syncer: &'a CommitSyncer<M>,
    source_hash: ChangesetId,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<(), Error> {
    let source_repo = commit_syncer.get_source_repo();
    let target_repo = commit_syncer.get_target_repo();

    let (target_hash, version) = get_synced_commit(ctx.clone(), commit_syncer, source_hash).await?;

    info!(
        ctx.logger(),
        "target repo cs id: {}, mapping version: {}", target_hash, version
    );

    let prefixes_to_visit = get_fast_path_prefixes(
        source_repo,
        commit_syncer,
        &version,
        live_commit_sync_config,
    )
    .await?;

    verify_working_copy_inner(
        ctx,
        Source::ref_cast(source_repo),
        Target::ref_cast(target_repo),
        Source(source_hash),
        Target(target_hash),
        &commit_syncer.get_mover_by_version(&version).await?,
        &commit_syncer.get_reverse_mover_by_version(&version).await?,
        prefixes_to_visit,
    )
    .await
}

pub async fn verify_working_copy_with_version_fast_path<
    'a,
    M: SyncedCommitMapping + Clone + 'static,
>(
    ctx: &'a CoreContext,
    commit_syncer: &'a CommitSyncer<M>,
    source_hash: Source<ChangesetId>,
    target_hash: Target<ChangesetId>,
    version: &'a CommitSyncConfigVersion,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<(), Error> {
    let source_repo = commit_syncer.get_source_repo();
    let target_repo = commit_syncer.get_target_repo();

    let prefixes_to_visit =
        get_fast_path_prefixes(source_repo, commit_syncer, version, live_commit_sync_config)
            .await?;

    verify_working_copy_inner(
        ctx,
        Source::ref_cast(source_repo),
        Target::ref_cast(target_repo),
        source_hash,
        target_hash,
        &commit_syncer.get_mover_by_version(version).await?,
        &commit_syncer.get_reverse_mover_by_version(version).await?,
        prefixes_to_visit,
    )
    .await
}

// Returns list of prefixes that need to be visited in both large and small
// repositories to establish working copy equivalence.
async fn get_fast_path_prefixes<'a, M: SyncedCommitMapping + Clone + 'static>(
    source_repo: &'a BlobRepo,
    commit_syncer: &'a CommitSyncer<M>,
    version: &'a CommitSyncConfigVersion,
    live_commit_sync_config: Arc<dyn LiveCommitSyncConfig>,
) -> Result<PrefixesToVisit, Error> {
    let small_repo_id = commit_syncer.get_small_repo().get_repoid();
    let config = live_commit_sync_config
        .get_commit_sync_config_by_version(source_repo.get_repoid(), version)
        .await?;

    let small_repo_config = config.small_repos.get(&small_repo_id).ok_or_else(|| {
        format_err!(
            "cannot find small repo id {} in commit sync config for {}",
            small_repo_id,
            version
        )
    })?;

    match &small_repo_config.default_action {
        DefaultSmallToLargeCommitSyncPathAction::Preserve => Ok(PrefixesToVisit::default()),
        DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(prefix) => {
            // Gets a list of large repo paths that small repo paths can map to.
            // All other large repo paths don't need visiting.
            let mut prefixes_to_visit = small_repo_config.map.values().cloned().collect::<Vec<_>>();
            prefixes_to_visit.push(prefix.clone());
            if small_repo_id == source_repo.get_repoid() {
                Ok(PrefixesToVisit {
                    source_prefixes_to_visit: None,
                    target_prefixes_to_visit: Some(prefixes_to_visit),
                })
            } else {
                Ok(PrefixesToVisit {
                    source_prefixes_to_visit: Some(prefixes_to_visit),
                    target_prefixes_to_visit: None,
                })
            }
        }
    }
}

#[derive(Default)]
pub struct PrefixesToVisit {
    source_prefixes_to_visit: Option<Vec<MPath>>,
    target_prefixes_to_visit: Option<Vec<MPath>>,
}

pub async fn verify_working_copy_inner<'a>(
    ctx: &'a CoreContext,
    source_repo: &'a SourceRepo,
    target_repo: &'a TargetRepo,
    source_hash: Source<ChangesetId>,
    target_hash: Target<ChangesetId>,
    mover: &Mover,
    reverse_mover: &Mover,
    prefixes_to_visit: PrefixesToVisit,
) -> Result<(), Error> {
    let PrefixesToVisit {
        source_prefixes_to_visit,
        target_prefixes_to_visit,
    } = prefixes_to_visit;

    let moved_source_repo_entries = get_maybe_moved_contents_and_types(
        ctx,
        source_repo,
        *source_hash,
        if *source_hash != *target_hash {
            Some(GetMaybeMovedFilenodesPolicy::ActuallyMove(mover))
        } else {
            // No need to move any paths, because this commit was preserved as is
            None
        },
        source_prefixes_to_visit,
    );
    let target_repo_entries = get_maybe_moved_contents_and_types(
        ctx,
        target_repo,
        *target_hash,
        Some(GetMaybeMovedFilenodesPolicy::CheckThatRewritesIntoSomeButDontMove(reverse_mover)),
        target_prefixes_to_visit,
    );

    let (moved_source_repo_entries, target_repo_entries) =
        try_join!(moved_source_repo_entries, target_repo_entries)?;

    verify_type_content_mapping_equivalence(
        ctx.clone(),
        source_hash,
        source_repo,
        target_repo,
        &Source(moved_source_repo_entries),
        &Target(target_repo_entries),
        reverse_mover,
    )
    .await
}

/// Given two maps of paths to (type, contentid), verify that they are
/// equivalent, save for paths rewritten into nothingness
/// by the `reverse_mover` (Note that the name `reverse_mover`
/// means that it moves paths from `target_repo` to `source_repo`)
async fn verify_type_content_mapping_equivalence<'a>(
    ctx: CoreContext,
    source_hash: Source<ChangesetId>,
    source_repo: &'a Source<BlobRepo>,
    target_repo: &'a Target<BlobRepo>,
    moved_source_repo_entries: &'a Source<HashMap<MPath, (FileType, ContentId)>>,
    target_repo_entries: &'a Target<HashMap<MPath, (FileType, ContentId)>>,
    reverse_mover: &'a Mover,
) -> Result<(), Error> {
    info!(
        ctx.logger(),
        "{} moved source entries, {} target entries",
        moved_source_repo_entries.len(),
        target_repo_entries.len()
    );
    // If you are wondering, why the lifetime is needed,
    // in the function signature, see
    // https://github.com/rust-lang/rust/issues/63033
    compare_contents_and_types(
        ctx.clone(),
        (source_repo.clone(), moved_source_repo_entries),
        (target_repo.clone(), target_repo_entries),
        source_hash,
    )
    .await?;

    let mut extra_source_files_count = 0;
    for path in moved_source_repo_entries.0.keys() {
        if target_repo_entries.0.get(path).is_none() {
            error!(
                ctx.logger(),
                "{:?} is present in {}, but not in {}",
                path,
                source_repo.0.name(),
                target_repo.0.name(),
            );
            extra_source_files_count += 1;
        }
    }
    if extra_source_files_count > 0 {
        return Err(format_err!(
            "{} files are present in {}, but not in {}",
            extra_source_files_count,
            source_repo.0.name(),
            target_repo.0.name(),
        ));
    }

    let mut extra_target_files_count = 0;
    for path in target_repo_entries.0.keys() {
        // "path" is not present in the source, however that might be expected - we use
        // reverse_mover to check that.
        if moved_source_repo_entries.0.get(path).is_none() && reverse_mover(path)?.is_some() {
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

/// Whether to move paths or just check that they don't disappear
enum GetMaybeMovedFilenodesPolicy<'a> {
    /// Actually apply the provided mover to the paths
    ActuallyMove(&'a Mover),
    /// Only check that the provided mover does not rewrite
    /// the paths into None
    CheckThatRewritesIntoSomeButDontMove(&'a Mover),
}

// Get all the file content and types for a given commit,
/// potentially applying a `Mover` to all file paths
async fn get_maybe_moved_contents_and_types<'a>(
    ctx: &'a CoreContext,
    repo: &'a BlobRepo,
    hash: ChangesetId,
    maybe_mover_policy: Option<GetMaybeMovedFilenodesPolicy<'a>>,
    prefixes: Option<Vec<MPath>>,
) -> Result<HashMap<MPath, (FileType, ContentId)>, Error> {
    let content_ids_and_types = list_content_ids_and_types(ctx, repo, hash, prefixes).await?;

    match maybe_mover_policy {
        None => Ok(content_ids_and_types),
        Some(GetMaybeMovedFilenodesPolicy::ActuallyMove(mover)) => {
            move_all_paths(&content_ids_and_types, mover)
        }
        Some(GetMaybeMovedFilenodesPolicy::CheckThatRewritesIntoSomeButDontMove(mover)) => {
            keep_movable_paths(&content_ids_and_types, mover)
        }
    }
}

/// This function returns what bookmarks are different between a source repo and a target repo.
/// Note that this is not just a trivial comparison, because this function also remaps all the
/// commits and renames bookmarks appropriately e.g. bookmark 'book' in source repo
/// might be renamed to bookmark 'prefix/book' in target repo, and commit A to which bookmark 'book'
/// points can be remapped to commit B in the target repo.
///
/// ```text
///  Source repo                Target repo
///
///   A <- "book"      <----->    B <- "prefix/book"
///   |                           |
///  ...                         ...
/// ```
pub async fn find_bookmark_diff<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
) -> Result<Vec<BookmarkDiff>, Error> {
    let source_repo = commit_syncer.get_source_repo();
    let target_repo = commit_syncer.get_target_repo();

    let target_bookmarks = target_repo
        .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
        .map_ok(|(bookmark, cs_id)| (bookmark.name().clone(), cs_id))
        .try_collect::<HashMap<_, _>>()
        .await?;

    // 'renamed_source_bookmarks' - take all the source bookmarks, rename the bookmarks, remap
    // the commits.
    let (renamed_source_bookmarks, no_sync_outcome) = {
        let source_bookmarks: Vec<_> = source_repo
            .get_bonsai_publishing_bookmarks_maybe_stale(ctx.clone())
            .map_ok(|(bookmark, cs_id)| (bookmark.name().clone(), cs_id))
            .try_collect()
            .await?;

        // Renames bookmarks and also maps large cs ids to small cs ids
        rename_and_remap_bookmarks(ctx.clone(), commit_syncer, source_bookmarks).await?
    };

    let reverse_bookmark_renamer = commit_syncer.get_reverse_bookmark_renamer().await?;
    let mut diff = vec![];
    for (target_book, target_cs_id) in &target_bookmarks {
        if no_sync_outcome.contains(target_book) {
            diff.push(BookmarkDiff::NoSyncOutcome {
                target_bookmark: target_book.clone(),
            });
            continue;
        }
        let corresponding_changesets = renamed_source_bookmarks.get(target_book);
        let remapped_source_cs_id = corresponding_changesets.map(|cs| cs.target_cs_id);
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

async fn list_content_ids_and_types(
    ctx: &CoreContext,
    repo: &BlobRepo,
    cs_id: ChangesetId,
    prefixes: Option<Vec<MPath>>,
) -> Result<HashMap<MPath, (FileType, ContentId)>, Error> {
    info!(
        ctx.logger(),
        "fetching content ids and types for {} in {}",
        cs_id,
        repo.name(),
    );

    let root_fsnode_id = RootFsnodeId::derive(ctx, repo, cs_id).await?;
    let root_fsnode_id = root_fsnode_id.fsnode_id();
    let s = match prefixes {
        Some(prefixes) => root_fsnode_id
            .list_leaf_entries_under(ctx.clone(), repo.get_blobstore(), prefixes)
            .left_stream(),
        None => root_fsnode_id
            .list_leaf_entries(ctx.clone(), repo.get_blobstore())
            .right_stream(),
    };
    let content_ids_and_types = s
        .map_ok(|(path, fsnode)| (path, (*fsnode.file_type(), *fsnode.content_id())))
        .try_collect::<HashMap<_, _>>()
        .await?;
    Ok(content_ids_and_types)
}

async fn compare_contents_and_types(
    ctx: CoreContext,
    (source_repo, source_types_and_content_ids): (
        Source<BlobRepo>,
        &Source<HashMap<MPath, (FileType, ContentId)>>,
    ),
    (target_repo, target_types_and_content_ids): (
        Target<BlobRepo>,
        &Target<HashMap<MPath, (FileType, ContentId)>>,
    ),
    source_hash: Source<ChangesetId>,
) -> Result<(), Error> {
    // Both of these sets have three-element tuples as their elements:
    // `(MPath, SourceThing, TargetThing)`, where `Thing` is a `FileType`
    // or a `ContentId` for different sets
    let mut different_content_ids = HashSet::new();
    let mut different_filetypes = HashSet::new();
    let mut exists_in_target_but_not_source = HashSet::new();
    for (path, (target_file_type, target_content_id)) in &target_types_and_content_ids.0 {
        let maybe_source_type_and_content_id = &source_types_and_content_ids.0.get(path);
        let (maybe_source_file_type, maybe_source_content_id) =
            match maybe_source_type_and_content_id {
                Some((source_file_type, source_content_id)) => {
                    (Some(source_file_type), Some(source_content_id))
                }
                None => (None, None),
            };

        if maybe_source_content_id != Some(target_content_id) {
            match maybe_source_content_id {
                Some(source_content_id) => {
                    different_content_ids.insert((
                        path.clone(),
                        Source(*source_content_id),
                        Target(*target_content_id),
                    ));
                }
                None => {
                    exists_in_target_but_not_source.insert(path);
                }
            }
        }

        if maybe_source_file_type != Some(target_file_type) {
            match maybe_source_file_type {
                Some(source_file_type) => {
                    different_filetypes.insert((
                        path.clone(),
                        Source(*source_file_type),
                        Target(*target_file_type),
                    ));
                }
                None => {
                    exists_in_target_but_not_source.insert(path);
                }
            };
        }
    }

    if !exists_in_target_but_not_source.is_empty() {
        for path in &exists_in_target_but_not_source {
            debug!(
                ctx.logger(),
                "{:?} exists in {} but not in {}",
                path,
                target_repo.0.name(),
                source_repo.0.name(),
            )
        }
        info!(
            ctx.logger(),
            "{} files exist in {} but not in {}",
            exists_in_target_but_not_source.len(),
            target_repo.0.name(),
            source_repo.0.name(),
        );
        let path = exists_in_target_but_not_source
            .into_iter()
            .next()
            .expect("just checked that the set wasn't empty");

        return Err(format_err!(
            "{:?} exists in {} but not in {}",
            path,
            target_repo.0.name(),
            source_repo.0.name(),
        ));
    }

    report_different(
        &ctx,
        different_filetypes,
        &source_hash,
        "filetype",
        Source(source_repo.0.name()),
        Target(target_repo.0.name()),
    )?;

    report_different(
        &ctx,
        different_content_ids,
        &source_hash,
        "contents",
        Source(source_repo.0.name()),
        Target(target_repo.0.name()),
    )?;

    Ok(())
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

        for (mpath, source_thing, target_thing) in different_things {
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

pub fn move_all_paths<V: Clone>(
    path_to_values: &HashMap<MPath, V>,
    mover: &Mover,
) -> Result<HashMap<MPath, V>, Error> {
    let mut moved_entries = HashMap::new();
    for (path, value) in path_to_values {
        let moved_path = mover(path)?;
        if let Some(moved_path) = moved_path {
            moved_entries.insert(moved_path, value.clone());
        }
    }

    Ok(moved_entries)
}

// Drop all paths which `mover` rewrites into `None`
fn keep_movable_paths<V: Clone>(
    path_to_values: &HashMap<MPath, V>,
    mover: &Mover,
) -> Result<HashMap<MPath, V>, Error> {
    let mut res = HashMap::new();
    for (path, value) in path_to_values {
        if mover(path)?.is_some() {
            res.insert(path.clone(), value.clone());
        }
    }

    Ok(res)
}

async fn get_synced_commit<M: SyncedCommitMapping + Clone + 'static>(
    ctx: CoreContext,
    commit_syncer: &CommitSyncer<M>,
    hash: ChangesetId,
) -> Result<(ChangesetId, CommitSyncConfigVersion), Error> {
    let maybe_sync_outcome = commit_syncer.get_commit_sync_outcome(&ctx, hash).await?;
    let sync_outcome = maybe_sync_outcome
        .ok_or_else(|| format_err!("No sync outcome for {} in {:?}", hash, commit_syncer))?;

    use CommitSyncOutcome::*;
    match sync_outcome {
        NotSyncCandidate(_) => {
            return Err(format_err!("{} does not remap in small repo", hash));
        }
        RewrittenAs(cs_id, mapping_version)
        | EquivalentWorkingCopyAncestor(cs_id, mapping_version) => Ok((cs_id, mapping_version)),
    }
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
    let bookmark_renamer = commit_syncer.get_bookmark_renamer().await?;

    let mut renamed_and_remapped_bookmarks = vec![];
    for (bookmark, cs_id) in bookmarks {
        if let Some(renamed_bookmark) = bookmark_renamer(&bookmark) {
            let maybe_sync_outcome = commit_syncer
                .get_commit_sync_outcome(&ctx, cs_id)
                .map(move |maybe_sync_outcome| {
                    let maybe_sync_outcome = maybe_sync_outcome?;
                    use CommitSyncOutcome::*;
                    let maybe_remapped_cs_id = match maybe_sync_outcome {
                        Some(RewrittenAs(cs_id, _))
                        | Some(EquivalentWorkingCopyAncestor(cs_id, _)) => Some(cs_id),
                        Some(NotSyncCandidate(_)) => {
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

    let mut s = stream::iter(renamed_and_remapped_bookmarks).buffer_unordered(100);
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
    use crate::CommitSyncDataProvider;
    use crate::CommitSyncRepos;
    use ascii::AsciiString;
    use bookmarks::BookmarkName;
    use fbinit::FacebookInit;
    use fixtures::Linear;
    use fixtures::TestRepoFixture;
    use futures::compat::Future01CompatExt;
    use futures_old::stream::Stream;
    use live_commit_sync_config::TestLiveCommitSyncConfig;
    use maplit::hashmap;
    use metaconfig_types::CommitSyncConfig;
    use metaconfig_types::CommitSyncConfigVersion;
    use metaconfig_types::CommitSyncDirection;
    use metaconfig_types::CommonCommitSyncConfig;
    use metaconfig_types::SmallRepoCommitSyncConfig;
    use metaconfig_types::SmallRepoPermanentConfig;
    use mononoke_types::MPath;
    use mononoke_types::RepositoryId;
    use revset::AncestorsNodeStream;
    use sql_construct::SqlConstruct;
    use std::str::FromStr;
    use std::sync::Arc;
    // To support async tests
    use cross_repo_sync_test_utils::get_live_commit_sync_config;
    use synced_commit_mapping::SqlSyncedCommitMapping;
    use synced_commit_mapping::SyncedCommitMappingEntry;
    use test_repo_factory::TestRepoFactory;
    use tests_utils::bookmark;
    use tests_utils::CreateCommitContext;

    #[fbinit::test]
    fn test_bookmark_diff_with_renamer(fb: FacebookInit) -> Result<(), Error> {
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_bookmark_diff_with_renamer_impl(fb))
    }

    async fn test_bookmark_diff_with_renamer_impl(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let commit_syncer = init(fb, CommitSyncDirection::LargeToSmall).await?;

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
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_bookmark_small_to_large_impl(fb))
    }

    async fn test_bookmark_small_to_large_impl(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let commit_syncer = init(fb, CommitSyncDirection::SmallToLarge).await?;

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
        let runtime = tokio::runtime::Runtime::new()?;
        runtime.block_on(test_bookmark_no_sync_outcome_impl(fb))
    }

    async fn test_bookmark_no_sync_outcome_impl(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let commit_syncer = init(fb, CommitSyncDirection::LargeToSmall).await?;

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

    #[fbinit::test]
    async fn test_verify_working_copy(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let source = test_repo_factory::build_empty(fb)?;
        let source_cs_id = CreateCommitContext::new_root(&ctx, &source)
            .add_file("prefix/file1", "1")
            .add_file("prefix/file2", "2")
            .commit()
            .await?;

        let target = test_repo_factory::build_empty(fb)?;
        let target_cs_id = CreateCommitContext::new_root(&ctx, &target)
            .add_file("file1", "1")
            .commit()
            .await?;

        // Source is a large repo, hence reverse the movers
        let mover: Mover = Arc::new(reverse_prefix_mover);
        let reverse_mover: Mover = Arc::new(prefix_mover);
        let res = verify_working_copy_inner(
            &ctx,
            &Source(source),
            &Target(target),
            Source(source_cs_id),
            Target(target_cs_id),
            &mover,
            &reverse_mover,
            PrefixesToVisit::default(),
        )
        .await;

        assert!(res.is_err());

        Ok(())
    }

    #[fbinit::test]
    async fn test_verify_working_copy_with_prefixes(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let source: BlobRepo = test_repo_factory::build_empty(fb)?;
        let source_cs_id = CreateCommitContext::new_root(&ctx, &source)
            .add_file("prefix/sub/file1", "1")
            .add_file("prefix/sub/file2", "2")
            .add_file("prefix/file1", "1")
            .commit()
            .await?;

        let target: BlobRepo = test_repo_factory::build_empty(fb)?;
        let target_cs_id = CreateCommitContext::new_root(&ctx, &target)
            .add_file("sub/file1", "1")
            .add_file("sub/file2", "2")
            .add_file("file1", "someothercontent")
            .commit()
            .await?;

        let mover: Mover = Arc::new(reverse_prefix_mover);
        let reverse_mover: Mover = Arc::new(prefix_mover);
        let res = verify_working_copy_inner(
            &ctx,
            &Source(source.clone()),
            &Target(target.clone()),
            Source(source_cs_id),
            Target(target_cs_id),
            &mover,
            &reverse_mover,
            PrefixesToVisit::default(),
        )
        .await;

        assert!(res.is_err());

        // Now limit the paths we need to verify
        verify_working_copy_inner(
            &ctx,
            &Source(source),
            &Target(target),
            Source(source_cs_id),
            Target(target_cs_id),
            &mover,
            &reverse_mover,
            PrefixesToVisit {
                source_prefixes_to_visit: Some(vec![MPath::new("prefix/sub")?]),
                target_prefixes_to_visit: Some(vec![MPath::new("sub")?]),
            },
        )
        .await?;

        Ok(())
    }

    #[fbinit::test]
    async fn test_verify_working_copy_fast_path(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let mut factory = TestRepoFactory::new(fb)?;
        let source = factory.with_id(RepositoryId::new(0)).build()?;
        let root_source_cs_id = CreateCommitContext::new_root(&ctx, &source)
            .add_file("prefix/sub/file1", "1")
            .add_file("somefile", "content")
            .commit()
            .await?;
        let first_source_cs_id = CreateCommitContext::new(&ctx, &source, vec![root_source_cs_id])
            .add_file("prefix/sub/file2", "1")
            .commit()
            .await?;
        let second_source_cs_id = CreateCommitContext::new(&ctx, &source, vec![first_source_cs_id])
            .add_file("special/1", "special")
            .commit()
            .await?;

        let target = factory.with_id(RepositoryId::new(1)).build()?;
        let root_target_cs_id = CreateCommitContext::new_root(&ctx, &target)
            .add_file("sub/file1", "1")
            .commit()
            .await?;
        let first_target_cs_id = CreateCommitContext::new(&ctx, &target, vec![root_target_cs_id])
            .add_file("sub/file2", "1")
            .commit()
            .await?;
        let second_target_cs_id = CreateCommitContext::new(&ctx, &target, vec![first_target_cs_id])
            .add_file("special/1", "special")
            .commit()
            .await?;

        let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory()?;
        let repos = CommitSyncRepos::LargeToSmall {
            small_repo: target,
            large_repo: source,
        };

        let live_commit_sync_config = get_live_commit_sync_config();
        let commit_sync_data_provider =
            CommitSyncDataProvider::Live(live_commit_sync_config.clone());
        let commit_syncer =
            CommitSyncer::new_with_provider(&ctx, mapping, repos, commit_sync_data_provider);

        println!("checking root commit");
        for version in &["first_version", "second_version"] {
            println!("version: {}", version);
            verify_working_copy_with_version_fast_path(
                &ctx,
                &commit_syncer,
                Source(root_source_cs_id),
                Target(root_target_cs_id),
                &CommitSyncConfigVersion(version.to_string()),
                live_commit_sync_config.clone(),
            )
            .await?;
        }

        println!("checking first commit");
        for version in &["first_version", "second_version"] {
            println!("version: {}", version);
            verify_working_copy_with_version_fast_path(
                &ctx,
                &commit_syncer,
                Source(first_source_cs_id),
                Target(first_target_cs_id),
                &CommitSyncConfigVersion(version.to_string()),
                live_commit_sync_config.clone(),
            )
            .await?;
        }

        let version = "second_version";
        println!("checking second commit, version: {}", version);
        verify_working_copy_with_version_fast_path(
            &ctx,
            &commit_syncer,
            Source(second_source_cs_id),
            Target(second_target_cs_id),
            &CommitSyncConfigVersion(version.to_string()),
            live_commit_sync_config.clone(),
        )
        .await?;

        let version = "first_version";
        println!("checking second commit, version: {}", version);
        let res = verify_working_copy_with_version_fast_path(
            &ctx,
            &commit_syncer,
            Source(second_source_cs_id),
            Target(second_target_cs_id),
            &CommitSyncConfigVersion(version.to_string()),
            live_commit_sync_config.clone(),
        )
        .await;
        assert!(res.is_err());

        let version = "second_version";
        println!("checking first and second commit, version: {}", version);
        let res = verify_working_copy_with_version_fast_path(
            &ctx,
            &commit_syncer,
            Source(first_source_cs_id),
            Target(second_target_cs_id),
            &CommitSyncConfigVersion(version.to_string()),
            live_commit_sync_config.clone(),
        )
        .await;
        assert!(res.is_err());

        Ok(())
    }

    fn prefix_mover(v: &MPath) -> Result<Option<MPath>, Error> {
        let prefix = MPath::new("prefix").unwrap();
        Ok(Some(MPath::join(&prefix, v)))
    }

    fn reverse_prefix_mover(v: &MPath) -> Result<Option<MPath>, Error> {
        let prefix = MPath::new("prefix").unwrap();
        if prefix.is_prefix_of(v) {
            Ok(v.remove_prefix_component(&prefix))
        } else {
            Ok(None)
        }
    }

    async fn init(
        fb: FacebookInit,
        direction: CommitSyncDirection,
    ) -> Result<CommitSyncer<SqlSyncedCommitMapping>, Error> {
        let ctx = CoreContext::test_mock(fb);
        let small_repo = Linear::getrepo_with_id(fb, RepositoryId::new(0)).await;
        let large_repo = Linear::getrepo_with_id(fb, RepositoryId::new(1)).await;

        let master = BookmarkName::new("master")?;
        let maybe_master_val = small_repo.get_bonsai_bookmark(ctx.clone(), &master).await?;

        let master_val = maybe_master_val.ok_or(Error::msg("master not found"))?;
        let changesets =
            AncestorsNodeStream::new(ctx.clone(), &small_repo.get_changeset_fetcher(), master_val)
                .collect()
                .compat()
                .await?;

        let current_version = CommitSyncConfigVersion("noop".to_string());

        let repos = match direction {
            CommitSyncDirection::LargeToSmall => CommitSyncRepos::LargeToSmall {
                small_repo: small_repo.clone(),
                large_repo: large_repo.clone(),
            },
            CommitSyncDirection::SmallToLarge => CommitSyncRepos::SmallToLarge {
                small_repo: small_repo.clone(),
                large_repo: large_repo.clone(),
            },
        };

        let mapping = SqlSyncedCommitMapping::with_sqlite_in_memory().unwrap();
        for cs_id in changesets {
            mapping
                .add(
                    &ctx,
                    SyncedCommitMappingEntry {
                        large_repo_id: large_repo.get_repoid(),
                        small_repo_id: small_repo.get_repoid(),
                        small_bcs_id: cs_id,
                        large_bcs_id: cs_id,
                        version_name: Some(current_version.clone()),
                        source_repo: Some(repos.get_source_repo_type()),
                    },
                )
                .await?;
        }

        let (lv_cfg, lv_cfg_src) = TestLiveCommitSyncConfig::new_with_source();

        let common_config = CommonCommitSyncConfig {
            common_pushrebase_bookmarks: vec![BookmarkName::new("master")?],
            small_repos: hashmap! {
                small_repo.get_repoid() => SmallRepoPermanentConfig {
                    bookmark_prefix: AsciiString::from_str("prefix/").unwrap(),
                }
            },
            large_repo_id: large_repo.get_repoid(),
        };

        let current_version_config = CommitSyncConfig {
            large_repo_id: large_repo.get_repoid(),
            common_pushrebase_bookmarks: vec![BookmarkName::new("master")?],
            small_repos: hashmap! {
                small_repo.get_repoid() => SmallRepoCommitSyncConfig {
                    default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
                    map: hashmap! { },

                },
            },
            version_name: current_version.clone(),
        };

        lv_cfg_src.add_common_config(common_config);
        lv_cfg_src.add_config(current_version_config);

        let commit_sync_data_provider = CommitSyncDataProvider::Live(Arc::new(lv_cfg));

        Ok(CommitSyncer::new_with_provider(
            &ctx,
            mapping,
            repos,
            commit_sync_data_provider,
        ))
    }
}
