/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::format_err;
use anyhow::Error;
use backsyncer::backsync_latest;
use backsyncer::format_counter;
use backsyncer::open_backsyncer_dbs;
use backsyncer::BacksyncLimit;
use backsyncer::TargetRepoDbs;
use blobrepo_hg::BlobRepoHg;
use bookmarks::Freshness;
use clap::Arg;
use clap::SubCommand;
use cloned::cloned;
use cmdlib::args;
use cmdlib::helpers;
use cmdlib::monitoring;
use cmdlib_x_repo::create_commit_syncer_from_matches;
use context::CoreContext;
use context::SessionContainer;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncOutcome;
use cross_repo_sync::CommitSyncer;
use fbinit::FacebookInit;
use futures::future::FutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::try_join;
use live_commit_sync_config::CfgrLiveCommitSyncConfig;
use live_commit_sync_config::LiveCommitSyncConfig;
use mercurial_derived_data::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;
use slog::info;
use stats::prelude::*;
use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use synced_commit_mapping::SqlSyncedCommitMapping;
use synced_commit_mapping::SyncedCommitMapping;

const ARG_MODE_BACKSYNC_FOREVER: &str = "backsync-forever";
const ARG_MODE_BACKSYNC_ALL: &str = "backsync-all";
const ARG_MODE_BACKSYNC_COMMITS: &str = "backsync-commits";
const ARG_BATCH_SIZE: &str = "batch-size";
const ARG_INPUT_FILE: &str = "INPUT_FILE";
const SCUBA_TABLE: &str = "mononoke_xrepo_backsync";

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
        Some(RewrittenAs(cs_id, _)) => Ok(Some(cs_id)),
        Some(NotSyncCandidate(_)) => Ok(None),
        Some(EquivalentWorkingCopyAncestor(cs_id, _)) => Ok(Some(cs_id)),
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
            let hg_cs_id = commit_syncer
                .get_target_repo()
                .derive_hg_changeset(ctx, target_cs_id)
                .await?;
            info!(
                ctx.logger(),
                "Hg cs id {} derived for {}", hg_cs_id, target_cs_id
            );
            Ok(())
        }
        None => Ok(()),
    }
}

