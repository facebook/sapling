/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(optin_builtin_traits)]
#![deny(warnings)]

/// Mononoke Cross Repo sync job
///
/// This is a special job used to tail "small" Mononoke repo into "large" Mononoke repo when
/// small repo is a source of truth (i.e. "hg push" go directly to small repo).
/// At the moment the main limitation is that syncing of merges commits is not supported
/// (though it will be changed soon to support some types of merges).
///
/// This job does tailing by following bookmark update log of the small repo and replaying
/// each commit into the large repo. Note that some bookmarks called "common_pushrebase_bookmarks"
/// are treated specially, see comments in the code for more details
use anyhow::{format_err, Error, Result};
use backsyncer::format_counter as format_backsyncer_counter;
use blobrepo::BlobRepo;
use bookmarks::{BookmarkName, BookmarkUpdateLog, Freshness};
use cached_config::ConfigStore;
use clap::{App, ArgMatches};
use cmdlib::{args, monitoring};
use cmdlib_x_repo::{create_commit_syncer_args_from_matches, create_commit_syncer_from_matches};
use context::CoreContext;
use cross_repo_sync::{
    types::{Source, Target},
    CommitSyncer, CommitSyncerArgs,
};
use derived_data_utils::derive_data_for_csids;
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future::TryFutureExt,
    future::{self, try_join, try_join3},
    stream::{self, TryStreamExt},
    StreamExt,
};
use futures_stats::TimedFutureExt;
use live_commit_sync_config::{CfgrLiveCommitSyncConfig, LiveCommitSyncConfig};
use mononoke_types::{ChangesetId, RepositoryId};
use mutable_counters::{MutableCounters, SqlMutableCounters};
use scuba_ext::ScubaSampleBuilder;
use skiplist::SkiplistIndex;
use slog::{debug, error, info, warn};
use std::{collections::HashSet, sync::Arc, time::Duration};
use synced_commit_mapping::SyncedCommitMapping;

mod cli;
mod reporting;
mod setup;
mod sync;

use crate::cli::{
    create_app, ARG_BACKPRESSURE_REPOS_IDS, ARG_CATCH_UP_ONCE, ARG_DERIVED_DATA_TYPES, ARG_ONCE,
    ARG_TAIL,
};
use crate::reporting::{add_common_fields, log_bookmark_update_result, log_noop_iteration};
use crate::setup::{
    get_scuba_sample, get_skiplist_index, get_sleep_secs, get_starting_commit, get_target_bookmark,
};
use crate::sync::{
    is_already_synced, sync_commit_without_pushrebase, sync_commits_via_pushrebase,
    sync_single_bookmark_update_log,
};

fn print_error(ctx: CoreContext, error: &Error) {
    error!(ctx.logger(), "{}", error);
    for cause in error.chain().skip(1) {
        error!(ctx.logger(), "caused by: {}", cause);
    }
}

async fn run_in_single_commit_mode<M: SyncedCommitMapping + Clone + 'static>(
    ctx: &CoreContext,
    bcs: ChangesetId,
    commit_syncer: CommitSyncer<M>,
    scuba_sample: ScubaSampleBuilder,
    source_skiplist_index: Source<Arc<SkiplistIndex>>,
    target_skiplist_index: Target<Arc<SkiplistIndex>>,
    bookmark: BookmarkName,
    common_bookmarks: HashSet<BookmarkName>,
) -> Result<(), Error> {
    let is_synced = is_already_synced(ctx.clone(), bcs.clone(), commit_syncer.clone())
        .compat()
        .await?;

    if is_synced {
        return Ok(());
    }

    let res = if common_bookmarks.contains(&bookmark) {
        info!(ctx.logger(), "syncing via pushrebase");
        sync_commits_via_pushrebase(
            &ctx,
            &commit_syncer,
            &source_skiplist_index,
            &target_skiplist_index,
            &bookmark,
            &common_bookmarks,
            scuba_sample,
            vec![bcs],
        )
        .await
    } else {
        info!(ctx.logger(), "syncing without pushrebase");
        sync_commit_without_pushrebase(
            &ctx,
            &commit_syncer,
            &target_skiplist_index,
            scuba_sample,
            bcs,
            &common_bookmarks,
        )
        .await
    };
    if res.is_ok() {
        info!(ctx.logger(), "successful sync");
    }
    res.map(|_| ())
}

enum TailingArgs<M> {
    CatchUpOnce(CommitSyncer<M>),
    LoopForever(CommitSyncerArgs<M>, ConfigStore),
}

