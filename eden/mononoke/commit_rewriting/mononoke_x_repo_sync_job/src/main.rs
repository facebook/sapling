/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(auto_traits)]
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
use anyhow::format_err;
use backsyncer::format_counter as format_backsyncer_counter;
use bookmarks::BookmarkKey;
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
use regex::Regex;
use repo_derived_data::RepoDerivedDataRef;
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
            log_info(ctx, format!("{} is already synced", bcs_id));
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
        log_info(ctx, format!("{} is already synced", bcs));
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

    log_info(ctx, format!("successful sync of head {}", bcs));
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
            tail(
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

                let synced_something = tail(
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

                if !synced_something {
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
                                    "Bookmark {} does not exist in the large repo",
                                    target_bookmark
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

async fn tail(
    ctx: &CoreContext,
    commit_sync_data: &CommitSyncData<Arc<Repo>>,
    target_mutable_counters: &ArcMutableCounters,
    mut scuba_sample: MononokeScubaSampleBuilder,
    common_pushrebase_bookmarks: &HashSet<BookmarkKey>,
    backpressure_params: &BackpressureParams,
    derived_data_types: &[DerivableType],
    sleep_duration: Duration,
    maybe_bookmark_regex: &Option<Regex>,
    pushrebase_rewrite_dates: PushrebaseRewriteDates,
) -> Result<bool, Error> {
    let small_repo = commit_sync_data.get_source_repo();
    let bookmark_update_log = small_repo.bookmark_update_log();
    let counter = format_counter(commit_sync_data);

    let maybe_start_id = target_mutable_counters.get_counter(ctx, &counter).await?;
    let start_id = maybe_start_id.ok_or_else(|| format_err!("counter not found"))?;
    let limit = 10;
    let log_entries = bookmark_update_log
        .read_next_bookmark_log_entries(
            ctx.clone(),
            start_id.try_into()?,
            limit,
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
    log_info(ctx, format!("queue size is {}", remaining_entries));

    for entry in log_entries {
        let entry_id = entry.id;
        scuba_sample.add("entry_id", u64::from(entry.id));

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
                    .derive_bulk_locally(ctx, &synced_css, None, derived_data_types, None)
                    .await?;

                maybe_apply_backpressure(
                    ctx,
                    backpressure_params,
                    commit_sync_data.get_target_repo(),
                    scuba_sample.clone(),
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
                    entry.id, entry.bookmark_name,
                ),
            );
            let mut scuba_sample = scuba_sample.clone();
            scuba_sample.add("source_bookmark_name", format!("{}", entry.bookmark_name));
            scuba_sample.add("skipped", true);
            scuba_sample.log();
        }

        // Note that updating the counter might fail after successful sync of the commits.
        // This is expected - next run will try to update the counter again without
        // re-syncing the commits.
        target_mutable_counters
            .set_counter(ctx, &counter, entry_id.try_into()?, None)
            .await?;
    }
    Ok(true)
}

async fn maybe_apply_backpressure(
    ctx: &CoreContext,
    backpressure_params: &BackpressureParams,
    large_repo: &Repo,
    scuba_sample: MononokeScubaSampleBuilder,
    sleep_duration: Duration,
) -> Result<(), Error> {
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
                            log_debug(ctx, format!("repo {}, counter {}", repo_id, counter));
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
                                format!("backsyncer counter not found for repo {}!", repo_id),
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
    format!("xreposync_from_{}", source_repo_id)
}

async fn async_main(app: MononokeApp, ctx: CoreContext) -> Result<(), Error> {
    let args: Arc<ForwardSyncerArgs> = Arc::new(app.args()?);
    let app = Arc::new(app);
    let ctx = Arc::new(ctx);
    let repo_args = args.repo_args.clone();
    let runtime = app.runtime().clone();
    let logger = app.logger().clone();
    let res = if let Some(executor) = args.sharded_executor_args.clone().build_executor(
        app.fb,
        runtime.clone(),
        &logger,
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
        executor.block_and_execute(&logger, receiver).await?;
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

    let ctx = session_container.new_context(app.logger().clone(), scuba);

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
