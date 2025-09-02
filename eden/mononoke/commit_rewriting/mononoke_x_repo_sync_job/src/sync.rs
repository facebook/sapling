/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::format_err;
use blobstore::Loadable;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateReason;
use bookmarks::BookmarksRef;
use bulk_derivation::BulkDerivation;
use commit_graph::CommitGraphRef;
use context::CoreContext;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncData;
use cross_repo_sync::CommitSyncOutcome;
use cross_repo_sync::PushrebaseRewriteDates;
use cross_repo_sync::Source;
use cross_repo_sync::Target;
use cross_repo_sync::find_toposorted_unsynced_ancestors;
use cross_repo_sync::find_toposorted_unsynced_ancestors_with_commit_graph;
use cross_repo_sync::get_version_and_parent_map_for_sync_via_pushrebase;
use cross_repo_sync::log_debug;
use cross_repo_sync::log_info;
use cross_repo_sync::log_trace;
use cross_repo_sync::log_warning;
use cross_repo_sync::unsafe_always_rewrite_sync_commit;
use cross_repo_sync::unsafe_get_parent_map_for_target_bookmark_rewrite;
use cross_repo_sync::unsafe_sync_commit;
use cross_repo_sync::unsafe_sync_commit_pushrebase;
use fsnodes::RootFsnodeId;
use futures::FutureExt;
use futures::future::try_join_all;
use futures::stream::TryStreamExt;
use futures::try_join;
use futures_stats::TimedFutureExt;
use futures_stats::TimedTryFutureExt;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_types::ChangesetId;
use mononoke_types::Timestamp;
use repo_blobstore::RepoBlobstoreRef;
use repo_identity::RepoIdentityRef;
use scuba_ext::FutureStatsScubaExt;
use scuba_ext::MononokeScubaSampleBuilder;

use crate::reporting::log_bookmark_deletion_result;
use crate::reporting::log_non_pushrebase_sync_single_changeset_result;
use crate::reporting::log_pushrebase_sync_single_changeset_result;
use crate::reporting::log_success_to_scuba;

pub trait Repo = cross_repo_sync::Repo;

#[derive(Debug, Eq, PartialEq)]
pub enum SyncResult {
    Synced(Vec<ChangesetId>),
    // SkippedNoKnownVersion usually happens when a new root commit was
    // added to the repository, and its descendant are not merged into any
    // mainline bookmark. See top level doc comments in main file for
    // more details.
    SkippedNoKnownVersion,
}

/// Sync all new commits and update the bookmark that were introduced by BookmarkUpdateLogEntry
/// in the source repo.
/// This function:
/// 1) Finds commits that needs syncing
/// 2) Syncs them from source repo into target (*)
/// 3) Updates the bookmark
///
/// (*) There are two ways how a commit can be synced from source repo into a target repo.
/// It can either be rewritten and saved into a target repo, or rewritten and pushrebased
/// in a target repo. This depends on which bookmark introduced a commit - if it's a
/// common_pushrebase_bookmark (usually "master"), then a commit will be pushrebased.
/// Otherwise it will be synced without pushrebase.
pub async fn sync_single_bookmark_update_log<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    entry: BookmarkUpdateLogEntry,
    common_pushrebase_bookmarks: &HashSet<BookmarkKey>,
    mut scuba_sample: MononokeScubaSampleBuilder,
    pushrebase_rewrite_dates: PushrebaseRewriteDates,
) -> Result<SyncResult, Error>
where
    R: Repo,
{
    log_info(ctx, format!("processing log entry #{}", entry.id));
    let source_bookmark = Source(entry.bookmark_name);
    let target_bookmark = Target(
        commit_sync_data
            .rename_bookmark(&source_bookmark)
            .await?
            .ok_or_else(|| format_err!("unexpected empty bookmark rename"))?,
    );
    scuba_sample
        .add("source_bookmark_name", format!("{}", source_bookmark))
        .add("target_bookmark_name", format!("{}", target_bookmark));

    let to_cs_id = match entry.to_changeset_id {
        Some(to_cs_id) => to_cs_id,
        None => {
            // This is a bookmark deletion - just delete a bookmark and exit,
            // no need to sync commits
            process_bookmark_deletion(
                ctx,
                commit_sync_data,
                scuba_sample,
                &source_bookmark,
                &target_bookmark,
                common_pushrebase_bookmarks,
                Some(entry.timestamp),
            )
            .boxed()
            .await?;

            return Ok(SyncResult::Synced(vec![]));
        }
    };

    sync_commit_and_ancestors(
        ctx,
        commit_sync_data,
        entry.from_changeset_id,
        to_cs_id,
        &Some(target_bookmark),
        common_pushrebase_bookmarks,
        scuba_sample,
        pushrebase_rewrite_dates,
        Some(entry.timestamp),
        &None,
        false,
    )
    .boxed()
    .await
    // Note: counter update might fail after a successful sync
}

