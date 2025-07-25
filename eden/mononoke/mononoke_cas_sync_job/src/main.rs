/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Mononoke -> cas sync job

#![feature(auto_traits)]

use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use anyhow::Result;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::Freshness;
use cas_client::CasClient;
use changesets_uploader::CasChangesetsUploader;
use clap::Parser;
use cloned::cloned;
use commit_graph::CommitGraph;
use context::CoreContext;
use executor_lib::args::ShardedExecutorArgs;
use fbinit::FacebookInit;
use futures::Stream;
use futures::future::BoxFuture;
use futures::future::FutureExt as _;
use futures::stream;
use futures::stream::TryStreamExt;
use futures_stats::FutureStats;
use futures_watchdog::WatchdogExt;
use metaconfig_types::RepoConfig;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use mononoke_app::args::OptRepoArgs;
use mononoke_app::monitoring::AliveService;
use mononoke_app::monitoring::MonitoringAppExtension;
use mononoke_types::RepositoryId;
use mutable_counters::ArcMutableCounters;
use mutable_counters::MutableCounters;
use mutable_counters::MutableCountersArc;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use scuba_ext::MononokeScubaSampleBuilder;
use slog::error;
use slog::info;

mod commands;
mod errors;
mod re_cas_sync;

use crate::errors::ErrorKind::SyncFailed;
use crate::errors::PipelineError;
use crate::errors::PipelineError::AnonymousError;
use crate::errors::PipelineError::EntryError;

const LATEST_REPLAYED_REQUEST_KEY: &str = "latest-replayed-request-cas";
const SLEEP_SECS: u64 = 1;

#[derive(Copy, Clone)]
struct QueueSize(usize);

#[facet::container]
#[derive(Clone)]
pub struct Repo {
    #[facet]
    pub commit_graph: CommitGraph,

    #[facet]
    pub bonsai_hg_mapping: dyn BonsaiHgMapping,

    #[facet]
    pub repo_derived_data: RepoDerivedData,

    #[facet]
    pub mutable_counters: dyn MutableCounters,

    #[facet]
    pub repo_identity: RepoIdentity,

    #[init(repo_identity.id())]
    pub repoid: RepositoryId,

    #[init(repo_identity.name().to_string())]
    pub repo_name: String,

    #[facet]
    pub repo_blobstore: RepoBlobstore,

    #[facet]
    pub repo_config: RepoConfig,

    #[facet]
    pub bookmark_update_log: dyn BookmarkUpdateLog,
}

#[derive(Parser)]
pub struct CasSyncArgs {
    #[clap(
        long,
        help = "If set job will log individual bundle sync states to Scuba"
    )]
    log_to_scuba: bool,

    #[clap(
        long,
        help = "Initial delay between failures. It will be increased on the successive attempts"
    )]
    base_retry_delay_ms: Option<u64>,

    #[clap(long = "retry-num", help = "How many times to retry the execution")]
    retry_num: Option<usize>,

    #[clap(
        long = "leader-only",
        help = "If leader election is enabled, only one instance of the job will be running at a time for a repo"
    )]
    leader_only: bool,

    #[clap(flatten)]
    sharded_executor_args: ShardedExecutorArgs,

    #[clap(flatten)]
    pub repo: OptRepoArgs,
}

pub struct PipelineState<T> {
    entries: Vec<BookmarkUpdateLogEntry>,
    data: T,
}

pub type OutcomeWithStats =
    Result<(FutureStats, PipelineState<usize>), (Option<FutureStats>, PipelineError)>;

pub type Outcome = Result<PipelineState<usize>, PipelineError>;

pub fn get_id_to_search_after(entries: &[BookmarkUpdateLogEntry]) -> BookmarkUpdateLogId {
    entries
        .iter()
        .map(|entry| entry.id)
        .max()
        .unwrap_or(0.into())
}

fn bind_sync_err(entries: &[BookmarkUpdateLogEntry], cause: Error) -> PipelineError {
    let ids: Vec<_> = entries.iter().map(|entry| entry.id).collect();
    let entries = entries.to_vec();
    EntryError {
        entries,
        cause: (SyncFailed { ids, cause }).into(),
    }
}

