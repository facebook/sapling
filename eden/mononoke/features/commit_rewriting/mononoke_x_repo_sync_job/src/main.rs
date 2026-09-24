/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

//! Mononoke Cross Repo sync job
//!
//! This is a special job used to tail "small" Mononoke repo into "large" Mononoke repo when
//! small repo is a source of truth (i.e. "hg push" go directly to small repo).
//! At the moment there two main limitations:
//! 1) Syncing of some merge commits is not supported
//! 2) Root commits and their descendants that are not merged into a main line
//!    aren't going to be synced. For example,
//
//! ```text
//!   O <- main bookmark
//!   |
//!   O
//!   |   A <- new_bookmark, that added a new root commit
//!   O   |
//!    ...
//!
//!   Commit A, its ancestors and new_bookmark aren't going to be synced to the large repo.
//!   However if commit A gets merged into a mainline e.g.
//!   O <- main bookmark
//!   | \
//!   O  \
//!   |   A <- new_bookmark, that added a new root commit
//!   O   |
//!    ...
//!
//!   Then commit A and all of its ancestors WILL be synced to the large repo, however
//!   new_bookmark still WILL NOT be synced to the large repo.
//!
//! This job does tailing by following bookmark update log of the small repo and replaying
//! each commit into the large repo. Note that some bookmarks called "common_pushrebase_bookmarks"
//! are treated specially, see comments in the code for more details
//! ```

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::format_err;
use backsyncer::advance_bookmark_counter;
use backsyncer::format_counter as format_backsyncer_counter;
use backsyncer::get_bookmark_counter_or_initialize;
use backsyncer::list_publishing_bookmarks;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::BookmarkUpdateLogRef;
use bookmarks::BookmarksRef;
use bookmarks::Freshness;
use bulk_derivation::BulkDerivation;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use context::CoreContext;
use context::SessionContainer;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncData;
use cross_repo_sync::ConcreteRepo as CrossRepo;
use cross_repo_sync::PushrebaseRewriteDates;
use cross_repo_sync::Source;
use cross_repo_sync::Target;
use cross_repo_sync::log_debug;
use cross_repo_sync::log_error;
use cross_repo_sync::log_info;
use cross_repo_sync::log_warning;
use cross_repo_sync::sync_commit;
use executor_lib::RepoShardedProcessExecutor;
use fbinit::FacebookInit;
use futures::FutureExt;
use futures::StreamExt;
use futures::future;
use futures::stream;
use futures::stream::TryStreamExt;
use futures_stats::TimedFutureExt;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use live_commit_sync_config::LiveCommitSyncConfig;
use metaconfig_types::CommitSyncConfigVersion;
use metadata::Metadata;
use mononoke_api::Repo;
use mononoke_app::MononokeApp;
use mononoke_app::args::MultiRepoArgs;
use mononoke_app::monitoring::AliveService;
use mononoke_types::ChangesetId;
use mononoke_types::DerivableType;
use mutable_counters::ArcMutableCounters;
use mutable_counters::MutableCountersRef;
use mutable_counters::validate_counter_name;
use regex::Regex;
use repo_identity::RepoIdentityRef;
use scuba_ext::MononokeScubaSampleBuilder;
use sharding::XRepoSyncProcess;
use sharding::XRepoSyncProcessExecutor;

use crate::cli::ForwardSyncerArgs;
use crate::cli::TailCommandArgs;
use crate::sync::SyncResult;

mod cli;
mod reporting;
mod sharding;
mod sync;

use crate::cli::create_app;
use crate::reporting::log_bookmark_update_result;
use crate::reporting::log_noop_iteration;
use crate::sync::sync_commit_and_ancestors;
use crate::sync::sync_commits_for_initial_import;
use crate::sync::sync_single_bookmark_update_log;

const SM_CLEANUP_TIMEOUT_SECS: u64 = 60;
const BOOKMARK_POLLING_JUST_KNOB: &str = "scm/mononoke:forward_syncer_bookmark_polling";
// Keep per-repo fan-out conservatively bounded. The mode-level JK is the
// operational rollback control; changing this safety cap requires code review.
const BOOKMARK_POLLING_CONCURRENCY: usize = 10;
const TAIL_BATCH_SIZE: u64 = 10;

#[derive(Default)]
struct TailIterationOutcome {
    made_progress: bool,
    failed_bookmarks: usize,
}