/// Sync and all of its unsynced ancestors **if the given commit has at least
/// one synced ancestor**.
/// Unsafe_change_mapping_version allows for changing the mapping version used when pushrebasing the
/// commit. Should be only used when we know that the new mapping version is safe to use on common
/// pushrebase bookmark.
pub async fn sync_commit_and_ancestors<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    from_cs_id: Option<ChangesetId>,
    to_cs_id: ChangesetId,
    // When provided, sync commits to this bookmark using pushrebase.
    mb_target_bookmark: &Option<Target<BookmarkKey>>,
    common_pushrebase_bookmarks: &HashSet<BookmarkKey>,
    scuba_sample: MononokeScubaSampleBuilder,
    pushrebase_rewrite_dates: PushrebaseRewriteDates,
    bookmark_update_timestamp: Option<Timestamp>,
    unsafe_change_mapping_version_during_pushrebase: &Option<CommitSyncConfigVersion>,
    unsafe_force_rewrite_parent_to_target_bookmark: bool,
) -> Result<SyncResult, Error>
where
    R: Repo,
{
    log_debug(
        ctx,
        format!("Syncing commit {to_cs_id} from commit {0:#?}", from_cs_id),
    );

    log_debug(
        ctx,
        format!("Targeting bookmark {0:#?}", mb_target_bookmark),
    );

    if let Some(new_version) = unsafe_change_mapping_version_during_pushrebase {
        log_warning(
            ctx,
            format!("Changing mapping version during pushrebase to {new_version}"),
        );
    };
    if unsafe_force_rewrite_parent_to_target_bookmark {
        log_warning(ctx, "UNSAFE: Bypass working copy validation is enabled!");
    };

    let hint = match mb_target_bookmark {
        Some(target_bookmark) if common_pushrebase_bookmarks.contains(target_bookmark) => Some(
            CandidateSelectionHint::AncestorOfBookmark(
                target_bookmark.clone(),
                Target(commit_sync_data.get_target_repo().clone()),
            )
            .try_into_desired_relationship(ctx)
            .boxed()
            .await?
            .ok_or_else(||
                anyhow!(
                    "ProgrammingError: hint doesn't represent relationship when targeting bookmark {target_bookmark}"
                )
            )?,
        ),
        _ => None,
    };
    log_debug(ctx, "finding unsynced ancestors from source repo...");

    let (unsynced_ancestors, synced_ancestors_versions) =
        find_toposorted_unsynced_ancestors(ctx, commit_sync_data, to_cs_id.clone(), hint)
            .boxed()
            .await?;

    let version = if !synced_ancestors_versions.has_ancestor_with_a_known_outcome() {
        return Ok(SyncResult::SkippedNoKnownVersion);
    } else {
        let maybe_version = synced_ancestors_versions
            .get_only_version()
            .with_context(|| format!("failed to sync cs id {}", to_cs_id))?;
        maybe_version.ok_or_else(|| {
            format_err!(
                "failed to sync {} - all of the ancestors are NotSyncCandidate",
                to_cs_id
            )
        })?
    };

    let len = unsynced_ancestors.len();
    log_info(ctx, format!("{} unsynced ancestors of {}", len, to_cs_id));

    if let Some(target_bookmark) = mb_target_bookmark {
        // This is forward sync. The direction is small to large, so the source bookmark is the small
        // bookmark which is the key in the common_pushrebase_bookmarks
        // Source: small, e.g. `heads/main`
        // Target: large, e.g. `main`
        // common_pushrebase_bookmarks: large, e.g. `["main"]`

        if common_pushrebase_bookmarks.contains(target_bookmark) {
            // This is a commit that was introduced by common pushrebase bookmark (e.g. "master").
            // Use pushrebase to sync a commit.
            if let Some(from_cs_id) = from_cs_id {
                check_forward_move(ctx, commit_sync_data, to_cs_id, from_cs_id).await?;
            }

            log_debug(ctx, "obtaining version for the sync...");

            let (version, parent_mapping) = match (
                unsafe_change_mapping_version_during_pushrebase,
                unsafe_force_rewrite_parent_to_target_bookmark,
            ) {
                // `unsafe_force_rewrite_parent_to_target_bookmark` can only be
                // used when a new mapping version is also manually specified,
                // because some validation is skipped **because we know the
                // mapping version that will be used in the end**.
                (Some(new_version), true) => {
                    let parent_map = unsafe_get_parent_map_for_target_bookmark_rewrite(
                        ctx,
                        commit_sync_data,
                        target_bookmark,
                        &synced_ancestors_versions,
                    )
                    .boxed()
                    .await?;
                    (new_version.clone(), parent_map)
                }
                _ => {
                    get_version_and_parent_map_for_sync_via_pushrebase(
                        ctx,
                        commit_sync_data,
                        target_bookmark,
                        version,
                        &synced_ancestors_versions,
                    )
                    .boxed()
                    .await?
                }
            };

            return sync_commits_via_pushrebase(
                ctx,
                commit_sync_data,
                target_bookmark,
                common_pushrebase_bookmarks,
                scuba_sample.clone(),
                unsynced_ancestors,
                &version,
                pushrebase_rewrite_dates,
                bookmark_update_timestamp,
                unsafe_change_mapping_version_during_pushrebase,
                parent_mapping,
            )
            .boxed()
            .await
            .map(SyncResult::Synced);
        }
    }
    // Use a normal sync since a bookmark is not a common pushrebase bookmark
    let mut res = vec![];
    for cs_id in unsynced_ancestors {
        let synced = sync_commit_without_pushrebase(
            ctx,
            commit_sync_data,
            scuba_sample.clone(),
            cs_id,
            common_pushrebase_bookmarks,
            &version,
            bookmark_update_timestamp,
        )
        .boxed()
        .await?;
        res.extend(synced);
    }
    let maybe_remapped_cs_id = find_remapped_cs_id(ctx, commit_sync_data, to_cs_id).await?;
    let remapped_cs_id =
        maybe_remapped_cs_id.ok_or_else(|| format_err!("unknown sync outcome for {}", to_cs_id))?;
    if let Some(target_bookmark) = mb_target_bookmark {
        move_or_create_bookmark(
            ctx,
            commit_sync_data.get_target_repo(),
            target_bookmark,
            remapped_cs_id,
        )
        .boxed()
        .await?;
    }
    Ok(SyncResult::Synced(res))
}