async fn run_in_tailing_mode<
    M: SyncedCommitMapping + Clone + 'static,
    C: MutableCounters + Clone + Sync + 'static,
>(
    ctx: &CoreContext,
    mutable_counters: C,
    source_skiplist_index: Source<Arc<SkiplistIndex>>,
    target_skiplist_index: Target<Arc<SkiplistIndex>>,
    common_pushrebase_bookmarks: HashSet<BookmarkName>,
    base_scuba_sample: ScubaSampleBuilder,
    backpressure_repos: Vec<BlobRepo>,
    derived_data_types: Vec<String>,
    tailing_args: TailingArgs<M>,
    sleep_secs: u64,
) -> Result<(), Error> {
    match tailing_args {
        TailingArgs::CatchUpOnce(commit_syncer) => {
            let scuba_sample = ScubaSampleBuilder::with_discard();
            tail(
                &ctx,
                &commit_syncer,
                &mutable_counters,
                scuba_sample,
                &common_pushrebase_bookmarks,
                &source_skiplist_index,
                &target_skiplist_index,
                &backpressure_repos,
                &derived_data_types,
                sleep_secs,
            )
            .await?;
        }
        TailingArgs::LoopForever(commit_syncer_args, config_store) => {
            let live_commit_sync_config =
                CfgrLiveCommitSyncConfig::new(ctx.logger(), &config_store)?;
            let source_repo_id = commit_syncer_args.get_source_repo().get_repoid();

            loop {
                let scuba_sample = base_scuba_sample.clone();
                // We only care about public pushes because draft pushes are not in the bookmark
                // update log at all.
                let enabled =
                    live_commit_sync_config.push_redirector_enabled_for_public(source_repo_id);

                // Pushredirection is enabled - we need to disable forward sync in that case
                if enabled {
                    log_noop_iteration(scuba_sample);
                    tokio::time::delay_for(Duration::new(sleep_secs, 0)).await;
                    continue;
                }

                let commit_sync_config =
                    live_commit_sync_config.get_current_commit_sync_config(&ctx, source_repo_id)?;

                let commit_syncer = commit_syncer_args
                    .clone()
                    .try_into_commit_syncer(&commit_sync_config)?;

                let synced_something = tail(
                    &ctx,
                    &commit_syncer,
                    &mutable_counters,
                    scuba_sample.clone(),
                    &common_pushrebase_bookmarks,
                    &source_skiplist_index,
                    &target_skiplist_index,
                    &backpressure_repos,
                    &derived_data_types,
                    sleep_secs,
                )
                .await?;

                if !synced_something {
                    log_noop_iteration(scuba_sample);
                    tokio::time::delay_for(Duration::new(sleep_secs, 0)).await;
                }
            }
        }
    }

    Ok(())
}

async fn tail<
    M: SyncedCommitMapping + Clone + 'static,
    C: MutableCounters + Clone + Sync + 'static,
>(
    ctx: &CoreContext,
    commit_syncer: &CommitSyncer<M>,
    mutable_counters: &C,
    mut scuba_sample: ScubaSampleBuilder,
    common_pushrebase_bookmarks: &HashSet<BookmarkName>,
    source_skiplist_index: &Source<Arc<SkiplistIndex>>,
    target_skiplist_index: &Target<Arc<SkiplistIndex>>,
    backpressure_repos: &[BlobRepo],
    derived_data_types: &[String],
    sleep_secs: u64,
) -> Result<bool, Error> {
    let source_repo = commit_syncer.get_source_repo();
    let target_repo_id = commit_syncer.get_target_repo_id();
    let bookmark_update_log = source_repo.attribute_expected::<dyn BookmarkUpdateLog>();
    let counter = format_counter(&commit_syncer);

    let maybe_start_id = mutable_counters
        .get_counter(ctx.clone(), target_repo_id, &counter)
        .compat()
        .await?;
    let start_id = maybe_start_id.ok_or(format_err!("counter not found"))?;
    let limit = 10;
    let log_entries = bookmark_update_log
        .read_next_bookmark_log_entries(ctx.clone(), start_id as u64, limit, Freshness::MaybeStale)
        .try_collect::<Vec<_>>()
        .await?;

    let remaining_entries = commit_syncer
        .get_source_repo()
        .count_further_bookmark_log_entries(ctx.clone(), start_id as u64, None)
        .compat()
        .await?;

    if log_entries.is_empty() {
        log_noop_iteration(scuba_sample.clone());
        Ok(false)
    } else {
        scuba_sample.add("queue_size", remaining_entries);
        info!(ctx.logger(), "queue size is {}", remaining_entries);

        for entry in log_entries {
            let entry_id = entry.id;
            scuba_sample.add("entry_id", entry.id);

            let (stats, res) = sync_single_bookmark_update_log(
                &ctx,
                &commit_syncer,
                entry,
                source_skiplist_index,
                target_skiplist_index,
                &common_pushrebase_bookmarks,
                scuba_sample.clone(),
            )
            .timed()
            .await;

            log_bookmark_update_result(&ctx, entry_id, scuba_sample.clone(), &res, stats);
            let synced_css = res?;

            derive_data_for_csids(
                &ctx,
                &commit_syncer.get_target_repo(),
                synced_css,
                derived_data_types,
            )?
            .await?;

            maybe_apply_backpressure(
                ctx,
                mutable_counters,
                backpressure_repos,
                commit_syncer.get_target_repo().get_repoid(),
                scuba_sample.clone(),
                sleep_secs,
            )
            .await?;

            // Note that updating the counter might fail after successful sync of the commits.
            // This is expected - next run will try to update the counter again without
            // re-syncing the commits.
            mutable_counters
                .set_counter(ctx.clone(), target_repo_id, &counter, entry_id, None)
                .compat()
                .await?;
        }
        Ok(true)
    }
}

