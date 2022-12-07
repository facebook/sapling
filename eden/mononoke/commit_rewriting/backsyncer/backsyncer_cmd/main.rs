/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::File;
use std::io::BufRead;
use std::io::BufReader;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use backsyncer::backsync_latest;
use backsyncer::format_counter;
use backsyncer::open_backsyncer_dbs;
use backsyncer::BacksyncLimit;
use blobrepo_hg::BlobRepoHg;
use bookmarks::Freshness;
use clap::Arg;
use clap::SubCommand;
use cloned::cloned;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers;
use cmdlib_x_repo::create_commit_syncer_from_matches;
use context::CoreContext;
use context::SessionContainer;
use cross_repo_sync::CandidateSelectionHint;
use cross_repo_sync::CommitSyncContext;
use cross_repo_sync::CommitSyncOutcome;
use cross_repo_sync::CommitSyncer;
use executor_lib::split_repo_names;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use executor_lib::ShardedProcessExecutor;
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
use once_cell::sync::OnceCell;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::debug;
use slog::error;
use slog::info;
use stats::prelude::*;
use synced_commit_mapping::SqlSyncedCommitMapping;
use synced_commit_mapping::SyncedCommitMapping;
use wireproto_handler::TargetRepoDbs;

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

const SM_SERVICE_SCOPE: &str = "global";
const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;
const APP_NAME: &str = "backsyncer cmd-line tool";

/// Struct representing the Back Syncer BP.
pub struct BacksyncProcess {
    matches: Arc<MononokeMatches<'static>>,
    fb: FacebookInit,
}

impl BacksyncProcess {
    fn new(fb: FacebookInit) -> anyhow::Result<Self> {
        let app = args::MononokeAppBuilder::new(APP_NAME)
            .with_fb303_args()
            .with_source_and_target_repos()
            .with_dynamic_repos()
            .build();
        let backsync_forever_subcommand = SubCommand::with_name(ARG_MODE_BACKSYNC_FOREVER)
            .about("Backsyncs all new bookmark moves");

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

        let backsync_all_subcommand = SubCommand::with_name(ARG_MODE_BACKSYNC_ALL)
            .about("Backsyncs all new bookmark moves once");
        let app = app
            .subcommand(backsync_all_subcommand)
            .subcommand(backsync_forever_subcommand)
            .subcommand(sync_loop);
        let matches = Arc::new(app.get_matches(fb)?);
        Ok(Self { matches, fb })
    }
}

#[async_trait]
impl RepoShardedProcess for BacksyncProcess {
    async fn setup(
        &self,
        repo_names_pair: &str,
    ) -> anyhow::Result<Arc<dyn RepoShardedProcessExecutor>> {
        // Since for backsyncer, two repos (i.e. source and target)
        // are required, the input would be of the form:
        // source_repo_TO_target_repo (e.g. fbsource_TO_aros)
        let repo_pair: Vec<_> = split_repo_names(repo_names_pair);
        if repo_pair.len() != 2 {
            error!(
                self.matches.logger(),
                "Repo names provided in incorrect format: {}", repo_names_pair
            );
            bail!(
                "Repo names provided in incorrect format: {}",
                repo_names_pair
            )
        }
        let (source_repo_name, target_repo_name) = (repo_pair[0], repo_pair[1]);
        info!(
            self.matches.logger(),
            "Setting up back syncer command from repo {} to repo {}",
            source_repo_name,
            target_repo_name,
        );
        let executor = BacksyncProcessExecutor::new(
            self.fb,
            Arc::clone(&self.matches),
            source_repo_name.to_string(),
            target_repo_name.to_string(),
        );
        info!(
            self.matches.logger(),
            "Completed back syncer command setup from repo {} to repo {}",
            source_repo_name,
            target_repo_name,
        );
        Ok(Arc::new(executor))
    }
}

/// Struct representing the execution of the Back Syncer
/// BP over the context of a provided repos.
pub struct BacksyncProcessExecutor {
    fb: FacebookInit,
    matches: Arc<MononokeMatches<'static>>,
    source_repo_name: String,
    target_repo_name: String,
    cancellation_requested: Arc<AtomicBool>,
}