/// This function syncs commits via pushrebase with a caveat - some commits shouldn't be
/// pushrebased! Consider pushing of a merge
///
/// ```text
///  source repo (X - synced commit, O - unsynced commit)
///
///     O <- merge commit (this commit needs to be pushrebased in target repo)
///    / |
///   X   O <- this commit DOES NOT NEED to be pushrebased in the target repo
///  ...  |
///      ...
///
/// Just as normal pushrebase behaves while pushing merges, we rebase the actual merge
/// commit and it's ancestors, but we don't rebase merge ancestors.
///
/// Optionally, the mapping version can be changed during pushrebase - this is useful
/// for setting up the initial configuration for the sync. The validation of the version
/// applicability to pushrebased bookmarks belongs to caller.
/// ```
async fn sync_commits_via_pushrebase<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    target_bookmark: &Target<BookmarkKey>,
    common_pushrebase_bookmarks: &HashSet<BookmarkKey>,
    scuba_sample: MononokeScubaSampleBuilder,
    unsynced_ancestors: Vec<ChangesetId>,
    mut version: &CommitSyncConfigVersion,
    pushrebase_rewrite_dates: PushrebaseRewriteDates,
    bookmark_update_timestamp: Option<Timestamp>,
    unsafe_change_mapping_version: &Option<CommitSyncConfigVersion>,
    parent_mapping: HashMap<ChangesetId, ChangesetId>,
) -> Result<Vec<ChangesetId>, Error>
where
    R: Repo,
{
    if let Some(new_version) = unsafe_change_mapping_version {
        log_warning(
            ctx,
            format!("UNSAFE: changing mapping version during pushrebase to {new_version}"),
        );
    };
    let change_mapping_version =
        if let Some(unsafe_change_mapping_version) = unsafe_change_mapping_version {
            if version != unsafe_change_mapping_version {
                version = unsafe_change_mapping_version;
            }
            Some(unsafe_change_mapping_version.clone())
        } else {
            None
        };

    let small_repo = commit_sync_data.get_source_repo();
    // It stores commits that were introduced as part of current bookmark update, but that
    // shouldn't be pushrebased.
    let mut no_pushrebase = HashSet::new();
    let mut res = vec![];

    // Iterate in reverse order i.e. descendants before ancestors
    for cs_id in unsynced_ancestors.iter().rev() {
        if no_pushrebase.contains(cs_id) {
            continue;
        }

        let bcs = cs_id.load(ctx, small_repo.repo_blobstore()).await?;

        let mut parents = bcs.parents();
        let maybe_p1 = parents.next();
        let maybe_p2 = parents.next();
        if let (Some(p1), Some(p2)) = (maybe_p1, maybe_p2) {
            if parents.next().is_some() {
                return Err(format_err!("only 2 parent merges are supported"));
            }

            no_pushrebase.extend(validate_if_new_repo_merge(ctx, small_repo, p1, p2).await?);
        }
    }

    for cs_id in unsynced_ancestors {
        let maybe_new_cs_id = if no_pushrebase.contains(&cs_id) {
            sync_commit_without_pushrebase(
                ctx,
                commit_sync_data,
                scuba_sample.clone(),
                cs_id,
                common_pushrebase_bookmarks,
                version,
                bookmark_update_timestamp,
            )
            .await?
        } else {
            log_info(
                ctx,
                format!("syncing {} via pushrebase for {}", cs_id, &target_bookmark),
            );
            let (stats, result) = pushrebase_commit(
                ctx,
                commit_sync_data,
                target_bookmark,
                cs_id,
                pushrebase_rewrite_dates,
                version.clone(),
                change_mapping_version.clone(),
                parent_mapping.clone(),
            )
            .timed()
            .await;
            log_pushrebase_sync_single_changeset_result(
                ctx.clone(),
                scuba_sample.clone(),
                cs_id,
                &result,
                stats,
                bookmark_update_timestamp,
            );
            let maybe_new_cs_id = result?;
            maybe_new_cs_id.into_iter().collect()
        };

        res.extend(maybe_new_cs_id);
    }
    Ok(res)
}

pub async fn sync_commit_without_pushrebase<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    scuba_sample: MononokeScubaSampleBuilder,
    cs_id: ChangesetId,
    common_pushrebase_bookmarks: &HashSet<BookmarkKey>,
    version: &CommitSyncConfigVersion,
    bookmark_update_timestamp: Option<Timestamp>,
) -> Result<Vec<ChangesetId>, Error>
where
    R: Repo,
{
    log_info(ctx, format!("syncing {}", cs_id));
    let bcs = cs_id
        .load(ctx, commit_sync_data.get_source_repo().repo_blobstore())
        .await?;

    let (stats, result) = if bcs.is_merge() {
        // We allow syncing of a merge only if there's no intersection between ancestors of this
        // merge commit and ancestors of common pushrebase bookmark in target repo.
        // The code below does exactly that - it fetches common_pushrebase_bookmarks and parent
        // commits from the target repo, and then it checks if there are no intersection.
        let large_repo = commit_sync_data.get_target_repo();
        let mut book_values = vec![];
        for common_bookmark in common_pushrebase_bookmarks {
            book_values.push(large_repo.bookmarks().get(
                ctx.clone(),
                common_bookmark,
                bookmarks::Freshness::MostRecent,
            ));
        }

        let book_values = try_join_all(book_values).await?;
        let book_values = book_values.into_iter().flatten().collect();

        let parents = try_join_all(
            bcs.parents()
                .map(|p| find_remapped_cs_id(ctx, commit_sync_data, p)),
        )
        .await?;
        let maybe_independent_branch = check_if_independent_branch_and_return(
            ctx,
            large_repo,
            parents.into_iter().flatten().collect(),
            book_values,
        )
        .await?;

        // Merge is from a branch completely independent from common_pushrebase_bookmark -
        // it's fine to sync it.
        if maybe_independent_branch.is_none() {
            bail!(
                "cannot sync merge commit - one of it's ancestors is an ancestor of a common pushrebase bookmark"
            );
        };

        unsafe_always_rewrite_sync_commit(
            ctx,
            cs_id,
            commit_sync_data,
            None,
            version,
            CommitSyncContext::XRepoSyncJob,
        )
        .timed()
        .await
    } else {
        // When there are multiple choices for what should be the parent of the commit there's no
        // right or wrong answers, yet we have to pick something. Let's find something close to
        // mainline branch if possible.
        //
        // XXXX: With current "hinting" infra we can pull of this trick reliably only when there's
        // one common pushrebase bookmark. That's fine, we don't have more in production and maybe
        // we shouldn't even allow more than one.
        // For now let's give privilege to the first one.
        let parent_mapping_selection_hint: CandidateSelectionHint<R> =
            if let Some(bookmark) = common_pushrebase_bookmarks.iter().next() {
                CandidateSelectionHint::AncestorOfBookmark(
                    Target(bookmark.clone()),
                    Target(commit_sync_data.get_target_repo().clone()),
                )
            } else {
                // XXX: in this case it should be "Any" rather than "Only"
                CandidateSelectionHint::Only
            };

        unsafe_sync_commit(
            ctx,
            cs_id,
            commit_sync_data,
            parent_mapping_selection_hint,
            CommitSyncContext::XRepoSyncJob,
            Some(version.clone()),
            false, // add_mapping_to_hg_extra
        )
        .timed()
        .await
    };

    log_non_pushrebase_sync_single_changeset_result(
        ctx.clone(),
        scuba_sample.clone(),
        cs_id,
        &result,
        stats,
        bookmark_update_timestamp,
    );

    let maybe_cs_id = result?;
    Ok(maybe_cs_id.into_iter().collect())
}