async fn maybe_apply_backpressure<C>(
    ctx: &CoreContext,
    mutable_counters: &C,
    backpressure_repos: &[BlobRepo],
    target_repo_id: RepositoryId,
    scuba_sample: ScubaSampleBuilder,
    sleep_secs: u64,
) -> Result<(), Error>
where
    C: MutableCounters + Clone + Sync + 'static,
{
    let limit = 10;
    loop {
        let max_further_entries = stream::iter(backpressure_repos)
            .map(Ok)
            .map_ok(|repo| {
                async move {
                    let repo_id = repo.get_repoid();
                    let backsyncer_counter = format_backsyncer_counter(&target_repo_id);
                    let maybe_counter = mutable_counters
                        .get_counter(ctx.clone(), repo_id, &backsyncer_counter)
                        .compat()
                        .await?;

                    match maybe_counter {
                        Some(counter) => {
                            let bookmark_update_log =
                                repo.attribute_expected::<dyn BookmarkUpdateLog>();
                            debug!(ctx.logger(), "repo {}, counter {}", repo_id, counter);
                            bookmark_update_log
                                .count_further_bookmark_log_entries(
                                    ctx.clone(),
                                    counter as u64,
                                    None, // exclude_reason
                                )
                                .await
                        }
                        None => {
                            warn!(
                                ctx.logger(),
                                "backsyncer counter not found for repo {}!", repo_id,
                            );
                            Ok(0)
                        }
                    }
                }
            })
            .try_buffer_unordered(100)
            .try_fold(0, |acc, x| future::ready(Ok(::std::cmp::max(acc, x))))
            .await?;

        if max_further_entries > limit {
            reporting::log_backpressure(ctx, max_further_entries, scuba_sample.clone());
            tokio::time::delay_for(Duration::from_secs(sleep_secs)).await;
        } else {
            break;
        }
    }
    Ok(())
}

fn format_counter<M: SyncedCommitMapping + Clone + 'static>(
    commit_syncer: &CommitSyncer<M>,
) -> String {
    let source_repo_id = commit_syncer.get_source_repo_id();
    format!("xreposync_from_{}", source_repo_id)
}