/// Sync and all of its unsynced ancestors **if the given commit has at least
/// one synced ancestor**.
async fn run_in_single_sync_mode(
    ctx: &CoreContext,
    bcs_ids: Vec<ChangesetId>,
    commit_sync_data: CommitSyncData<Arc<Repo>>,
    scuba_sample: MononokeScubaSampleBuilder,
    mb_target_bookmark: Option<BookmarkKey>,
    common_bookmarks: HashSet<BookmarkKey>,
    pushrebase_rewrite_dates: PushrebaseRewriteDates,
    new_version: Option<CommitSyncConfigVersion>,
    unsafe_force_rewrite_parent_to_target_bookmark: bool,
) -> Result<(), Error> {
    let mb_target_bookmark = mb_target_bookmark.map(Target);

    log_info(
        ctx,
        format!(
            "Syncing {} commits and all of their unsynced ancestors",
            bcs_ids.len()
        ),
    );

    for bcs_id in bcs_ids {
        log_info(
            ctx,
            format!(
                "Checking if {} is already synced {}->{}",
                bcs_id,
                commit_sync_data
                    .repos
                    .get_source_repo()
                    .repo_identity()
                    .id(),
                commit_sync_data
                    .repos
                    .get_target_repo()
                    .repo_identity()
                    .id(),
            ),
        );
        if commit_sync_data
            .commit_sync_outcome_exists(ctx, Source(bcs_id))
            .await?
        {
            log_info(ctx, format!("{bcs_id} is already synced"));
            continue;
        }

        let res = sync_commit_and_ancestors(
            ctx,
            &commit_sync_data,
            None, // from_cs_id,
            bcs_id,
            &mb_target_bookmark,
            &common_bookmarks,
            scuba_sample.clone(),
            pushrebase_rewrite_dates,
            None,
            &new_version,
            unsafe_force_rewrite_parent_to_target_bookmark,
        )
        .await;

        if res.is_ok() {
            log_info(ctx, "successful sync");
        }
        res.map(|_| ())?
    }

    Ok(())
}

async fn run_in_initial_import_mode_for_single_head(
    ctx: &CoreContext,
    bcs: ChangesetId,
    commit_sync_data: &CommitSyncData<Arc<Repo>>,
    config_version: CommitSyncConfigVersion,
    scuba_sample: MononokeScubaSampleBuilder,
    disable_progress_bar: bool,
    no_automatic_derivation: bool,
    derivation_batch_size: usize,
    add_mapping_to_hg_extra: bool,
) -> Result<()> {
    log_info(
        ctx,
        format!(
            "Checking if {} is already synced {}->{}",
            bcs,
            commit_sync_data
                .repos
                .get_source_repo()
                .repo_identity()
                .id(),
            commit_sync_data
                .repos
                .get_target_repo()
                .repo_identity()
                .id()
        ),
    );
    if commit_sync_data
        .commit_sync_outcome_exists(ctx, Source(bcs))
        .await?
    {
        log_info(ctx, format!("{bcs} is already synced"));
        return Ok(());
    }
    let res = sync_commits_for_initial_import(
        ctx,
        commit_sync_data,
        scuba_sample.clone(),
        bcs,
        config_version,
        disable_progress_bar,
        no_automatic_derivation,
        derivation_batch_size,
        add_mapping_to_hg_extra,
    )
    .await;

    if let Err(e) = res {
        log_error(ctx, format!("Initial import failed: {e:#?}"));
        return Err(e);
    }

    log_info(ctx, format!("successful sync of head {bcs}"));
    Ok(())
}

/// Run the initial import of a small repo into a large repo.
/// It will sync a specific commit (i.e. head commit) and all of its ancestors
/// if commit is notprovided
async fn run_in_initial_import_mode(
    ctx: &CoreContext,
    bcs_ids: Vec<ChangesetId>,
    commit_sync_data: CommitSyncData<Arc<Repo>>,
    config_version: CommitSyncConfigVersion,
    scuba_sample: MononokeScubaSampleBuilder,
    disable_progress_bar: bool,
    no_automatic_derivation: bool,
    derivation_batch_size: usize,
    add_mapping_to_hg_extra: bool,
) -> Result<()> {
    for bcs_id in bcs_ids {
        run_in_initial_import_mode_for_single_head(
            ctx,
            bcs_id,
            &commit_sync_data,
            config_version.clone(),
            scuba_sample.clone(),
            disable_progress_bar,
            no_automatic_derivation,
            derivation_batch_size,
            add_mapping_to_hg_extra,
        )
        .await?;
    }
    Ok(())
}

enum TailingArgs<R> {
    CatchUpOnce(CommitSyncData<R>),
    LoopForever(CommitSyncData<R>),
}