/// Run the initial import of a small repo into a large repo.
/// It will sync a specific commit (i.e. head commit) and all of its ancestors
/// and optionally bookmark the head commit.
pub async fn sync_commits_for_initial_import<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    scuba_sample: MononokeScubaSampleBuilder,
    // Head commit to sync. All of its unsynced ancestors will be synced as well.
    cs_id: ChangesetId,
    // Sync config version to use for importing the commits.
    config_version: CommitSyncConfigVersion,
    disable_progress_bar: bool,
    no_automatic_derivation: bool,
    derivation_batch_size: usize,
    add_mapping_to_hg_extra: bool,
) -> Result<Vec<ChangesetId>>
where
    R: Repo,
{
    log_info(ctx, format!("Syncing {cs_id} for initial import"));
    log_info(
        ctx,
        format!(
            "Source repo: {} / Target repo: {}",
            commit_sync_data.get_source_repo().repo_identity().name(),
            commit_sync_data.get_target_repo().repo_identity().name(),
        ),
    );
    log_debug(
        ctx,
        format!(
            "Automatic derivation is {0}",
            if no_automatic_derivation {
                "disabled"
            } else {
                "enabled"
            }
        ),
    );

    // All the synced ancestors of the provided commit should have been synced
    // using the config version that was provided manually, or we can create
    // a broken set of commits.
    let (unsynced_ancestors, synced_ancestors_versions, _last_synced_ancestors) =
        find_toposorted_unsynced_ancestors_with_commit_graph(ctx, commit_sync_data, cs_id.clone())
            .try_timed()
            .await?
            .log_future_stats(
                ctx.scuba().clone(),
                "Finding toposorted unsynced ancestors with commit graph",
                None,
            );

    let synced_ancestors_versions = synced_ancestors_versions
        .versions
        .into_iter()
        .collect::<Vec<_>>();

    // IF YOU REALLY KNOW WHAT YOU'RE DOING, you can comment out the
    // assertions below and use different config versions.
    if !synced_ancestors_versions.is_empty() {
        if synced_ancestors_versions.len() != 1 {
            bail!("Multiple config versions were used to sync the ancestors of the head commit.");
        }

        if config_version != synced_ancestors_versions[0] {
            bail!("Provided config version doesn't match the one used to sync ancestors");
        }
    }

    let num_unsynced_ancestors: u64 = unsynced_ancestors.len().try_into()?;

    log_info(
        ctx,
        format!("Found {0} unsynced ancestors", num_unsynced_ancestors),
    );

    log_trace(
        ctx,
        format!("Unsynced ancestors: {0:#?}", &unsynced_ancestors),
    );

    let mb_prog_bar = if disable_progress_bar {
        None
    } else {
        let progress_bar = ProgressBar::new(num_unsynced_ancestors)
        .with_message("Syncing ancestors...")
        .with_style(
            ProgressStyle::with_template(
                "[{percent}%][elapsed: {elapsed}] {msg} [{bar:60.cyan}] (ETA: {eta}) ({pos}/{len}) ({per_sec}) ",
            )?
            .progress_chars("#>-"),
        );
        progress_bar.enable_steady_tick(std::time::Duration::from_secs(3));
        Some(progress_bar)
    };

    let large_repo = commit_sync_data.get_target_repo();

    let mut res = vec![];
    let mut changesets_to_derive = vec![];

    // Sync all of the ancestors first
    for ancestor_cs_id in unsynced_ancestors {
        let (stats, mb_synced) = unsafe_sync_commit(
            ctx,
            ancestor_cs_id,
            commit_sync_data,
            CandidateSelectionHint::Only,
            CommitSyncContext::ForwardSyncerInitialImport,
            Some(config_version.clone()),
            add_mapping_to_hg_extra,
        )
        .timed()
        .boxed()
        .await;
        let mb_synced = mb_synced?;
        let synced = mb_synced
            .clone()
            .ok_or(anyhow!("Failed to sync ancestor commit {}", ancestor_cs_id))?;
        res.push(synced);
        changesets_to_derive.push(synced);

        log_debug(
            ctx,
            format!("Ancestor {ancestor_cs_id} synced successfully as {synced}"),
        );

        // Fsnodes always need to be derived synchronously during initial
        // import because syncing a commit with submodule expansion depends
        // on the fsnodes of its parents.
        //
        // If fsnodes aren't derived synchronously, expansion of submodules
        // will derive it using an InMemoryRepo, throwing away all the results
        // and doing it all again in the next changeset.
        let root_fsnode_id = large_repo
            .repo_derived_data()
            .derive::<RootFsnodeId>(ctx, synced)
            .await?;
        log_trace(
            ctx,
            format!(
                "Root fsnode id from {synced}: {0}",
                root_fsnode_id.into_fsnode_id()
            ),
        );

        if !no_automatic_derivation {
            if changesets_to_derive.len() >= derivation_batch_size {
                derive_initial_import_batch(ctx, large_repo, &changesets_to_derive).await?;
                changesets_to_derive.clear();
            };
        };

        if let Some(progress_bar) = &mb_prog_bar {
            progress_bar.inc(1);
        }

        log_success_to_scuba(scuba_sample.clone(), ancestor_cs_id, mb_synced, stats, None);
    }

    // Make sure we derive the last batch
    if !no_automatic_derivation && !changesets_to_derive.is_empty() {
        derive_initial_import_batch(ctx, large_repo, &changesets_to_derive).await?;
        changesets_to_derive.clear();
    };

    let (stats, result) = unsafe_sync_commit(
        ctx,
        cs_id,
        commit_sync_data,
        CandidateSelectionHint::Only,
        CommitSyncContext::ForwardSyncerInitialImport,
        Some(config_version),
        add_mapping_to_hg_extra,
    )
    .timed()
    .boxed()
    .await;

    let maybe_cs_id: Option<ChangesetId> = result?;

    // Check that the head commit was synced properly and log something otherwise
    // clippy: This warning relates to creating `err` as `Err(...)` followed by `unwrap_err()`
    // below, which would be redundant.
    // In this instance, it ignores the fact that `err` is used in between by a function that needs
    // a borrow to a `Result`.
    // Since the `Result` owns its content, trying to work around it forces a clone which feels
    // worse than muting clippy for this instance.
    #[allow(clippy::unnecessary_literal_unwrap)]
    let new_cs_id = maybe_cs_id.ok_or_else(|| {
        let err = Err(anyhow!("Head changeset wasn't synced"));
        log_non_pushrebase_sync_single_changeset_result(
            ctx.clone(),
            scuba_sample.clone(),
            cs_id,
            &err,
            stats.clone(),
            None,
        );
        err.unwrap_err()
    })?;

    res.push(new_cs_id.clone());

    log_non_pushrebase_sync_single_changeset_result(
        ctx.clone(),
        scuba_sample,
        cs_id,
        &Ok(Some(new_cs_id)),
        stats,
        None,
    );
    Ok(res)
}

