/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::{bail, format_err, Error};
use backsyncer::{
    backsync_latest, format_counter, open_backsyncer_dbs, BacksyncLimit, TargetRepoDbs,
};
use blobrepo_hg::BlobRepoHg;
use bookmarks::Freshness;
use cached_config::ConfigStore;
use clap::{Arg, SubCommand};
use cloned::cloned;
use cmdlib::{args, monitoring};
use cmdlib_x_repo::{create_commit_syncer_args_from_matches, create_commit_syncer_from_matches};
use context::{CoreContext, SessionContainer};
use cross_repo_sync::{CommitSyncOutcome, CommitSyncer, CommitSyncerArgs};
use fbinit::FacebookInit;
use futures::{
    compat::Future01CompatExt,
    future::FutureExt,
    stream::{self, StreamExt, TryStreamExt},
    try_join,
};
use futures_old::{stream::Stream as Stream_old, Future};
use live_commit_sync_config::{CfgrLiveCommitSyncConfig, LiveCommitSyncConfig};
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;
use mutable_counters::MutableCounters;
use scuba_ext::ScubaSampleBuilder;
use slog::{debug, info};
use stats::prelude::*;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::str::FromStr;
use std::time::Duration;
use synced_commit_mapping::{SqlSyncedCommitMapping, SyncedCommitMapping};

const ARG_MODE_BACKSYNC_FOREVER: &str = "backsync-forever";
const ARG_MODE_BACKSYNC_ALL: &str = "backsync-all";
const ARG_MODE_BACKSYNC_COMMITS: &str = "backsync-commits";
const ARG_BATCH_SIZE: &str = "batch-size";
const ARG_INPUT_FILE: &str = "INPUT_FILE";
const SCUBA_TABLE: &'static str = "mononoke_xrepo_backsync";

define_stats! {
    prefix = "mononoke.backsyncer";
    remaining_entries: dynamic_singleton_counter(
        "{}.{}.remaining_entries",
        (source_repo_name: String, target_repo_name: String)
    ),
    delay_secs: dynamic_singleton_counter(
        "{}.{}.delay_secs",
        (source_repo_name: String, target_repo_name: String)
    ),
}

fn extract_cs_id_from_sync_outcome(
    source_cs_id: ChangesetId,
    maybe_sync_outcome: Option<CommitSyncOutcome>,
) -> Result<Option<ChangesetId>, Error> {
    use CommitSyncOutcome::*;

    match maybe_sync_outcome {
        Some(Preserved) => Ok(Some(source_cs_id)),
        Some(RewrittenAs(cs_id, _)) => Ok(Some(cs_id)),
        Some(NotSyncCandidate) => Ok(None),
        Some(EquivalentWorkingCopyAncestor(cs_id)) => Ok(Some(cs_id)),
        None => Err(format_err!(
            "sync outcome is not available for {}",
            source_cs_id
        )),
    }
}

async fn derive_target_hg_changesets(
    ctx: &CoreContext,
    maybe_target_cs_id: Option<ChangesetId>,
    commit_syncer: &CommitSyncer<SqlSyncedCommitMapping>,
) -> Result<(), Error> {
    match maybe_target_cs_id {
        Some(target_cs_id) => {
            commit_syncer
                .get_target_repo()
                .get_hg_from_bonsai_changeset(ctx.clone(), target_cs_id)
                .map(move |hg_cs_id| {
                    info!(
                        ctx.logger(),
                        "Hg cs id {} derived for {}", hg_cs_id, target_cs_id
                    );
                })
                .compat()
                .await
        }
        None => Ok(()),
    }
}

pub async fn backsync_forever<M>(
    ctx: CoreContext,
    config_store: ConfigStore,
    commit_syncer_args: CommitSyncerArgs<M>,
    target_repo_dbs: TargetRepoDbs,
    source_repo_name: String,
    target_repo_name: String,
) -> Result<(), Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let target_repo_id = commit_syncer_args.get_target_repo_id();
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(ctx.logger(), &config_store)?;

    loop {
        // We only care about public pushes because draft pushes are not in the bookmark
        // update log at all.
        let enabled = live_commit_sync_config.push_redirector_enabled_for_public(target_repo_id);

        if enabled {
            let delay = calculate_delay(&ctx, &commit_syncer_args, &target_repo_dbs).await?;
            log_delay(&ctx, &delay, &source_repo_name, &target_repo_name);
            if delay.remaining_entries == 0 {
                debug!(ctx.logger(), "no entries remained");
                tokio::time::delay_for(Duration::new(1, 0)).await;
            } else {
                debug!(ctx.logger(), "backsyncing...");
                let commit_sync_config =
                    live_commit_sync_config.get_current_commit_sync_config(&ctx, target_repo_id)?;

                let commit_syncer = commit_syncer_args
                    .clone()
                    .try_into_commit_syncer(&commit_sync_config)?;

                backsync_latest(
                    ctx.clone(),
                    commit_syncer,
                    target_repo_dbs.clone(),
                    BacksyncLimit::NoLimit,
                )
                .await?
            }
        } else {
            debug!(ctx.logger(), "push redirector is disabled");
            let delay = Delay::no_delay();
            log_delay(&ctx, &delay, &source_repo_name, &target_repo_name);
            tokio::time::delay_for(Duration::new(1, 0)).await;
        }
    }
}