async fn run_in_tailing_mode(
    ctx: &CoreContext,
    target_mutable_counters: ArcMutableCounters,
    common_pushrebase_bookmarks: HashSet<BookmarkKey>,
    base_scuba_sample: MononokeScubaSampleBuilder,
    backpressure_params: BackpressureParams,
    derived_data_types: Vec<DerivableType>,
    tailing_args: TailingArgs<Arc<Repo>>,
    sleep_duration: Duration,
    maybe_bookmark_regex: Option<Regex>,
    pushrebase_rewrite_dates: PushrebaseRewriteDates,
    live_commit_sync_config: Arc<CfgrLiveCommitSyncConfig>,
) -> Result<(), Error> {
    match tailing_args {
        TailingArgs::CatchUpOnce(commit_sync_data) => {
            let scuba_sample = MononokeScubaSampleBuilder::with_discard();
            let outcome = tail_iteration(
                ctx,
                &commit_sync_data,
                &target_mutable_counters,
                scuba_sample,
                &common_pushrebase_bookmarks,
                &backpressure_params,
                &derived_data_types,
                sleep_duration,
                &maybe_bookmark_regex,
                pushrebase_rewrite_dates,
            )
            .boxed()
            .await?;
            if outcome.failed_bookmarks > 0 {
                bail!(
                    "{} forward-sync bookmark workers failed",
                    outcome.failed_bookmarks
                );
            }
        }
        TailingArgs::LoopForever(commit_sync_data) => {
            let source_repo_id = commit_sync_data.get_source_repo().repo_identity().id();

            loop {
                let scuba_sample = base_scuba_sample.clone();
                // We only care about public pushes because draft pushes are not in the bookmark
                // update log at all.
                let enabled = live_commit_sync_config
                    .push_redirector_enabled_for_public(ctx, source_repo_id)
                    .await?;

                // Pushredirection is enabled - we need to disable forward sync in that case
                if enabled {
                    log_noop_iteration(scuba_sample);
                    tokio::time::sleep(sleep_duration).await;
                    continue;
                }

                let outcome = tail_iteration(
                    ctx,
                    &commit_sync_data,
                    &target_mutable_counters,
                    scuba_sample.clone(),
                    &common_pushrebase_bookmarks,
                    &backpressure_params,
                    &derived_data_types,
                    sleep_duration,
                    &maybe_bookmark_regex,
                    pushrebase_rewrite_dates,
                )
                .boxed()
                .await?;

                if outcome.failed_bookmarks > 0 {
                    log_error(
                        ctx,
                        format!(
                            "{} forward-sync bookmark workers failed for {} -> {}; retrying after the configured polling delay",
                            outcome.failed_bookmarks,
                            commit_sync_data.get_source_repo().repo_identity().name(),
                            commit_sync_data.get_target_repo().repo_identity().name(),
                        ),
                    );
                    tokio::time::sleep(sleep_duration).await;
                } else if !outcome.made_progress {
                    log_noop_iteration(scuba_sample);
                    // Maintain the working copy equivalence mapping so we don't build up a backlog
                    for target_bookmark in common_pushrebase_bookmarks.iter() {
                        let target_bookmark_value = commit_sync_data
                            .get_large_repo()
                            .bookmarks()
                            .get(
                                ctx.clone(),
                                target_bookmark,
                                bookmarks::Freshness::MostRecent,
                            )
                            .await?
                            .ok_or_else(|| {
                                anyhow!(
                                    "Bookmark {target_bookmark} does not exist in the large repo"
                                )
                            })?;

                        sync_commit(
                            ctx,
                            target_bookmark_value,
                            &commit_sync_data.reverse(),
                            CandidateSelectionHint::Only,
                            CommitSyncContext::XRepoSyncJob,
                            false,
                        )
                        .await?;
                    }

                    tokio::time::sleep(sleep_duration).await;
                }
            }
        }
    }

    Ok(())
}