async fn process_bookmark_deletion<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    scuba_sample: MononokeScubaSampleBuilder,
    source_bookmark: &Source<BookmarkKey>,
    target_bookmark: &Target<BookmarkKey>,
    common_pushrebase_bookmarks: &HashSet<BookmarkKey>,
    bookmark_update_timestamp: Option<Timestamp>,
) -> Result<(), Error>
where
    R: Repo,
{
    if common_pushrebase_bookmarks.contains(source_bookmark) {
        Err(format_err!(
            "unexpected deletion of a shared bookmark {}",
            source_bookmark
        ))
    } else {
        log_info(ctx, format!("deleting bookmark {}", target_bookmark));
        let (stats, result) = delete_bookmark(
            ctx.clone(),
            commit_sync_data.get_target_repo(),
            target_bookmark,
        )
        .timed()
        .await;
        log_bookmark_deletion_result(scuba_sample, &result, stats, bookmark_update_timestamp);
        result
    }
}

async fn check_forward_move<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    to_cs_id: ChangesetId,
    from_cs_id: ChangesetId,
) -> Result<(), Error>
where
    R: Repo,
{
    if !commit_sync_data
        .get_source_repo()
        .commit_graph()
        .is_ancestor(ctx, from_cs_id, to_cs_id)
        .await?
    {
        return Err(format_err!(
            "non-forward moves of shared bookmarks are not allowed"
        ));
    }
    Ok(())
}

async fn find_remapped_cs_id<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    orig_cs_id: ChangesetId,
) -> Result<Option<ChangesetId>, Error>
where
    R: Repo,
{
    let maybe_sync_outcome = commit_sync_data
        .get_commit_sync_outcome(ctx, orig_cs_id)
        .await?;
    use CommitSyncOutcome::*;
    match maybe_sync_outcome {
        Some(RewrittenAs(cs_id, _)) | Some(EquivalentWorkingCopyAncestor(cs_id, _)) => {
            Ok(Some(cs_id))
        }
        Some(NotSyncCandidate(_)) => Err(format_err!("unexpected NotSyncCandidate")),
        None => Ok(None),
    }
}

async fn pushrebase_commit<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    target_bookmark: &Target<BookmarkKey>,
    cs_id: ChangesetId,
    pushrebase_rewrite_dates: PushrebaseRewriteDates,
    version: CommitSyncConfigVersion,
    change_mapping_version: Option<CommitSyncConfigVersion>,
    parent_mapping: HashMap<ChangesetId, ChangesetId>,
) -> Result<Option<ChangesetId>, Error>
where
    R: Repo,
{
    let small_repo = commit_sync_data.get_source_repo();
    let bcs = cs_id.load(ctx, small_repo.repo_blobstore()).await?;

    unsafe_sync_commit_pushrebase(
        ctx,
        bcs,
        commit_sync_data,
        target_bookmark.clone(),
        CommitSyncContext::XRepoSyncJob,
        pushrebase_rewrite_dates,
        version,
        change_mapping_version,
        parent_mapping,
    )
    .await
}

/// Function validates if a this merge is supported for x-repo sync. At the moment we support
/// only a single type of merges - merge that introduces a new repo i.e. merge p1 and p2
/// have no shared history.
///
///     O <- merge commit to sync
///    / |
///   O   O <- these are new commits we need to sync
///   |   |
///   |   ...
///
/// This function returns new commits that were introduced by this merge
async fn validate_if_new_repo_merge(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + RepoIdentityRef + CommitGraphRef),
    p1: ChangesetId,
    p2: ChangesetId,
) -> Result<Vec<ChangesetId>, Error> {
    let p1gen = repo.commit_graph().changeset_generation(ctx, p1);
    let p2gen = repo.commit_graph().changeset_generation(ctx, p2);
    let (p1gen, p2gen) = try_join!(p1gen, p2gen)?;
    // FIXME: this code has an assumption that parent with a smaller generation number is a
    // parent that introduces a new repo. This is usually the case, however it might not be true
    // in some rare cases.
    let (larger_gen, smaller_gen) = if p1gen > p2gen { (p1, p2) } else { (p2, p1) };

    let err_msg = || format_err!("unsupported merge - only merges of new repos are supported");

    // Check if this is a diamond merge i.e. check if any of the ancestor of smaller_gen
    // is also ancestor of larger_gen.
    let maybe_independent_branch =
        check_if_independent_branch_and_return(ctx, repo, vec![smaller_gen], vec![larger_gen])
            .await?;

    let independent_branch = maybe_independent_branch.ok_or_else(err_msg)?;

    Ok(independent_branch)
}

/// Checks if `branch_tips` and their ancestors have no intersection with ancestors of
/// other_branches. If there are no intersection then branch_tip and it's ancestors are returned,
/// i.e. (::branch_tips) is returned in mercurial's revset terms
async fn check_if_independent_branch_and_return(
    ctx: &CoreContext,
    repo: &(impl RepoBlobstoreRef + RepoIdentityRef + CommitGraphRef),
    branch_tips: Vec<ChangesetId>,
    other_branches: Vec<ChangesetId>,
) -> Result<Option<Vec<ChangesetId>>, Error> {
    let blobstore = repo.repo_blobstore();
    let bcss = repo
        .commit_graph()
        .ancestors_difference_stream(ctx, branch_tips.clone(), other_branches)
        .await?
        .map_ok(move |cs| async move { Ok(cs.load(ctx, blobstore).await?) })
        .try_buffered(100)
        .try_collect::<Vec<_>>()
        .await?;

    let bcss: Vec<_> = bcss.into_iter().rev().collect();
    let mut cs_to_parents: HashMap<_, Vec<_>> = HashMap::new();
    for bcs in &bcss {
        let cs_id = bcs.get_changeset_id();
        cs_to_parents.insert(cs_id, bcs.parents().collect());
    }

    // If any of branch_tips hasn't been returned, then it was an ancestor of some of the
    // other_branches.
    for tip in branch_tips {
        if !cs_to_parents.contains_key(&tip) {
            return Ok(None);
        }
    }

    for parents in cs_to_parents.values() {
        for p in parents {
            if !cs_to_parents.contains_key(p) {
                return Ok(None);
            }
        }
    }

    Ok(Some(cs_to_parents.keys().cloned().collect()))
}