pub fn bind_sync_result<T>(
    entries: &[BookmarkUpdateLogEntry],
    res: Result<T>,
) -> Result<PipelineState<T>, PipelineError> {
    match res {
        Ok(data) => Ok(PipelineState {
            entries: entries.to_vec(),
            data,
        }),
        Err(cause) => Err(bind_sync_err(entries, cause)),
    }
}

fn drop_outcome_stats(o: OutcomeWithStats) -> Outcome {
    o.map(|(_, r)| r).map_err(|(_, e)| e)
}

pub fn build_reporting_handler<'a>(
    ctx: &'a CoreContext,
    scuba_sample: &'a MononokeScubaSampleBuilder,
    attempt_num: usize,
    bookmarks: Arc<dyn BookmarkUpdateLog>,
) -> impl Fn(OutcomeWithStats) -> BoxFuture<'a, Result<PipelineState<usize>, PipelineError>> {
    move |res| {
        cloned!(bookmarks);
        async move {
            let log_entries = match &res {
                Ok((_, pipeline_state, ..)) => Some(pipeline_state.entries.clone()),
                Err((_, EntryError { entries, .. })) => Some(entries.clone()),
                Err((_, AnonymousError { .. })) => None,
            };

            let maybe_stats = match &res {
                Ok((stats, _)) => Some(stats),
                Err((stats, _)) => stats.as_ref(),
            };

            let attempts = match &res {
                Ok((_, PipelineState { data: attempts, .. })) => attempts.clone(),
                Err(..) => attempt_num,
            };

            let maybe_error = match &res {
                Ok(..) => None,
                Err((_, EntryError { cause, .. })) => Some(cause),
                Err((_, AnonymousError { cause, .. })) => Some(cause),
            };

            let f = async {
                if let Some(log_entries) = log_entries {
                    let duration =
                        maybe_stats.map_or_else(|| Duration::from_secs(0), |s| s.completion_time);

                    let error = maybe_error.map(|e| format!("{:?}", e));
                    let next_id = get_id_to_search_after(&log_entries);

                    let n = bookmarks
                        .count_further_bookmark_log_entries(ctx.clone(), next_id, None)
                        .await?;
                    let queue_size = QueueSize(n as usize);
                    info!(
                        ctx.logger(),
                        "queue size after processing: {}", queue_size.0
                    );
                    log_processed_entries_to_scuba(
                        &log_entries,
                        scuba_sample.clone(),
                        error,
                        attempts,
                        duration,
                        queue_size,
                    );
                }
                Result::<_, Error>::Ok(())
            };

            // Ignore result from future that did the logging
            let _ = f.await;
            drop_outcome_stats(res)
        }
        .boxed()
    }
}

pub fn build_outcome_handler<'a>(
    ctx: &'a CoreContext,
) -> impl Fn(Outcome) -> BoxFuture<'a, Result<Vec<BookmarkUpdateLogEntry>, Error>> {
    move |res| {
        async move {
            match res {
                Ok(PipelineState { entries, .. }) => {
                    info!(
                        ctx.logger(),
                        "successful sync of entries {:?}",
                        entries.iter().map(|c| c.id).collect::<Vec<_>>()
                    );
                    Ok(entries)
                }
                Err(AnonymousError { cause: e }) => {
                    error!(ctx.logger(), "Error without entry: {:?}", e);
                    Err(e)
                }
                Err(EntryError { cause: e, .. }) => Err(e),
            }
        }
        .boxed()
    }
}

#[derive(Clone)]
pub struct CombinedBookmarkUpdateLogEntry {
    components: Vec<BookmarkUpdateLogEntry>,
}

/// Sends commits to CAS while syncing a set of bookmark update log entries.
pub async fn try_sync_single_combined_entry<'a>(
    re_cas_client: &CasChangesetsUploader<impl CasClient + 'a>,
    repo: &'a Repo,
    ctx: &'a CoreContext,
    combined_entry: &'a CombinedBookmarkUpdateLogEntry,
    main_bookmark: &'a str,
) -> Result<usize, Error> {
    re_cas_sync::try_sync_single_combined_entry(
        re_cas_client,
        repo,
        ctx,
        combined_entry,
        main_bookmark,
    )
    .await
}