async fn tail_iteration<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    target_mutable_counters: &ArcMutableCounters,
    scuba_sample: MononokeScubaSampleBuilder,
    common_pushrebase_bookmarks: &HashSet<BookmarkKey>,
    backpressure_params: &BackpressureParams,
    derived_data_types: &[DerivableType],
    sleep_duration: Duration,
    maybe_bookmark_regex: &Option<Regex>,
    pushrebase_rewrite_dates: PushrebaseRewriteDates,
) -> Result<TailIterationOutcome, Error>
where
    R: crate::sync::Repo,
{
    let bookmark_polling_enabled = justknobs::eval(
        BOOKMARK_POLLING_JUST_KNOB,
        None,
        Some(commit_sync_data.get_source_repo().repo_identity().name()),
    );
    if bookmark_polling_enabled {
        tail_by_bookmark(
            ctx,
            commit_sync_data,
            target_mutable_counters,
            scuba_sample,
            common_pushrebase_bookmarks,
            backpressure_params,
            derived_data_types,
            sleep_duration,
            maybe_bookmark_regex,
            pushrebase_rewrite_dates,
        )
        .await
    } else {
        Ok(TailIterationOutcome {
            made_progress: tail(
                ctx,
                commit_sync_data,
                target_mutable_counters,
                scuba_sample,
                common_pushrebase_bookmarks,
                backpressure_params,
                derived_data_types,
                sleep_duration,
                maybe_bookmark_regex,
                pushrebase_rewrite_dates,
            )
            .await?,
            failed_bookmarks: 0,
        })
    }
}

async fn tail<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    target_mutable_counters: &ArcMutableCounters,
    mut scuba_sample: MononokeScubaSampleBuilder,
    common_pushrebase_bookmarks: &HashSet<BookmarkKey>,
    backpressure_params: &BackpressureParams,
    derived_data_types: &[DerivableType],
    sleep_duration: Duration,
    maybe_bookmark_regex: &Option<Regex>,
    pushrebase_rewrite_dates: PushrebaseRewriteDates,
) -> Result<bool, Error>
where
    R: crate::sync::Repo,
{
    let small_repo = commit_sync_data.get_source_repo();
    let bookmark_update_log = small_repo.bookmark_update_log();
    let counter = format_counter(commit_sync_data);

    let maybe_start_id = target_mutable_counters.get_counter(ctx, &counter).await?;
    let start_id = maybe_start_id.ok_or_else(|| format_err!("counter not found"))?;
    let log_entries = bookmark_update_log
        .read_next_bookmark_log_entries(
            ctx.clone(),
            start_id.try_into()?,
            TAIL_BATCH_SIZE,
            Freshness::MaybeStale,
        )
        .try_collect::<Vec<_>>()
        .await?;

    let remaining_entries = commit_sync_data
        .get_source_repo()
        .bookmark_update_log()
        .count_further_bookmark_log_entries(ctx.clone(), start_id.try_into()?, None)
        .boxed()
        .await?;

    if log_entries.is_empty() {
        log_noop_iteration(scuba_sample.clone());
        return Ok(false);
    };

    scuba_sample.add("queue_size", remaining_entries);
    log_info(ctx, format!("queue size is {remaining_entries}"));

    for entry in log_entries {
        let entry_id = entry.id;
        if bookmark_counter_covers_entry(ctx, commit_sync_data, target_mutable_counters, &entry)
            .await?
        {
            target_mutable_counters
                .set_counter(ctx, &counter, entry_id.try_into()?, None)
                .await?;
            continue;
        }

        sync_bookmark_update_log_entry(
            ctx,
            commit_sync_data,
            entry,
            common_pushrebase_bookmarks,
            backpressure_params,
            derived_data_types,
            sleep_duration,
            maybe_bookmark_regex,
            pushrebase_rewrite_dates,
            scuba_sample.clone(),
        )
        .await?;

        // Note that updating the counter might fail after successful sync of the commits.
        // This is expected - next run will try to update the counter again without
        // re-syncing the commits.
        target_mutable_counters
            .set_counter(ctx, &counter, entry_id.try_into()?, None)
            .await?;
    }
    Ok(true)
}