async fn delete_bookmark(
    ctx: CoreContext,
    repo: &impl BookmarksRef,
    bookmark: &BookmarkKey,
) -> Result<(), Error> {
    let mut book_txn = repo.bookmarks().create_transaction(ctx.clone());
    let maybe_bookmark_val = repo
        .bookmarks()
        .get(ctx.clone(), bookmark, bookmarks::Freshness::MostRecent)
        .await?;
    if let Some(bookmark_value) = maybe_bookmark_val {
        book_txn.delete(bookmark, bookmark_value, BookmarkUpdateReason::XRepoSync)?;
        let res = book_txn.commit().await?.is_some();

        if res {
            Ok(())
        } else {
            Err(format_err!("failed to delete a bookmark"))
        }
    } else {
        log_warning(
            &ctx,
            format!(
                "Not deleting '{}' bookmark because it does not exist",
                bookmark
            ),
        );
        Ok(())
    }
}

async fn move_or_create_bookmark(
    ctx: &CoreContext,
    repo: &impl BookmarksRef,
    bookmark: &BookmarkKey,
    cs_id: ChangesetId,
) -> Result<(), Error> {
    let maybe_bookmark_val = repo
        .bookmarks()
        .get(ctx.clone(), bookmark, bookmarks::Freshness::MostRecent)
        .await?;

    let mut book_txn = repo.bookmarks().create_transaction(ctx.clone());
    match maybe_bookmark_val {
        Some(old_bookmark_val) => {
            book_txn.update(
                bookmark,
                cs_id,
                old_bookmark_val,
                BookmarkUpdateReason::XRepoSync,
            )?;
        }
        None => {
            book_txn.create(bookmark, cs_id, BookmarkUpdateReason::XRepoSync)?;
        }
    }
    let res = book_txn.commit().await?.is_some();

    if res {
        Ok(())
    } else {
        Err(format_err!("failed to move or create a bookmark"))
    }
}

async fn derive_initial_import_batch<R: Repo>(
    ctx: &CoreContext,
    large_repo: &R,
    changesets_to_derive: &[ChangesetId],
) -> Result<()> {
    let derived_data_types = large_repo
        .repo_derived_data()
        .active_config()
        .types
        .iter()
        .cloned()
        .collect::<Vec<_>>();

    // Derive all the data types in bulk to speed up the overall import process
    large_repo
        .repo_derived_data()
        .manager()
        .derive_bulk_locally(ctx, changesets_to_derive, None, &derived_data_types, None)
        .await?;

    log_debug(
        ctx,
        format!(
            "Finished bulk derivation of {0} changesets",
            changesets_to_derive.len(),
        ),
    );
    Ok(())
}

#[cfg(test)]
mod test {
    use bookmarks::BookmarkUpdateLogRef;
    use bookmarks::BookmarksMaybeStaleExt;
    use bookmarks::Freshness;
    use cross_repo_sync::find_bookmark_diff;
    use cross_repo_sync::test_utils::TestRepo;
    use cross_repo_sync::test_utils::init_small_large_repo;
    use cross_repo_sync::verify_working_copy;
    use fbinit::FacebookInit;
    use futures::TryStreamExt;
    use maplit::hashset;
    use mononoke_macros::mononoke;
    use mutable_counters::MutableCountersRef;
    use tests_utils::CreateCommitContext;
    use tests_utils::bookmark;
    use tests_utils::resolve_cs_id;
    use tokio::runtime::Runtime;

    use super::*;

    #[mononoke::fbinit_test]
    fn test_simple(fb: FacebookInit) -> Result<(), Error> {
        let runtime = Runtime::new()?;
        runtime.block_on(async move {
            let ctx = CoreContext::test_mock(fb);
            let (syncers, _, _, _) = init_small_large_repo(&ctx).await?;
            let commit_sync_data = syncers.small_to_large;
            let smallrepo = commit_sync_data.get_source_repo();

            // Single commit
            let new_master = CreateCommitContext::new(&ctx, &smallrepo, vec!["master"])
                .add_file("newfile", "newcontent")
                .commit()
                .await?;
            bookmark(&ctx, &smallrepo, "master")
                .set_to(new_master)
                .await?;

            sync_and_validate(&ctx, &commit_sync_data).await?;

            let non_master_commit = CreateCommitContext::new(&ctx, &smallrepo, vec!["master"])
                .add_file("nonmasterfile", "nonmastercontent")
                .commit()
                .await?;
            bookmark(&ctx, &smallrepo, "nonmasterbookmark")
                .set_to(non_master_commit)
                .await?;

            sync_and_validate(&ctx, &commit_sync_data).await?;

            // Create a stack of commits
            let first_in_stack = CreateCommitContext::new(&ctx, &smallrepo, vec!["master"])
                .add_file("stack", "first")
                .commit()
                .await?;

            let second_in_stack = CreateCommitContext::new(&ctx, &smallrepo, vec![first_in_stack])
                .add_file("stack", "second")
                .commit()
                .await?;
            bookmark(&ctx, &smallrepo, "master")
                .set_to(second_in_stack)
                .await?;

            // Create a commit that's based on commit rewritten with noop mapping
            // - it should NOT be rewritten
            let premove = CreateCommitContext::new(&ctx, &smallrepo, vec!["premove"])
                .add_file("premove", "premovecontent")
                .commit()
                .await?;
            bookmark(&ctx, &smallrepo, "newpremove")
                .set_to(premove)
                .await?;

            // Move a bookmark
            bookmark(&ctx, &smallrepo, "newpremove")
                .set_to("premove")
                .await?;
            sync_and_validate(&ctx, &commit_sync_data).await?;
            let commit_sync_outcome = commit_sync_data
                .get_commit_sync_outcome(&ctx, premove)
                .await?
                .ok_or_else(|| format_err!("commit sync outcome not set"))?;
            match commit_sync_outcome {
                CommitSyncOutcome::RewrittenAs(_cs_id, version) => {
                    assert_eq!(version, CommitSyncConfigVersion("noop".to_string()));
                }
                _ => {
                    return Err(format_err!("unexpected outcome"));
                }
            };

            // Delete bookmarks
            bookmark(&ctx, &smallrepo, "newpremove").delete().await?;
            bookmark(&ctx, &smallrepo, "nonmasterbookmark")
                .delete()
                .await?;

            sync_and_validate(&ctx, &commit_sync_data).await?;
            Ok(())
        })
    }