pub async fn backsync_forever<M>(
    ctx: CoreContext,
    commit_syncer: CommitSyncer<M>,
    target_repo_dbs: TargetRepoDbs,
    source_repo_name: String,
    target_repo_name: String,
    live_commit_sync_config: CfgrLiveCommitSyncConfig,
) -> Result<(), Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let target_repo_id = commit_syncer.get_target_repo_id();
    let live_commit_sync_config = Arc::new(live_commit_sync_config);

    loop {
        // We only care about public pushes because draft pushes are not in the bookmark
        // update log at all.
        let enabled = live_commit_sync_config.push_redirector_enabled_for_public(target_repo_id);

        if enabled {
            let delay = calculate_delay(&ctx, &commit_syncer, &target_repo_dbs).await?;
            log_delay(&ctx, &delay, &source_repo_name, &target_repo_name);
            if delay.remaining_entries == 0 {
                debug!(ctx.logger(), "no entries remained");
                tokio::time::sleep(Duration::new(1, 0)).await;
            } else {
                debug!(ctx.logger(), "backsyncing...");

                backsync_latest(
                    ctx.clone(),
                    commit_syncer.clone(),
                    target_repo_dbs.clone(),
                    BacksyncLimit::NoLimit,
                )
                .await?
            }
        } else {
            debug!(ctx.logger(), "push redirector is disabled");
            let delay = Delay::no_delay();
            log_delay(&ctx, &delay, &source_repo_name, &target_repo_name);
            tokio::time::sleep(Duration::new(1, 0)).await;
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
    commit_syncer: &CommitSyncer<M>,
    target_repo_dbs: &TargetRepoDbs,
) -> Result<Delay, Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let TargetRepoDbs { ref counters, .. } = target_repo_dbs;
    let source_repo_id = commit_syncer.get_source_repo().get_repoid();

    let counter_name = format_counter(&source_repo_id);
    let maybe_counter = counters.get_counter(ctx, &counter_name).await?;
    let counter = maybe_counter.ok_or(format_err!("{} counter not found", counter_name))?;
    let source_repo = commit_syncer.get_source_repo();
    let next_entry = source_repo
        .read_next_bookmark_log_entries(ctx.clone(), counter as u64, 1, Freshness::MostRecent)
        .try_collect::<Vec<_>>();
    let remaining_entries =
        source_repo.count_further_bookmark_log_entries(ctx.clone(), counter as u64, None);

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
    let app = args::MononokeAppBuilder::new(app_name)
        .with_fb303_args()
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
    let matches = app.get_matches(fb)?;

    let logger = matches.logger();
    let runtime = matches.runtime();
    let config_store = matches.config_store();

    let source_repo_id = args::get_source_repo_id(config_store, &matches)?;
    let target_repo_id = args::get_target_repo_id(config_store, &matches)?;

    let (source_repo_name, _) = args::get_config_by_repoid(config_store, &matches, source_repo_id)?;
    let (target_repo_name, target_repo_config) =
        args::get_config_by_repoid(config_store, &matches, target_repo_id)?;

    let session_container = SessionContainer::new_with_defaults(fb);
    let commit_syncer = {
        let scuba_sample = MononokeScubaSampleBuilder::with_discard();
        let ctx = session_container.new_context(logger.clone(), scuba_sample);
        runtime.block_on(create_commit_syncer_from_matches(&ctx, &matches))?
    };

    let mysql_options = matches.mysql_options();
    let readonly_storage = matches.readonly_storage();

    info!(
        logger,
        "syncing from repoid {:?} into repoid {:?}", source_repo_id, target_repo_id,
    );

    let config_store = matches.config_store();
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(&logger, config_store)?;

    match matches.subcommand() {
        (ARG_MODE_BACKSYNC_ALL, _) => {
            let scuba_sample = MononokeScubaSampleBuilder::with_discard();
            let ctx = session_container.new_context(logger.clone(), scuba_sample);
            let db_config = target_repo_config.storage_config.metadata;
            let target_repo_dbs = runtime.block_on(
                open_backsyncer_dbs(
                    ctx.clone(),
                    commit_syncer.get_target_repo().clone(),
                    db_config,
                    mysql_options.clone(),
                    *readonly_storage,
                )
                .boxed(),
            )?;

            // TODO(ikostia): why do we use discarding ScubaSample for BACKSYNC_ALL?
            runtime.block_on(
                backsync_latest(ctx, commit_syncer, target_repo_dbs, BacksyncLimit::NoLimit)
                    .boxed(),
            )?;
        }
        (ARG_MODE_BACKSYNC_FOREVER, _) => {
            let db_config = target_repo_config.storage_config.metadata;
            let ctx = session_container
                .new_context(logger.clone(), MononokeScubaSampleBuilder::with_discard());
            let target_repo_dbs = runtime.block_on(
                open_backsyncer_dbs(
                    ctx,
                    commit_syncer.get_target_repo().clone(),
                    db_config,
                    mysql_options.clone(),
                    *readonly_storage,
                )
                .boxed(),
            )?;

            let mut scuba_sample = MononokeScubaSampleBuilder::new(fb, SCUBA_TABLE);
            scuba_sample.add("source_repo", source_repo_id.id());
            scuba_sample.add("source_repo_name", source_repo_name.clone());
            scuba_sample.add("target_repo", target_repo_id.id());
            scuba_sample.add("target_repo_name", target_repo_name.clone());
            scuba_sample.add_common_server_data();

            let ctx = session_container.new_context(logger.clone(), scuba_sample);
            let f = backsync_forever(
                ctx,
                commit_syncer,
                target_repo_dbs,
                source_repo_name,
                target_repo_name,
                live_commit_sync_config,
            )
            .boxed();

            helpers::block_execute(f, fb, app_name, &logger, &matches, monitoring::AliveService)?;
        }
        (ARG_MODE_BACKSYNC_COMMITS, Some(sub_m)) => {
            let ctx = session_container
                .new_context(logger.clone(), MononokeScubaSampleBuilder::with_discard());
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
                        cloned!(ctx, source_repo);
                        async move { source_repo.get_hg_bonsai_mapping(ctx.clone(), chunk).await }
                    }
                })
                .try_fold(0, move |backsynced_so_far, hg_bonsai_mapping| {
                    hg_bonsai_mapping
                        .into_iter()
                        .map({
                            move |(_, bonsai)| async move {
                                // Backsyncer is always used in the large-to-small direction,
                                // therefore there can be at most one remapped candidate,
                                // so `CandidateSelectionHint::Only` is a safe choice
                                commit_syncer
                                    .sync_commit(
                                        &ctx,
                                        bonsai.clone(),
                                        CandidateSelectionHint::Only,
                                        CommitSyncContext::Backsyncer,
                                    )
                                    .await?;

                                let maybe_sync_outcome =
                                    commit_syncer.get_commit_sync_outcome(&ctx, bonsai).await?;

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

            runtime.block_on(f)?;
        }
        _ => {
            bail!("unknown subcommand");
        }
    }

    Ok(())
}