async fn tail_by_bookmark<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    target_mutable_counters: &ArcMutableCounters,
    scuba_sample: MononokeScubaSampleBuilder,
    common_pushrebase_bookmarks: &HashSet<BookmarkKey>,
    backpressure_params: &BackpressureParams,
    derived_data_types: &[DerivableType],
    sleep_duration: Duration,
    maybe_bookmark_regex: &Option<Regex>,
    pushrebase_rewrite_dates: PushrebaseRewriteDates,
) -> Result<TailIterationOutcome, Error>
where
    R: crate::sync::Repo,
{
    let global_counter_name = format_counter(commit_sync_data);
    let global_counter: BookmarkUpdateLogId = target_mutable_counters
        .get_counter(ctx, &global_counter_name)
        .await?
        .ok_or_else(|| format_err!("counter not found"))?
        .try_into()?;
    let bookmarks = list_publishing_bookmarks(
        ctx,
        commit_sync_data.get_source_repo(),
        &BookmarkPrefix::empty(),
    )
    .await?;
    let results = stream::iter(bookmarks)
        .map(|bookmark| {
            let scuba_sample = scuba_sample.clone();
            async move {
                let result = tail_bookmark(
                    ctx,
                    commit_sync_data,
                    target_mutable_counters,
                    bookmark.clone(),
                    global_counter,
                    common_pushrebase_bookmarks,
                    backpressure_params,
                    derived_data_types,
                    sleep_duration,
                    maybe_bookmark_regex,
                    pushrebase_rewrite_dates,
                    scuba_sample,
                )
                .await;
                (bookmark, result)
            }
        })
        .buffer_unordered(BOOKMARK_POLLING_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

    let mut outcome = TailIterationOutcome::default();
    for (bookmark, result) in results {
        match result {
            Ok(made_progress) => outcome.made_progress |= made_progress,
            Err(error) => {
                log_error(
                    ctx,
                    format!("Failed to forward-sync bookmark {bookmark}: {error:#}"),
                );
                outcome.failed_bookmarks += 1;
            }
        }
    }
    Ok(outcome)
}

async fn tail_bookmark<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    target_mutable_counters: &ArcMutableCounters,
    bookmark: BookmarkKey,
    global_counter_snapshot: BookmarkUpdateLogId,
    common_pushrebase_bookmarks: &HashSet<BookmarkKey>,
    backpressure_params: &BackpressureParams,
    derived_data_types: &[DerivableType],
    sleep_duration: Duration,
    maybe_bookmark_regex: &Option<Regex>,
    pushrebase_rewrite_dates: PushrebaseRewriteDates,
    mut scuba_sample: MononokeScubaSampleBuilder,
) -> Result<bool, Error>
where
    R: crate::sync::Repo,
{
    let counter_name = format_bookmark_counter(commit_sync_data, &bookmark)?;
    let Some(mut counter) = get_bookmark_counter_or_initialize(
        ctx,
        target_mutable_counters,
        &counter_name,
        global_counter_snapshot,
    )
    .await?
    else {
        // A competing worker initialized this stream. Retry from the durable
        // value on the next polling iteration instead of competing with it.
        return Ok(false);
    };

    let entries = commit_sync_data
        .get_source_repo()
        .bookmark_update_log()
        .read_next_bookmark_log_entries_by_bookmark(
            ctx.clone(),
            bookmark,
            counter,
            TAIL_BATCH_SIZE,
            Freshness::MaybeStale,
        )
        .try_collect::<Vec<_>>()
        .await?;
    if entries.is_empty() {
        return Ok(false);
    }

    scuba_sample.add("queue_size", entries.len());
    for entry in entries {
        let entry_id = entry.id;
        // A competing worker may have advanced the durable counter past
        // entries that were already fetched in this batch. Do not replay
        // entries that the newly observed cursor already covers.
        if entry_id <= counter {
            continue;
        }
        // An existing per-bookmark cursor can lag after bookmark polling was
        // disabled and the legacy global loop advanced. Such entries were
        // already processed in global order, so reconcile this cursor one
        // exact bookmark entry at a time instead of replaying the update.
        if entry_id > global_counter_snapshot {
            sync_bookmark_update_log_entry(
                ctx,
                commit_sync_data,
                entry,
                common_pushrebase_bookmarks,
                backpressure_params,
                derived_data_types,
                sleep_duration,
                maybe_bookmark_regex,
                pushrebase_rewrite_dates,
                scuba_sample.clone(),
            )
            .await?;
        }
        advance_bookmark_counter(
            ctx,
            target_mutable_counters,
            &counter_name,
            &mut counter,
            entry_id,
        )
        .await?;
    }

    Ok(true)
}

async fn bookmark_counter_covers_entry<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    target_mutable_counters: &ArcMutableCounters,
    entry: &BookmarkUpdateLogEntry,
) -> Result<bool, Error>
where
    R: crate::sync::Repo,
{
    let counter_name = format_bookmark_counter(commit_sync_data, &entry.bookmark_name)?;
    let counter = target_mutable_counters
        .get_counter(ctx, &counter_name)
        .await?
        .map(BookmarkUpdateLogId::try_from)
        .transpose()?;
    Ok(counter.is_some_and(|counter| counter >= entry.id))
}