struct Delay {
    delay_secs: i64,
    remaining_entries: u64,
}

impl Delay {
    fn no_delay() -> Self {
        Self {
            delay_secs: 0,
            remaining_entries: 0,
        }
    }
}

// Returns logs delay and returns the number of remaining bookmark update log entries
async fn calculate_delay<M>(
    ctx: &CoreContext,
    commit_syncer_args: &CommitSyncerArgs<M>,
    target_repo_dbs: &TargetRepoDbs,
) -> Result<Delay, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let TargetRepoDbs { ref counters, .. } = target_repo_dbs;
    let target_repo_id = commit_syncer_args.get_target_repo().get_repoid();
    let source_repo_id = commit_syncer_args.get_source_repo().get_repoid();

    let counter_name = format_counter(&source_repo_id);
    let maybe_counter = counters
        .get_counter(ctx.clone(), target_repo_id, &counter_name)
        .compat()
        .await?;
    let counter = maybe_counter.ok_or(format_err!("{} counter not found", counter_name))?;
    let source_repo = commit_syncer_args.get_source_repo();
    let next_entry = source_repo
        .read_next_bookmark_log_entries(ctx.clone(), counter as u64, 1, Freshness::MostRecent)
        .collect()
        .compat();
    let remaining_entries = source_repo
        .count_further_bookmark_log_entries(ctx.clone(), counter as u64, None)
        .compat();

    let (next_entry, remaining_entries) = try_join!(next_entry, remaining_entries)?;
    let delay_secs = next_entry
        .get(0)
        .map(|entry| entry.timestamp.since_seconds())
        .unwrap_or(0);

    Ok(Delay {
        delay_secs,
        remaining_entries,
    })
}