    #[mononoke::fbinit_test]
    fn test_simple_merge(fb: FacebookInit) -> Result<(), Error> {
        let runtime = Runtime::new()?;
        runtime.block_on(async move {
            let ctx = CoreContext::test_mock(fb);
            let (syncers, _, _, _) = init_small_large_repo(&ctx).await?;
            let commit_sync_data = syncers.small_to_large;
            let smallrepo = commit_sync_data.get_source_repo();

            // Merge new repo
            let first_new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("firstnewrepo", "newcontent")
                .commit()
                .await?;
            let second_new_repo = CreateCommitContext::new(&ctx, &smallrepo, vec![first_new_repo])
                .add_file("secondnewrepo", "anothercontent")
                .commit()
                .await?;

            bookmark(&ctx, &smallrepo, "newrepohead")
                .set_to(second_new_repo)
                .await?;

            let res = sync(
                &ctx,
                &commit_sync_data,
                &hashset! {BookmarkKey::new("master")?},
                PushrebaseRewriteDates::No,
            )
            .await?;
            assert_eq!(res.last(), Some(&SyncResult::SkippedNoKnownVersion));

            let merge = CreateCommitContext::new(&ctx, &smallrepo, vec!["master", "newrepohead"])
                .commit()
                .await?;

            bookmark(&ctx, &smallrepo, "master").set_to(merge).await?;

            sync_and_validate_with_common_bookmarks(
                &ctx,
                &commit_sync_data,
                &hashset! {BookmarkKey::new("master")?},
                &hashset! {BookmarkKey::new("newrepohead")?},
                PushrebaseRewriteDates::No,
            )
            .await?;

            // Diamond merges are not allowed
            let diamond_merge =
                CreateCommitContext::new(&ctx, &smallrepo, vec!["master", "newrepohead"])
                    .commit()
                    .await?;
            bookmark(&ctx, &smallrepo, "master")
                .set_to(diamond_merge)
                .await?;
            assert!(sync_and_validate(&ctx, &commit_sync_data,).await.is_err());
            Ok(())
        })
    }

    #[mononoke::fbinit_test]
    fn test_merge_added_in_single_bookmark_update(fb: FacebookInit) -> Result<(), Error> {
        let runtime = Runtime::new()?;
        runtime.block_on(async move {
            let ctx = CoreContext::test_mock(fb);
            let (syncers, _, _, _) = init_small_large_repo(&ctx).await?;
            let commit_sync_data = syncers.small_to_large;
            let smallrepo = commit_sync_data.get_source_repo();

            // Merge new repo
            let first_new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("firstnewrepo", "newcontent")
                .commit()
                .await?;
            let second_new_repo = CreateCommitContext::new(&ctx, &smallrepo, vec![first_new_repo])
                .add_file("secondnewrepo", "anothercontent")
                .commit()
                .await?;

            let master_cs_id = resolve_cs_id(&ctx, &smallrepo, "master").await?;
            let merge =
                CreateCommitContext::new(&ctx, &smallrepo, vec![master_cs_id, second_new_repo])
                    .commit()
                    .await?;

            bookmark(&ctx, &smallrepo, "master").set_to(merge).await?;
            sync_and_validate(&ctx, &commit_sync_data).await?;

            Ok(())
        })
    }

    #[mononoke::fbinit_test]
    fn test_merge_of_a_merge_one_step(fb: FacebookInit) -> Result<(), Error> {
        let runtime = Runtime::new()?;
        runtime.block_on(async move {
            let ctx = CoreContext::test_mock(fb);
            let (syncers, _, _, _) = init_small_large_repo(&ctx).await?;
            let commit_sync_data = syncers.small_to_large;
            let smallrepo = commit_sync_data.get_source_repo();

            // Merge new repo, which itself has a merge
            let first_new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("firstnewrepo", "newcontent")
                .commit()
                .await?;
            let second_new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("secondnewrepo", "anothercontent")
                .commit()
                .await?;

            let merge_new_repo =
                CreateCommitContext::new(&ctx, &smallrepo, vec![first_new_repo, second_new_repo])
                    .commit()
                    .await?;

            let master_cs_id = resolve_cs_id(&ctx, &smallrepo, "master").await?;
            let merge =
                CreateCommitContext::new(&ctx, &smallrepo, vec![master_cs_id, merge_new_repo])
                    .commit()
                    .await?;

            bookmark(&ctx, &smallrepo, "master").set_to(merge).await?;
            sync_and_validate(&ctx, &commit_sync_data).await?;

            Ok(())
        })
    }

    #[mononoke::fbinit_test]
    fn test_merge_of_a_merge_two_steps(fb: FacebookInit) -> Result<(), Error> {
        let runtime = Runtime::new()?;
        runtime.block_on(async move {
            let ctx = CoreContext::test_mock(fb);
            let (syncers, _, _, _) = init_small_large_repo(&ctx).await?;
            let commit_sync_data = syncers.small_to_large;
            let smallrepo = commit_sync_data.get_source_repo();

            // Merge new repo, which itself has a merge
            let first_new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("firstnewrepo", "newcontent")
                .commit()
                .await?;
            let second_new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("secondnewrepo", "anothercontent")
                .commit()
                .await?;

            let merge_new_repo =
                CreateCommitContext::new(&ctx, &smallrepo, vec![first_new_repo, second_new_repo])
                    .commit()
                    .await?;
            bookmark(&ctx, &smallrepo, "newrepoimport")
                .set_to(merge_new_repo)
                .await?;
            let res = sync(
                &ctx,
                &commit_sync_data,
                &hashset! {BookmarkKey::new("master")?},
                PushrebaseRewriteDates::No,
            )
            .await?;
            assert_eq!(res.last(), Some(&SyncResult::SkippedNoKnownVersion));

            let merge = CreateCommitContext::new(&ctx, &smallrepo, vec!["master", "newrepoimport"])
                .commit()
                .await?;

            bookmark(&ctx, &smallrepo, "master").set_to(merge).await?;
            sync_and_validate_with_common_bookmarks(
                &ctx,
                &commit_sync_data,
                &hashset! {BookmarkKey::new("master")?},
                &hashset! {BookmarkKey::new("newrepoimport")?},
                PushrebaseRewriteDates::No,
            )
            .await?;

            Ok(())
        })
    }