async fn sync_bookmark_update_log_entry<R>(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<R>,
    entry: BookmarkUpdateLogEntry,
    common_pushrebase_bookmarks: &HashSet<BookmarkKey>,
    backpressure_params: &BackpressureParams,
    derived_data_types: &[DerivableType],
    sleep_duration: Duration,
    maybe_bookmark_regex: &Option<Regex>,
    pushrebase_rewrite_dates: PushrebaseRewriteDates,
    mut scuba_sample: MononokeScubaSampleBuilder,
) -> Result<(), Error>
where
    R: crate::sync::Repo,
{
    let entry_id = entry.id;
    scuba_sample.add("entry_id", u64::from(entry_id));

    let skip = maybe_bookmark_regex
        .as_ref()
        .is_some_and(|regex| !regex.is_match(entry.bookmark_name.as_str()));

    if !skip {
        let (stats, res) = sync_single_bookmark_update_log(
            ctx,
            commit_sync_data,
            entry,
            common_pushrebase_bookmarks,
            scuba_sample.clone(),
            pushrebase_rewrite_dates,
        )
        .timed()
        .await;

        log_bookmark_update_result(ctx, entry_id, scuba_sample.clone(), &res, stats);
        let maybe_synced_css = res?;

        if let SyncResult::Synced(synced_css) = maybe_synced_css {
            commit_sync_data
                .get_target_repo()
                .repo_derived_data()
                .manager()
                .derive_bulk_locally(ctx, &synced_css, None, derived_data_types, None, None)
                .await?;

            maybe_apply_backpressure(
                ctx,
                backpressure_params,
                commit_sync_data.get_target_repo(),
                scuba_sample,
                sleep_duration,
            )
            .boxed()
            .await?;
        }
    } else {
        log_info(
            ctx,
            format!(
                "skipping log entry #{} for {}",
                entry_id, entry.bookmark_name,
            ),
        );
        scuba_sample.add("source_bookmark_name", format!("{}", entry.bookmark_name));
        scuba_sample.add("skipped", true);
        scuba_sample.log();
    }

    Ok(())
}

async fn maybe_apply_backpressure<R>(
    ctx: &CoreContext,
    backpressure_params: &BackpressureParams,
    large_repo: &R,
    scuba_sample: MononokeScubaSampleBuilder,
    sleep_duration: Duration,
) -> Result<(), Error>
where
    R: RepoIdentityRef,
{
    let large_repo_id = large_repo.repo_identity().id();
    let limit = 10;
    loop {
        let max_further_entries = stream::iter(&backpressure_params.backsync_repos)
            .map(Ok)
            .map_ok(|repo| {
                async move {
                    let repo_id = repo.repo_identity().id();
                    let backsyncer_counter = format_backsyncer_counter(&large_repo_id);
                    let maybe_counter = repo
                        .mutable_counters()
                        .get_counter(ctx, &backsyncer_counter)
                        .boxed()
                        .await?
                        .map(|counter| counter.try_into())
                        .transpose()?;

                    match maybe_counter {
                        Some(counter) => {
                            let bookmark_update_log = repo.bookmark_update_log();
                            log_debug(ctx, format!("repo {repo_id}, counter {counter}"));
                            bookmark_update_log
                                .count_further_bookmark_log_entries(
                                    ctx.clone(),
                                    counter,
                                    None, // exclude_reason
                                )
                                .await
                        }
                        None => {
                            log_warning(
                                ctx,
                                format!("backsyncer counter not found for repo {repo_id}!"),
                            );
                            Ok(0)
                        }
                    }
                }
            })
            .try_buffer_unordered(100)
            .try_fold(0, |acc, x| future::ready(Ok(::std::cmp::max(acc, x))))
            .boxed()
            .await?;

        if max_further_entries > limit {
            reporting::log_backpressure(ctx, max_further_entries, scuba_sample.clone());
            tokio::time::sleep(sleep_duration).await;
        } else {
            break;
        }
    }

    Ok(())
}

fn format_counter<R>(commit_sync_data: &CommitSyncData<R>) -> String
where
    R: RepoIdentityRef + cross_repo_sync::Repo,
{
    let source_repo_id = commit_sync_data.get_source_repo_id();
    format!("xreposync_from_{source_repo_id}")
}

fn format_bookmark_counter<R>(
    commit_sync_data: &CommitSyncData<R>,
    bookmark: &BookmarkKey,
) -> Result<String>
where
    R: RepoIdentityRef + cross_repo_sync::Repo,
{
    let source_repo_id = commit_sync_data.get_source_repo_id();
    let counter_name = format!(
        "xreposync_by_bookmark_v1_{}_{}_{}",
        source_repo_id.id(),
        bookmark.category(),
        bookmark.as_str(),
    );
    validate_counter_name(&counter_name)?;
    Ok(counter_name)
}