fn log_delay(
    ctx: &CoreContext,
    delay: &Delay,
    source_repo_name: &String,
    target_repo_name: &String,
) {
    STATS::remaining_entries.set_value(
        ctx.fb,
        delay.remaining_entries as i64,
        (source_repo_name.clone(), target_repo_name.clone()),
    );
    STATS::delay_secs.set_value(
        ctx.fb,
        delay.delay_secs,
        (source_repo_name.clone(), target_repo_name.clone()),
    );
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app_name = "backsyncer cmd-line tool";
    let app = args::MononokeApp::new(app_name)
        .with_fb303_args()
        .with_test_args()
        .with_source_and_target_repos()
        .build();
    let backsync_forever_subcommand =
        SubCommand::with_name(ARG_MODE_BACKSYNC_FOREVER).about("Backsyncs all new bookmark moves");

    let sync_loop = SubCommand::with_name(ARG_MODE_BACKSYNC_COMMITS)
        .about("Syncs all commits from the file")
        .arg(
            Arg::with_name(ARG_INPUT_FILE)
                .takes_value(true)
                .required(true)
                .help("list of hg commits to backsync"),
        )
        .arg(
            Arg::with_name(ARG_BATCH_SIZE)
                .long(ARG_BATCH_SIZE)
                .takes_value(true)
                .required(false)
                .help("how many commits to backsync at once"),
        );

    let backsync_all_subcommand =
        SubCommand::with_name(ARG_MODE_BACKSYNC_ALL).about("Backsyncs all new bookmark moves once");
    let app = app
        .subcommand(backsync_all_subcommand)
        .subcommand(backsync_forever_subcommand)
        .subcommand(sync_loop);
    let matches = app.get_matches();

    let (_, logger, mut runtime) = args::init_mononoke(fb, &matches, None)?;

    let source_repo_id = args::get_source_repo_id(fb, &matches)?;
    let target_repo_id = args::get_target_repo_id(fb, &matches)?;

    let (source_repo_name, _) = args::get_config_by_repoid(fb, &matches, source_repo_id)?;
    let (target_repo_name, target_repo_config) =
        args::get_config_by_repoid(fb, &matches, target_repo_id)?;

    let commit_syncer_args = runtime.block_on_std(create_commit_syncer_args_from_matches(
        fb, &logger, &matches,
    ))?;
    let mysql_options = args::parse_mysql_options(&matches);
    let readonly_storage = args::parse_readonly_storage(&matches);

    let session_container = SessionContainer::new_with_defaults(fb);

    info!(
        logger,
        "syncing from repoid {:?} into repoid {:?}", source_repo_id, target_repo_id,
    );

    match matches.subcommand() {
        (ARG_MODE_BACKSYNC_ALL, _) => {
            // NOTE: this does not use `CfgrLiveCommitSyncConfig`, as I want to allow
            // for an opportunity to call this binary in non-forever mode with
            // local fs-based configs
            let commit_syncer =
                runtime.block_on_std(create_commit_syncer_from_matches(fb, &logger, &matches))?;

            let scuba_sample = ScubaSampleBuilder::with_discard();
            let ctx = session_container.new_context(logger.clone(), scuba_sample);
            let db_config = target_repo_config.storage_config.metadata;
            let target_repo_dbs = runtime.block_on_std(
                open_backsyncer_dbs(
                    ctx.clone(),
                    commit_syncer.get_target_repo().clone(),
                    db_config,
                    mysql_options,
                    readonly_storage,
                )
                .boxed(),
            )?;

            runtime.block_on_std(
                backsync_latest(ctx, commit_syncer, target_repo_dbs, BacksyncLimit::NoLimit)
                    .boxed(),
            )?;
        }
        (ARG_MODE_BACKSYNC_FOREVER, _) => {
            let db_config = target_repo_config.storage_config.metadata;
            let ctx =
                session_container.new_context(logger.clone(), ScubaSampleBuilder::with_discard());
            let target_repo_dbs = runtime.block_on_std(
                open_backsyncer_dbs(
                    ctx,
                    commit_syncer_args.get_target_repo().clone(),
                    db_config,
                    mysql_options,
                    readonly_storage,
                )
                .boxed(),
            )?;

            let config_store = args::maybe_init_config_store(fb, &logger, &matches)
                .ok_or_else(|| format_err!("Failed initializing ConfigStore"))?;

            let mut scuba_sample = ScubaSampleBuilder::new(fb, SCUBA_TABLE);
            scuba_sample.add("source_repo", source_repo_id.id());
            scuba_sample.add("source_repo_name", source_repo_name.clone());
            scuba_sample.add("target_repo", target_repo_id.id());
            scuba_sample.add("target_repo_name", target_repo_name.clone());
            scuba_sample.add_common_server_data();

            let ctx = session_container.new_context(logger.clone(), scuba_sample);
            let f = backsync_forever(
                ctx,
                config_store,
                commit_syncer_args,
                target_repo_dbs,
                source_repo_name,
                target_repo_name,
            )
            .boxed();

            monitoring::start_fb303_and_stats_agg(
                fb,
                &mut runtime,
                app_name,
                &logger,
                &matches,
                monitoring::AliveService,
            )?;
            runtime.block_on_std(f)?;
        }
        (ARG_MODE_BACKSYNC_COMMITS, Some(sub_m)) => {
            // NOTE: this does not use `CfgrLiveCommitSyncConfig`, as I want to allow
            // for an opportunity to call this binary in non-forever mode with
            // local fs-based configs
            let commit_syncer =
                runtime.block_on_std(create_commit_syncer_from_matches(fb, &logger, &matches))?;

            let ctx = session_container.new_context(logger, ScubaSampleBuilder::with_discard());
            let inputfile = sub_m
                .value_of(ARG_INPUT_FILE)
                .expect("input file is not set");
            let inputfile = File::open(inputfile)?;
            let file = BufReader::new(&inputfile);
            let batch_size = args::get_usize(&matches, ARG_BATCH_SIZE, 100);

            let source_repo = commit_syncer.get_source_repo().clone();

            let mut hg_cs_ids = vec![];
            for line in file.lines() {
                hg_cs_ids.push(HgChangesetId::from_str(&line?)?);
            }
            let total_to_backsync = hg_cs_ids.len();
            info!(ctx.logger(), "backsyncing {} commits", total_to_backsync);

            let ctx = &ctx;
            let commit_syncer = &commit_syncer;

            let f = stream::iter(hg_cs_ids.clone())
                .chunks(batch_size)
                .map(Result::<_, Error>::Ok)
                .and_then({
                    cloned!(ctx);
                    move |chunk| {
                        source_repo
                            .get_hg_bonsai_mapping(ctx.clone(), chunk)
                            .compat()
                    }
                })
                .try_fold(0, move |backsynced_so_far, hg_bonsai_mapping| {
                    hg_bonsai_mapping
                        .into_iter()
                        .map({
                            move |(_, bonsai)| async move {
                                commit_syncer.sync_commit(&ctx, bonsai.clone()).await?;

                                let maybe_sync_outcome = commit_syncer
                                    .get_commit_sync_outcome(ctx.clone(), bonsai)
                                    .await?;

                                info!(
                                    ctx.logger(),
                                    "{} backsynced as {:?}", bonsai, maybe_sync_outcome
                                );

                                let maybe_target_cs_id =
                                    extract_cs_id_from_sync_outcome(bonsai, maybe_sync_outcome)?;

                                derive_target_hg_changesets(ctx, maybe_target_cs_id, commit_syncer)
                                    .await
                            }
                        })
                        .collect::<stream::futures_unordered::FuturesUnordered<_>>()
                        .try_fold(backsynced_so_far, {
                            move |backsynced_so_far, _| async move {
                                info!(
                                    ctx.logger(),
                                    "backsynced so far {} out of {}",
                                    backsynced_so_far + 1,
                                    total_to_backsync
                                );
                                Ok::<_, Error>(backsynced_so_far + 1)
                            }
                        })
                });

            runtime.block_on_std(f)?;
        }
        _ => {
            bail!("unknown subcommand");
        }
    }

    Ok(())
}