impl BacksyncProcessExecutor {
    fn new(
        fb: FacebookInit,
        matches: Arc<MononokeMatches<'static>>,
        source_repo_name: String,
        target_repo_name: String,
    ) -> Self {
        Self {
            fb,
            matches,
            source_repo_name,
            target_repo_name,
            cancellation_requested: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[async_trait]
impl RepoShardedProcessExecutor for BacksyncProcessExecutor {
    async fn execute(&self) -> anyhow::Result<()> {
        info!(
            self.matches.logger(),
            "Initiating back syncer command execution for repo pair {}-{}",
            &self.source_repo_name,
            &self.target_repo_name,
        );
        run(
            self.fb,
            Arc::clone(&self.matches),
            self.source_repo_name.clone(),
            self.target_repo_name.clone(),
            Arc::clone(&self.cancellation_requested),
        )
        .await
        .with_context(|| {
            format!(
                "Error during back syncer command execution for repo pair {}-{}",
                &self.source_repo_name, &self.target_repo_name,
            )
        })?;
        info!(
            self.matches.logger(),
            "Finished back syncer command execution for repo pair {}-{}",
            &self.source_repo_name,
            self.target_repo_name
        );
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        info!(
            self.matches.logger(),
            "Terminating back syncer command execution for repo pair {}-{}",
            &self.source_repo_name,
            self.target_repo_name,
        );
        self.cancellation_requested.store(true, Ordering::Relaxed);
        Ok(())
    }
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
    target_repo_dbs: Arc<TargetRepoDbs>,
    source_repo_name: String,
    target_repo_name: String,
    live_commit_sync_config: CfgrLiveCommitSyncConfig,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<(), Error>
where
    M: SyncedCommitMapping + Clone + 'static,
{
    let target_repo_id = commit_syncer.get_target_repo_id();
    let live_commit_sync_config = Arc::new(live_commit_sync_config);

    loop {
        // Before initiating loop, check if cancellation has been
        // requested. If yes, then exit early.
        if cancellation_requested.load(Ordering::Relaxed) {
            info!(ctx.logger(), "sync stopping due to cancellation request");
            return Ok(());
        }
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
                    Arc::clone(&cancellation_requested),
                    CommitSyncContext::Backsyncer,
                    false,
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
    let counter = maybe_counter.ok_or_else(|| format_err!("{} counter not found", counter_name))?;
    let source_repo = commit_syncer.get_source_repo();
    let next_entry = source_repo
        .bookmark_update_log()
        .read_next_bookmark_log_entries(ctx.clone(), counter as u64, 1, Freshness::MostRecent)
        .try_collect::<Vec<_>>();
    let remaining_entries = source_repo
        .bookmark_update_log()
        .count_further_bookmark_log_entries(ctx.clone(), counter as u64, None);

    let (next_entry, remaining_entries) = try_join!(next_entry, remaining_entries)?;
    let delay_secs = next_entry
        .get(0)
        .map_or(0, |entry| entry.timestamp.since_seconds());

    Ok(Delay {
        delay_secs,
        remaining_entries,
    })
}

fn log_delay(ctx: &CoreContext, delay: &Delay, source_repo_name: &str, target_repo_name: &str) {
    STATS::remaining_entries.set_value(
        ctx.fb,
        delay.remaining_entries as i64,
        (source_repo_name.to_owned(), target_repo_name.to_owned()),
    );
    STATS::delay_secs.set_value(
        ctx.fb,
        delay.delay_secs,
        (source_repo_name.to_owned(), target_repo_name.to_owned()),
    );
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let process = BacksyncProcess::new(fb)?;
    match process.matches.value_of("sharded-service-name") {
        Some(service_name) => {
            // The service name needs to be 'static to satisfy SM contract
            static SM_SERVICE_NAME: OnceCell<String> = OnceCell::new();
            let logger = process.matches.logger().clone();
            let matches = Arc::clone(&process.matches);
            let mut executor = ShardedProcessExecutor::new(
                process.fb,
                process.matches.runtime().clone(),
                &logger,
                SM_SERVICE_NAME.get_or_init(|| service_name.to_string()),
                SM_SERVICE_SCOPE,
                SM_CLEANUP_TIMEOUT_SECS,
                Arc::new(process),
                true, // enable shard (repo) level healing
            )?;
            helpers::block_execute(
                executor.block_and_execute(&logger, Arc::new(AtomicBool::new(false))),
                fb,
                &std::env::var("TW_JOB_NAME").unwrap_or_else(|_| APP_NAME.to_string()),
                matches.logger(),
                &matches,
                cmdlib::monitoring::AliveService,
            )
        }
        None => {
            let logger = process.matches.logger().clone();
            let matches = process.matches.clone();
            let config_store = matches.config_store();
            let source_repo_id =
                args::not_shardmanager_compatible::get_source_repo_id(config_store, &matches)?;
            let target_repo_id =
                args::not_shardmanager_compatible::get_target_repo_id(config_store, &matches)?;
            let (source_repo_name, _) =
                args::get_config_by_repoid(config_store, &matches, source_repo_id)?;
            let (target_repo_name, _) =
                args::get_config_by_repoid(config_store, &matches, target_repo_id)?;
            let fut = run(
                fb,
                matches.clone(),
                source_repo_name,
                target_repo_name,
                Arc::new(AtomicBool::new(false)),
            );
            helpers::block_execute(
                fut,
                fb,
                APP_NAME,
                &logger,
                &matches,
                cmdlib::monitoring::AliveService,
            )
        }
    }
}

async fn run(
    fb: FacebookInit,
    matches: Arc<MononokeMatches<'static>>,
    source_repo_name: String,
    target_repo_name: String,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<(), Error> {
    let config_store = matches.config_store();

    let source_repo = args::resolve_repo_by_name(config_store, &matches, &source_repo_name)?;
    let target_repo = args::resolve_repo_by_name(config_store, &matches, &target_repo_name)?;
    let repo_tag = format!("{}=>{}", &source_repo_name, &target_repo_name);
    let session_container = SessionContainer::new_with_defaults(fb);
    let ctx = session_container
        .new_context(
            matches.logger().clone(),
            MononokeScubaSampleBuilder::with_discard(),
        )
        .clone_with_repo_name(&repo_tag);
    let commit_syncer =
        create_commit_syncer_from_matches(&ctx, &matches, Some((source_repo.id, target_repo.id)))
            .await?;
    let logger = ctx.logger();
    let mysql_options = matches.mysql_options();
    let readonly_storage = matches.readonly_storage();

    info!(
        logger,
        "syncing from repoid {:?} into repoid {:?}", source_repo.id, target_repo.id,
    );

    let config_store = matches.config_store();
    let live_commit_sync_config = CfgrLiveCommitSyncConfig::new(logger, config_store)?;

    match matches.subcommand() {
        (ARG_MODE_BACKSYNC_ALL, _) => {
            let scuba_sample = MononokeScubaSampleBuilder::with_discard();
            let ctx = session_container.new_context(logger.clone(), scuba_sample);
            let db_config = target_repo.config.storage_config.metadata;
            let target_repo_dbs = Arc::new(
                open_backsyncer_dbs(
                    ctx.clone(),
                    commit_syncer.get_target_repo().clone(),
                    db_config,
                    mysql_options.clone(),
                    *readonly_storage,
                )
                .boxed()
                .await?,
            );

            // TODO(ikostia): why do we use discarding ScubaSample for BACKSYNC_ALL?
            backsync_latest(
                ctx,
                commit_syncer,
                target_repo_dbs,
                BacksyncLimit::NoLimit,
                cancellation_requested,
                CommitSyncContext::Backsyncer,
                false,
            )
            .boxed()
            .await?;
        }
        (ARG_MODE_BACKSYNC_FOREVER, _) => {
            let db_config = target_repo.config.storage_config.metadata;
            let ctx = session_container
                .new_context(logger.clone(), MononokeScubaSampleBuilder::with_discard());
            let target_repo_dbs = Arc::new(
                open_backsyncer_dbs(
                    ctx,
                    commit_syncer.get_target_repo().clone(),
                    db_config,
                    mysql_options.clone(),
                    *readonly_storage,
                )
                .boxed()
                .await?,
            );

            let mut scuba_sample = MononokeScubaSampleBuilder::new(fb, SCUBA_TABLE)?;
            scuba_sample.add("source_repo", source_repo.id.id());
            scuba_sample.add("source_repo_name", source_repo.name.clone());
            scuba_sample.add("target_repo", target_repo.id.id());
            scuba_sample.add("target_repo_name", target_repo.name.clone());
            scuba_sample.add_common_server_data();

            let ctx = session_container.new_context(logger.clone(), scuba_sample);
            let f = backsync_forever(
                ctx,
                commit_syncer,
                target_repo_dbs,
                source_repo.name,
                target_repo.name,
                live_commit_sync_config,
                cancellation_requested,
            )
            .boxed();
            f.await?;
        }
        (ARG_MODE_BACKSYNC_COMMITS, Some(sub_m)) => {
            let ctx = session_container
                .new_context(logger.clone(), MononokeScubaSampleBuilder::with_discard());
            let inputfile = sub_m
                .value_of(ARG_INPUT_FILE)
                .expect("input file is not set");
            let inputfile = File::open(inputfile)?;
            let file = BufReader::new(&inputfile);
            let batch_size = args::get_usize(matches.as_ref(), ARG_BATCH_SIZE, 100);

            let source_repo = commit_syncer.get_source_repo().clone();

            let mut hg_cs_ids = vec![];
            for line in file.lines() {
                hg_cs_ids.push(HgChangesetId::from_str(&line?)?);
            }
            let total_to_backsync = hg_cs_ids.len();
            info!(ctx.logger(), "backsyncing {} commits", total_to_backsync);

            let ctx = &ctx;
            let commit_syncer = &commit_syncer;

            // Before processing each commit, check if cancellation has
            // been requested and exit if that's the case.
            if cancellation_requested.load(Ordering::Relaxed) {
                info!(ctx.logger(), "sync stopping due to cancellation request");
                return Ok(());
            }
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
                                        ctx,
                                        bonsai.clone(),
                                        CandidateSelectionHint::Only,
                                        CommitSyncContext::Backsyncer,
                                        false,
                                    )
                                    .await?;

                                let maybe_sync_outcome =
                                    commit_syncer.get_commit_sync_outcome(ctx, bonsai).await?;

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

            f.await?;
        }
        _ => {
            bail!("unknown subcommand");
        }
    }

    Ok(())
}