    #[mononoke::fbinit_test]
    fn test_merge_non_shared_bookmark(fb: FacebookInit) -> Result<(), Error> {
        let runtime = Runtime::new()?;
        runtime.block_on(async move {
            let ctx = CoreContext::test_mock(fb);

            let (syncers, _, _, _) = init_small_large_repo(&ctx).await?;
            let commit_sync_data = syncers.small_to_large;
            let smallrepo = commit_sync_data.get_source_repo();

            let new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
                .add_file("firstnewrepo", "newcontent")
                .commit()
                .await?;
            bookmark(&ctx, &smallrepo, "newrepohead")
                .set_to(new_repo)
                .await?;
            let res = sync(
                &ctx,
                &commit_sync_data,
                &hashset! {BookmarkKey::new("master")?},
                PushrebaseRewriteDates::No,
            )
            .await?;
            assert_eq!(res.last(), Some(&SyncResult::SkippedNoKnownVersion));

            let merge = CreateCommitContext::new(&ctx, &smallrepo, vec!["master", "newrepohead"])
                .commit()
                .await?;

            bookmark(&ctx, &smallrepo, "somebook").set_to(merge).await?;
            assert!(
                sync_and_validate_with_common_bookmarks(
                    &ctx,
                    &commit_sync_data,
                    &hashset! {BookmarkKey::new("master")?},
                    &hashset! {BookmarkKey::new("newrepohead")?, BookmarkKey::new("somebook")?},
                    PushrebaseRewriteDates::No,
                )
                .await
                .is_err()
            );
            Ok(())
        })
    }

    #[mononoke::fbinit_test]
    async fn test_merge_different_versions(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, _, _, _) = init_small_large_repo(&ctx).await?;
        let commit_sync_data = syncers.small_to_large;
        let smallrepo = commit_sync_data.get_source_repo();

        // Merge new repo
        let new_repo = CreateCommitContext::new_root(&ctx, &smallrepo)
            .add_file("firstnewrepo", "newcontent")
            .commit()
            .await?;

        bookmark(&ctx, &smallrepo, "another_pushrebase_bookmark")
            .set_to("premove")
            .await?;
        sync_and_validate_with_common_bookmarks(
            &ctx,
            &commit_sync_data,
            &hashset! { BookmarkKey::new("master")?},
            &hashset! {},
            PushrebaseRewriteDates::No,
        )
        .await?;

        let merge = CreateCommitContext::new_root(&ctx, &smallrepo)
            .add_parent("premove")
            .add_parent(new_repo)
            .commit()
            .await?;
        bookmark(&ctx, &smallrepo, "another_pushrebase_bookmark")
            .set_to(merge)
            .await?;

        sync_and_validate_with_common_bookmarks(
             &ctx, &commit_sync_data,
             &hashset!{ BookmarkKey::new("master")?, BookmarkKey::new("another_pushrebase_bookmark")?},
             &hashset!{},
                 PushrebaseRewriteDates::No,
         ).await?;

        Ok(())
    }

    async fn sync_and_validate(
        ctx: &CoreContext,
        commit_sync_data: &CommitSyncData<TestRepo>,
    ) -> Result<(), Error> {
        sync_and_validate_with_common_bookmarks(
            ctx,
            commit_sync_data,
            &hashset! {BookmarkKey::new("master")?},
            &hashset! {},
            PushrebaseRewriteDates::No,
        )
        .await
    }

    async fn sync_and_validate_with_common_bookmarks(
        ctx: &CoreContext,
        commit_sync_data: &CommitSyncData<TestRepo>,
        common_pushrebase_bookmarks: &HashSet<BookmarkKey>,
        should_be_missing: &HashSet<BookmarkKey>,
        pushrebase_rewrite_dates: PushrebaseRewriteDates,
    ) -> Result<(), Error> {
        let smallrepo = commit_sync_data.get_source_repo();
        sync(
            ctx,
            commit_sync_data,
            common_pushrebase_bookmarks,
            pushrebase_rewrite_dates,
        )
        .await?;

        let actually_missing = find_bookmark_diff(ctx.clone(), commit_sync_data)
            .await?
            .into_iter()
            .map(|diff| diff.target_bookmark().clone())
            .collect::<HashSet<_>>();
        println!("actually missing bookmarks: {:?}", actually_missing);
        assert_eq!(&actually_missing, should_be_missing,);

        let heads: Vec<_> = smallrepo
            .bookmarks()
            .get_heads_maybe_stale(ctx.clone())
            .try_collect()
            .await?;
        for head in heads {
            println!("verifying working copy for {}", head);
            verify_working_copy(
                ctx,
                commit_sync_data,
                head,
                commit_sync_data.live_commit_sync_config.clone(),
            )
            .await?;
        }

        Ok(())
    }

    async fn sync(
        ctx: &CoreContext,
        commit_sync_data: &CommitSyncData<TestRepo>,
        common_pushrebase_bookmarks: &HashSet<BookmarkKey>,
        pushrebase_rewrite_dates: PushrebaseRewriteDates,
    ) -> Result<Vec<SyncResult>, Error> {
        let smallrepo = commit_sync_data.get_source_repo();
        let megarepo = commit_sync_data.get_target_repo();

        let counter = crate::format_counter(commit_sync_data);
        let start_from = megarepo
            .mutable_counters()
            .get_counter(ctx, &counter)
            .await?
            .unwrap_or(1);

        println!("start from: {}", start_from);
        let read_all = 65536;
        let log_entries: Vec<_> = smallrepo
            .bookmark_update_log()
            .read_next_bookmark_log_entries(
                ctx.clone(),
                start_from.try_into()?,
                read_all,
                Freshness::MostRecent,
            )
            .try_collect()
            .await?;

        println!(
            "syncing log entries {:?}  from repo#{} to repo#{}",
            log_entries,
            smallrepo.repo_identity().id(),
            megarepo.repo_identity().id()
        );

        let mut res = vec![];
        for entry in log_entries {
            let entry_id = entry.id.try_into()?;
            let single_res = sync_single_bookmark_update_log(
                ctx,
                commit_sync_data,
                entry,
                common_pushrebase_bookmarks,
                MononokeScubaSampleBuilder::with_discard(),
                pushrebase_rewrite_dates,
            )
            .await?;
            res.push(single_res);

            megarepo
                .mutable_counters()
                .set_counter(ctx, &counter, entry_id, None)
                .await?;
        }

        Ok(res)
    }
}