/// Logs to Scuba information about a single sync event
fn log_processed_entry_to_scuba(
    log_entry: &BookmarkUpdateLogEntry,
    mut scuba_sample: MononokeScubaSampleBuilder,
    error: Option<String>,
    attempts: usize,
    duration: Duration,
    queue_size: QueueSize,
) {
    let entry = log_entry.id;
    let book = format!("{}", log_entry.bookmark_name);
    let reason = format!("{}", log_entry.reason);
    let delay = log_entry.timestamp.since_seconds();

    scuba_sample
        .add("entry", u64::from(entry))
        .add("bookmark", book)
        .add("reason", reason)
        .add("attempts", attempts)
        .add("duration", duration.as_millis() as i64);

    match error {
        Some(error) => {
            scuba_sample.add("success", 0).add("err", error);
        }
        None => {
            scuba_sample.add("success", 1).add("delay", delay);
            scuba_sample.add("queue_size", queue_size.0);
        }
    };

    scuba_sample.log();
}

fn log_processed_entries_to_scuba(
    entries: &[BookmarkUpdateLogEntry],
    scuba_sample: MononokeScubaSampleBuilder,
    error: Option<String>,
    attempts: usize,
    duration: Duration,
    queue_size: QueueSize,
) {
    let n: f64 = entries.len() as f64;
    let individual_duration = duration.div_f64(n);
    entries.iter().for_each(|entry| {
        log_processed_entry_to_scuba(
            entry,
            scuba_sample.clone(),
            error.clone(),
            attempts,
            individual_duration,
            queue_size,
        )
    });
}

pub fn loop_over_log_entries<'a>(
    ctx: &'a CoreContext,
    bookmarks: Arc<dyn BookmarkUpdateLog>,
    start_id: BookmarkUpdateLogId,
    loop_forever: bool,
    scuba_sample: &'a MononokeScubaSampleBuilder,
    batch_size: u64,
) -> impl Stream<Item = Result<Vec<BookmarkUpdateLogEntry>, Error>> + 'a {
    stream::try_unfold(Some(start_id), {
        move |maybe_id| {
            cloned!(ctx, bookmarks);
            async move {
                match maybe_id {
                    Some(current_id) => {
                        let entries = bookmarks
                            .read_next_bookmark_log_entries(
                                ctx.clone(),
                                current_id,
                                batch_size,
                                Freshness::MostRecent,
                            )
                            .try_collect::<Vec<_>>()
                            .watched(ctx.logger())
                            .await?;

                        match entries.iter().last().cloned() {
                            None => {
                                if loop_forever {
                                    info!(ctx.logger(), "id: {}, no new entries found", current_id);
                                    scuba_sample.clone().add("success", 1).add("delay", 0).log();

                                    // First None means that no new entries will be added to the stream,
                                    // Some(current_id) means that bookmarks will be fetched again
                                    tokio::time::sleep(Duration::new(SLEEP_SECS, 0)).await;

                                    Ok(Some((vec![], Some(current_id))))
                                } else {
                                    Ok(Some((vec![], None)))
                                }
                            }
                            Some(last_entry) => Ok(Some((entries, Some(last_entry.id)))),
                        }
                    }
                    None => Ok(None),
                }
            }
        }
    })
}

pub struct LatestReplayedSyncCounter {
    mutable_counters: ArcMutableCounters,
}

impl LatestReplayedSyncCounter {
    fn new(source_repo: &Repo) -> Result<Self, Error> {
        let mutable_counters = source_repo.mutable_counters_arc();
        Ok(Self { mutable_counters })
    }

    async fn get_counter(&self, ctx: &CoreContext) -> Result<Option<i64>, Error> {
        self.mutable_counters
            .get_counter(ctx, LATEST_REPLAYED_REQUEST_KEY)
            .await
    }

    async fn set_counter(&self, ctx: &CoreContext, value: i64) -> Result<bool, Error> {
        self.mutable_counters
            .set_counter(ctx, LATEST_REPLAYED_REQUEST_KEY, value, None)
            .await
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let subcommands = commands::subcommands();
    let app = MononokeAppBuilder::new(fb)
        .with_app_extension(MonitoringAppExtension {})
        .build_with_subcommands::<CasSyncArgs>(subcommands)?;
    app.run_with_monitoring_and_logging(async_main, "Mononoke -> CAS sync job", AliveService)
}

async fn async_main(app: MononokeApp) -> Result<()> {
    commands::dispatch(app).await
}