async fn async_main(app: MononokeApp, ctx: CoreContext) -> Result<(), Error> {
    let args: Arc<ForwardSyncerArgs> = Arc::new(app.args()?);
    let app = Arc::new(app);
    let ctx = Arc::new(ctx);
    let repo_args = args.repo_args.clone();
    let runtime = app.runtime().clone();
    let res = if let Some(executor) = args.sharded_executor_args.clone().build_executor(
        app.fb,
        runtime.clone(),
        || {
            Arc::new(XRepoSyncProcess::new(
                ctx.clone(),
                app.clone(),
                args.clone(),
            ))
        },
        true, // enable shard (repo) level healing
        SM_CLEANUP_TIMEOUT_SECS,
    )? {
        let (sender, receiver) = tokio::sync::oneshot::channel::<bool>();
        executor.block_and_execute(receiver).await?;
        drop(sender);
        Ok(())
    } else {
        let repo_args = repo_args
            .into_source_and_target_args()
            .context("Source and Target repos must be provided when running in non-sharded mode")?;
        let x_repo_process_executor =
            XRepoSyncProcessExecutor::new(app, ctx.clone(), args, &repo_args).await?;
        x_repo_process_executor.execute().await
    };

    if let Err(ref e) = res {
        let mut scuba = ctx.scuba().clone();
        scuba.log_with_msg("Execution error", e.to_string());
    }
    res
}

struct BackpressureParams {
    backsync_repos: Vec<CrossRepo>,
}

impl BackpressureParams {
    async fn new(app: &MononokeApp, tail_cmd_args: TailCommandArgs) -> Result<Self, Error> {
        let multi_repo_args = MultiRepoArgs {
            repo_id: tail_cmd_args.backsync_pressure_repo_ids,
            repo_name: vec![],
        };
        let backsync_repos = app.open_repos(&multi_repo_args).await?;

        Ok(Self { backsync_repos })
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = create_app(fb)?;

    let mut metadata = Metadata::default();
    metadata.add_client_info(ClientInfo::default_with_entry_point(
        ClientEntryPoint::MegarepoForwardsyncer,
    ));

    let mut scuba = app.environment().scuba_sample_builder.clone();
    scuba.add_metadata(&metadata);

    let session_container = SessionContainer::builder(fb)
        .metadata(Arc::new(metadata))
        .build();

    let ctx = session_container.new_context(scuba);

    log_info(
        &ctx,
        format!("Starting session with id {}", ctx.metadata().session_id(),),
    );

    app.run_with_monitoring_and_logging(
        |app| async_main(app, ctx.clone()),
        "x_repo_sync_job",
        AliveService,
    )
}

#[cfg(test)]
mod test {
    use cross_repo_sync::CommitSyncData;
    use cross_repo_sync::test_utils::TestRepo;
    use cross_repo_sync::test_utils::init_small_large_repo;
    use fbinit::FacebookInit;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use justknobs::test_helpers::with_just_knobs_async;
    use maplit::hashmap;
    use maplit::hashset;
    use mononoke_macros::mononoke;
    use mutable_counters::MAX_COUNTER_NAME_LENGTH;
    use mutable_counters::MutableCountersArc;
    use tests_utils::CreateCommitContext;
    use tests_utils::bookmark;

    use super::*;

    #[mononoke::fbinit_test]
    async fn bookmark_tailing_syncs_independent_streams_and_supports_rollback(
        fb: FacebookInit,
    ) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let (syncers, _, _, _) = init_small_large_repo(&ctx).await?;
        let commit_sync_data: CommitSyncData<TestRepo> = syncers.small_to_large;
        let small_repo = commit_sync_data.get_source_repo();
        let large_repo = commit_sync_data.get_target_repo();
        let counters = large_repo.mutable_counters_arc();
        let global_counter_name = format_counter(&commit_sync_data);
        // Bookmark names persisted by dbbookmarks are capped at 512 bytes, so
        // every discovered bookmark fits in the mutable-counter schema.
        let longest_persisted_bookmark = BookmarkKey::new("a".repeat(512))?;
        assert!(
            format_bookmark_counter(&commit_sync_data, &longest_persisted_bookmark)?.len()
                <= MAX_COUNTER_NAME_LENGTH
        );
        let initial_global_counter: BookmarkUpdateLogId = small_repo
            .bookmark_update_log()
            .get_largest_log_id(ctx.clone(), Freshness::MostRecent)
            .await?
            .expect("fixture has bookmark update-log entries")
            .into();
        counters
            .set_counter(
                &ctx,
                &global_counter_name,
                initial_global_counter.try_into()?,
                None,
            )
            .await?;