async fn run(
    fb: FacebookInit,
    ctx: CoreContext,
    matches: ArgMatches<'static>,
) -> Result<(), Error> {
    let mut scuba_sample = get_scuba_sample(ctx.clone(), &matches);
    let mutable_counters = args::open_source_sql::<SqlMutableCounters>(fb, &matches).compat();

    let source_repo_id = args::get_source_repo_id(fb, &matches)?;
    let target_repo_id = args::get_target_repo_id(fb, &matches)?;
    let (_, source_repo_config) = args::get_config_by_repoid(fb, &matches, source_repo_id)?;
    let (_, target_repo_config) = args::get_config_by_repoid(fb, &matches, target_repo_id)?;

    let common_bookmarks: HashSet<_> = source_repo_config
        .commit_sync_config
        .clone()
        .ok_or(format_err!("commit sync config not found!"))?
        .common_pushrebase_bookmarks
        .into_iter()
        .collect();

    let logger = ctx.logger();
    let source_repo = args::open_repo_with_repo_id(fb, &logger, source_repo_id, &matches).compat();
    let target_repo = args::open_repo_with_repo_id(fb, &logger, target_repo_id, &matches).compat();

    let (source_repo, target_repo, counters) =
        try_join3(source_repo, target_repo, mutable_counters).await?;

    let commit_syncer_args = create_commit_syncer_args_from_matches(fb, &logger, &matches).await?;
    // NOTE: this does not use `CfgrLiveCommitSyncConfig`, as I want to allow
    // for an opportunity to call this binary in "once" mode with
    // local fs-based configs
    let commit_syncer = create_commit_syncer_from_matches(fb, &logger, &matches).await?;

    let source_skiplist =
        get_skiplist_index(&ctx, &source_repo_config, &source_repo).map_ok(Source);
    let target_skiplist =
        get_skiplist_index(&ctx, &target_repo_config, &target_repo).map_ok(Target);
    match matches.subcommand() {
        (ARG_ONCE, Some(sub_m)) => {
            add_common_fields(&mut scuba_sample, &commit_syncer_args);
            let target_bookmark = get_target_bookmark(&sub_m)?;
            let (bcs, source_skiplist_index, target_skiplist_index) = try_join3(
                get_starting_commit(ctx.clone(), &sub_m, source_repo.clone()).compat(),
                source_skiplist,
                target_skiplist,
            )
            .await?;

            run_in_single_commit_mode(
                &ctx,
                bcs,
                commit_syncer,
                scuba_sample,
                source_skiplist_index,
                target_skiplist_index,
                target_bookmark,
                common_bookmarks,
            )
            .await
        }
        (ARG_TAIL, Some(sub_m)) => {
            let (source_skiplist_index, target_skiplist_index) =
                try_join(source_skiplist, target_skiplist).await?;
            add_common_fields(&mut scuba_sample, &commit_syncer_args);

            let sleep_secs = get_sleep_secs(&sub_m)?;
            let tailing_args = if sub_m.is_present(ARG_CATCH_UP_ONCE) {
                TailingArgs::CatchUpOnce(commit_syncer)
            } else {
                let config_store = args::maybe_init_config_store(fb, ctx.logger(), &matches)
                    .ok_or_else(|| format_err!("Failed to init ConfigStore."))?;

                TailingArgs::LoopForever(commit_syncer_args, config_store)
            };

            let backpressure_repos_ids = sub_m.values_of(ARG_BACKPRESSURE_REPOS_IDS);
            let backpressure_repos = match backpressure_repos_ids {
                Some(backpressure_repos_ids) => {
                    let backpressure_repos =
                        stream::iter(backpressure_repos_ids.into_iter().map(|repo_id| {
                            let repo_id = repo_id.parse::<i32>()?;
                            Ok(repo_id)
                        }))
                        .map_ok(|repo_id| {
                            args::open_repo_with_repo_id(
                                fb,
                                ctx.logger(),
                                RepositoryId::new(repo_id),
                                &matches,
                            )
                            .compat()
                        })
                        .try_buffer_unordered(100)
                        .try_collect::<Vec<_>>();
                    backpressure_repos.await?
                }
                None => vec![],
            };

            let derived_data_types: Vec<String> = match sub_m.values_of(ARG_DERIVED_DATA_TYPES) {
                Some(derived_data_types) => derived_data_types
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>(),
                None => vec![],
            };

            run_in_tailing_mode(
                &ctx,
                counters,
                source_skiplist_index,
                target_skiplist_index,
                common_bookmarks,
                scuba_sample,
                backpressure_repos,
                derived_data_types,
                tailing_args,
                sleep_secs,
            )
            .await
        }
        (incorrect, _) => Err(format_err!(
            "Incorrect mode of operation specified: {}",
            incorrect
        )),
    }
}

fn context_and_matches<'a>(fb: FacebookInit, app: App<'a, '_>) -> (CoreContext, ArgMatches<'a>) {
    let matches = app.get_matches();
    let logger = args::init_logging(fb, &matches);
    args::init_cachelib(fb, &matches, None);
    let ctx = CoreContext::new_with_logger(fb, logger);
    (ctx, matches)
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let (ctx, matches) = context_and_matches(fb, create_app());

    let mut runtime = tokio_compat::runtime::Runtime::new()?;
    monitoring::start_fb303_and_stats_agg(
        fb,
        &mut runtime,
        "x_repo_sync_job",
        ctx.logger(),
        &matches,
        monitoring::AliveService,
    )?;
    let res = runtime.block_on_std(run(fb, ctx.clone(), matches));
    if let Err(ref err) = res {
        print_error(ctx, err);
    }
    res
}