        let first_bookmark = BookmarkKey::new("forward_bookmark_one")?;
        let second_bookmark = BookmarkKey::new("forward_bookmark_two")?;
        let filtered_bookmark = BookmarkKey::new("filtered_bookmark")?;
        for (bookmark_key, file) in [
            (&first_bookmark, "forward-one"),
            (&second_bookmark, "forward-two"),
            (&filtered_bookmark, "filtered"),
        ] {
            let changeset = CreateCommitContext::new(&ctx, small_repo, vec!["master"])
                .add_file(file, file)
                .commit()
                .await?;
            bookmark(&ctx, small_repo, bookmark_key.as_str())
                .set_to(changeset)
                .await?;
        }

        let common_bookmarks = hashset! { BookmarkKey::new("master")? };
        let backpressure_params = BackpressureParams {
            backsync_repos: vec![],
        };
        let jk = JustKnobsInMemory::new(hashmap! {
            BOOKMARK_POLLING_JUST_KNOB.to_string() => KnobVal::Bool(true),
        });
        let bookmark_regex = Some(Regex::new("^forward_bookmark_")?);
        let outcome = with_just_knobs_async(
            jk,
            tail_iteration(
                &ctx,
                &commit_sync_data,
                &counters,
                MononokeScubaSampleBuilder::with_discard(),
                &common_bookmarks,
                &backpressure_params,
                &[],
                Duration::ZERO,
                &bookmark_regex,
                PushrebaseRewriteDates::No,
            )
            .boxed(),
        )
        .await?;
        assert!(outcome.made_progress);
        assert_eq!(outcome.failed_bookmarks, 0);

        for bookmark in [&first_bookmark, &second_bookmark, &filtered_bookmark] {
            let source_log_id = small_repo
                .bookmark_update_log()
                .read_next_bookmark_log_entries_by_bookmark(
                    ctx.clone(),
                    bookmark.clone(),
                    BookmarkUpdateLogId(0),
                    u64::MAX,
                    Freshness::MostRecent,
                )
                .try_collect::<Vec<_>>()
                .await?
                .last()
                .expect("new bookmark has an update-log entry")
                .id;
            assert_eq!(
                counters
                    .get_counter(&ctx, &format_bookmark_counter(&commit_sync_data, bookmark)?)
                    .await?,
                Some(source_log_id.try_into()?),
            );
        }

        for bookmark in [&first_bookmark, &second_bookmark] {
            let target_bookmark = commit_sync_data
                .rename_bookmark(&Source((*bookmark).clone()))
                .await?
                .expect("test bookmark is mapped");
            assert!(
                large_repo
                    .bookmarks()
                    .get(ctx.clone(), &target_bookmark, Freshness::MostRecent)
                    .await?
                    .is_some()
            );
        }
        let filtered_target_bookmark = commit_sync_data
            .rename_bookmark(&Source(filtered_bookmark.clone()))
            .await?
            .expect("test bookmark is mapped");
        assert!(
            large_repo
                .bookmarks()
                .get(
                    ctx.clone(),
                    &filtered_target_bookmark,
                    Freshness::MostRecent,
                )
                .await?
                .is_none()
        );

        // Bookmark mode deliberately leaves the legacy cursor untouched.
        assert_eq!(
            counters.get_counter(&ctx, &global_counter_name).await?,
            Some(initial_global_counter.try_into()?),
        );
        // The legacy path recognizes the durable per-bookmark progress and
        // advances without replaying those entries during rollback.
        assert!(
            tail(
                &ctx,
                &commit_sync_data,
                &counters,
                MononokeScubaSampleBuilder::with_discard(),
                &common_bookmarks,
                &backpressure_params,
                &[],
                Duration::ZERO,
                &None,
                PushrebaseRewriteDates::No,
            )
            .await?
        );
        assert!(
            counters
                .get_counter(&ctx, &global_counter_name)
                .await?
                .expect("global counter exists")
                > initial_global_counter.try_into()?
        );
        // Widening the regex during rollback does not replay an entry whose
        // per-bookmark cursor was already advanced while it was filtered.
        assert!(
            large_repo
                .bookmarks()
                .get(
                    ctx.clone(),
                    &filtered_target_bookmark,
                    Freshness::MostRecent,
                )
                .await?
                .is_none()
        );

        Ok(())
    }
}
