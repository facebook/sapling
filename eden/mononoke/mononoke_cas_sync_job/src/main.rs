/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Mononoke -> cas sync job

#![feature(auto_traits)]
#![feature(async_closure)]

use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::sync::OnceLock;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use assembly_line::TryAssemblyLine;
use async_trait::async_trait;
use bonsai_hg_mapping::BonsaiHgMapping;
use bookmarks::BookmarkUpdateLog;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::Freshness;
use borrowed::borrowed;
use cas_client::build_mononoke_cas_client;
use cas_client::CasClient;
use changesets_uploader::CasChangesetsUploader;
use clap_old::Arg;
use clap_old::SubCommand;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use cloned::cloned;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use cmdlib::helpers::block_execute;
use commit_graph::CommitGraph;
use context::CoreContext;
use dbbookmarks::SqlBookmarksBuilder;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use executor_lib::ShardedProcessExecutor;
use fbinit::FacebookInit;
use futures::future;
use futures::future::BoxFuture;
use futures::future::FutureExt as _;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures::Stream;
use futures_stats::futures03::TimedFutureExt;
use futures_stats::FutureStats;
use futures_watchdog::WatchdogExt;
use metaconfig_types::RepoConfig;
use mononoke_types::RepositoryId;
use mutable_counters::ArcMutableCounters;
use mutable_counters::MutableCounters;
use mutable_counters::MutableCountersArc;
use repo_blobstore::RepoBlobstore;
use repo_derived_data::RepoDerivedData;
use repo_identity::RepoIdentity;
use repourl::encode_repo_name;
use retry::retry_always;
use retry::RetryAttemptsCount;
use scuba_ext::MononokeScubaSampleBuilder;
use sharding_ext::RepoShard;
use slog::error;
use slog::info;
use tokio::runtime::Runtime;
use zk_leader_election::LeaderElection;
use zk_leader_election::ZkMode;

mod errors;
mod re_cas_sync;

use crate::errors::ErrorKind::SyncFailed;
use crate::errors::PipelineError;
use crate::errors::PipelineError::AnonymousError;
use crate::errors::PipelineError::EntryError;

const MODE_SYNC_LOOP: &str = "sync-loop";
const LATEST_REPLAYED_REQUEST_KEY: &str = "latest-replayed-request-cas";
const SLEEP_SECS: u64 = 1;
const SCUBA_TABLE: &str = "mononoke_cas_sync";
const JOB_NAME: &str = "mononoke_cas_sync_job";

const DEFAULT_EXECUTION_RETRY_NUM: usize = 1;
const DEFAULT_RETRY_DELAY_MS: u64 = 1000;
const DEFAULT_BATCH_SIZE: u64 = 10;

#[derive(Copy, Clone)]
struct QueueSize(usize);

const SM_SERVICE_SCOPE: &str = "global";
const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;

/// Struct representing the Mononoke to CAS sync.
pub struct MononokeCasSyncProcess {
    matches: Arc<MononokeMatches<'static>>,
    fb: FacebookInit,
    _runtime: Runtime,
}

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
}

impl MononokeCasSyncProcess {
    fn new(fb: FacebookInit) -> Result<Self> {
        let app = args::MononokeAppBuilder::new("Mononoke -> CAS sync job")
        .with_advanced_args_hidden()
        .with_fb303_args()
        .with_dynamic_repos()
        .build()
        .arg(
            Arg::with_name("log-to-scuba")
                .long("log-to-scuba")
                .takes_value(false)
                .required(false)
                .help("If set job will log individual bundle sync states to Scuba"),
        )
        .arg(
            Arg::with_name("base-retry-delay-ms")
                .long("base-retry-delay-ms")
                .takes_value(true)
                .required(false)
                .help("initial delay between failures. It will be increased on the successive attempts")
        )
        .arg(
            Arg::with_name("retry-num")
                .long("retry-num")
                .takes_value(true)
                .required(false)
                .help("how many times to retry the execution")
        )
        .arg(
            Arg::with_name("leader-only")
                .long("leader-only")
                .takes_value(false)
                .required(false)
                .help(
                    "If leader election is enabled, only one instance of the job will be running at a time for a repo",
                )
        )
        .about(
            "Special job that takes commits that were sent to Mononoke and \
             send their files and (augmented) trees to cas",
        );

        let sync_loop = SubCommand::with_name(MODE_SYNC_LOOP)
            .about("Replays bookmark's moves")
            .arg(
                Arg::with_name("start-id")
                    .long("start-id")
                    .takes_value(true)
                    .help("if current counter is not set then `start-id` will be used"),
            )
            .arg(
                Arg::with_name("batch-size")
                    .long("batch-size")
                    .takes_value(true)
                    .required(false)
                    .help("how many entries from the bookmark update log to process in one batch"),
            )
            .arg(
                Arg::with_name("loop-forever")
                    .long("loop-forever")
                    .takes_value(false)
                    .required(false)
                    .help(
                        "If set job will loop forever even if there are no new entries in db or \
                     if there was an error",
                    ),
            )
            .arg(
                Arg::with_name("exit-file")
                    .long("exit-file")
                    .takes_value(true)
                    .required(false)
                    .help(
                        "If you provide this argument, the sync loop will gracefully exit \
                     once this file exists",
                    ),
            );

        let app = app.subcommand(sync_loop);

        let (matches, _runtime) = app.get_matches(fb)?;
        let matches = Arc::new(matches);
        Ok(Self {
            matches,
            fb,
            _runtime,
        })
    }
}

#[async_trait]
impl RepoShardedProcess for MononokeCasSyncProcess {
    async fn setup(&self, repo: &RepoShard) -> anyhow::Result<Arc<dyn RepoShardedProcessExecutor>> {
        let repo_name = repo.repo_name.as_str();
        info!(
            self.matches.logger(),
            "Setting up mononoke cas sync command for repo {}", repo_name
        );
        let executor = MononokeCasSyncProcessExecutor::new(
            self.fb,
            Arc::clone(&self.matches),
            repo_name.to_string(),
        )?;
        info!(
            self.matches.logger(),
            "Completed mononoke cas sync command setup for repo {}", repo_name
        );
        Ok(Arc::new(executor))
    }
}

/// Struct representing the execution of the Mononoke RE CAS Sync.
/// BP over the context of a provided repo.
pub struct MononokeCasSyncProcessExecutor {
    fb: FacebookInit,
    matches: Arc<MononokeMatches<'static>>,
    ctx: CoreContext,
    cancellation_requested: Arc<AtomicBool>,
    repo_name: String,
}

#[async_trait]
impl LeaderElection for MononokeCasSyncProcessExecutor {
    fn get_shared_lock_path(&self) -> String {
        format!("{}_{}", JOB_NAME, encode_repo_name(self.repo_name.clone()))
    }
}

impl MononokeCasSyncProcessExecutor {
    fn new(
        fb: FacebookInit,
        matches: Arc<MononokeMatches<'static>>,
        repo_name: String,
    ) -> Result<Self> {
        let ctx = CoreContext::new_with_logger_and_client_info(
            fb,
            matches.logger().clone(),
            ClientInfo::default_with_entry_point(ClientEntryPoint::MononokeCasSync),
        )
        .clone_with_repo_name(&repo_name);
        Ok(Self {
            fb,
            matches,
            ctx,
            repo_name,
            cancellation_requested: Arc::new(AtomicBool::new(false)),
        })
    }

    async fn do_execute(&self) -> anyhow::Result<()> {
        info!(
            self.ctx.logger(),
            "Initiating mononoke RE CAS sync command execution for repo {}", &self.repo_name,
        );
        let base_retry_delay_ms = args::get_u64_opt(self.matches.as_ref(), "base-retry-delay-ms")
            .unwrap_or(DEFAULT_RETRY_DELAY_MS);
        let retry_num = args::get_usize(
            self.matches.as_ref(),
            "retry-num",
            DEFAULT_EXECUTION_RETRY_NUM,
        );
        let mode: ZkMode = self.matches.as_ref().is_present("leader-only").into();

        retry_always(
            self.ctx.logger(),
            |attempt| async move {
                // Once cancellation is requested, do not retry even if its
                // a retryable error.
                if self.cancellation_requested.load(Ordering::Relaxed) {
                    info!(
                        self.ctx.logger(),
                        "sync stopping due to cancellation request at attempt {}", attempt
                    );
                } else {
                    match self.maybe_become_leader(mode, self.ctx.logger().clone()).await {
                        Ok(_leader_token) => {
                            run(
                                attempt,
                                self.fb,
                                &self.ctx,
                                &self.matches,
                                self.repo_name.clone(),
                                Arc::clone(&self.cancellation_requested),
                            )
                            .await
                            .with_context(|| {
                                format!(
                                    "Error during mononoke RE CAS sync command execution for repo {}. Attempt number {}",
                                    &self.repo_name, attempt
                                )
                            })?;
                        },
                        Err(e) => {
                            error!(self.ctx.logger(), "Failed to become leader {:#}", e);
                        }
                    }
                }
                anyhow::Ok(())
            },
            base_retry_delay_ms,
            retry_num,
        )
        .await?;
        info!(
            self.ctx.logger(),
            "Finished mononoke RE CAS sync command execution for repo {}", &self.repo_name,
        );
        Ok(())
    }
}

#[async_trait]
impl RepoShardedProcessExecutor for MononokeCasSyncProcessExecutor {
    async fn execute(&self) -> anyhow::Result<()> {
        self.do_execute().await
    }

    async fn stop(&self) -> anyhow::Result<()> {
        info!(
            self.ctx.logger(),
            "Terminating mononoke RE CAS sync command execution for repo {}", &self.repo_name,
        );
        self.cancellation_requested.store(true, Ordering::Relaxed);
        Ok(())
    }
}

struct PipelineState<T> {
    entries: Vec<BookmarkUpdateLogEntry>,
    data: T,
}

type OutcomeWithStats =
    Result<(FutureStats, PipelineState<RetryAttemptsCount>), (Option<FutureStats>, PipelineError)>;

type Outcome = Result<PipelineState<RetryAttemptsCount>, PipelineError>;

fn get_id_to_search_after(entries: &[BookmarkUpdateLogEntry]) -> BookmarkUpdateLogId {
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

fn bind_sync_result<T>(
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

fn build_reporting_handler<'a, B>(
    ctx: &'a CoreContext,
    scuba_sample: &'a MononokeScubaSampleBuilder,
    attempt_num: usize,
    bookmarks: &'a B,
) -> impl Fn(OutcomeWithStats) -> BoxFuture<'a, Result<PipelineState<RetryAttemptsCount>, PipelineError>>
where
    B: BookmarkUpdateLog,
{
    move |res| {
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
                Err(..) => RetryAttemptsCount(attempt_num),
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

fn build_outcome_handler<'a>(
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
async fn try_sync_single_combined_entry<'a>(
    re_cas_client: &CasChangesetsUploader<impl CasClient + 'a>,
    repo: &'a Repo,
    ctx: &'a CoreContext,
    combined_entry: &'a CombinedBookmarkUpdateLogEntry,
    main_bookmark: &'a str,
) -> Result<RetryAttemptsCount, Error> {
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
    attempts: RetryAttemptsCount,
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
        .add("attempts", attempts.0)
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
    attempts: RetryAttemptsCount,
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

fn loop_over_log_entries<'a, B>(
    ctx: &'a CoreContext,
    bookmarks: &'a B,
    start_id: BookmarkUpdateLogId,
    loop_forever: bool,
    scuba_sample: &'a MononokeScubaSampleBuilder,
    batch_size: u64,
) -> impl Stream<Item = Result<Vec<BookmarkUpdateLogEntry>, Error>> + 'a
where
    B: BookmarkUpdateLog + Clone,
{
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

struct LatestReplayedSyncCounter {
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

async fn run<'a>(
    attempt_num: usize,
    fb: FacebookInit,
    ctx: &CoreContext,
    matches: &'a MononokeMatches<'a>,
    repo_name: String,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<(), Error> {
    let re_cas_client =
        CasChangesetsUploader::new(build_mononoke_cas_client(fb, ctx, &repo_name, false)?);
    let resolved_repo = args::resolve_repo_by_name(matches.config_store(), matches, &repo_name)
        .with_context(|| format!("Invalid repo name provided: {}", &repo_name))?;

    let repo_id = resolved_repo.id;

    let log_to_scuba = matches.is_present("log-to-scuba");
    let mut scuba_sample = if log_to_scuba {
        MononokeScubaSampleBuilder::new(ctx.fb, SCUBA_TABLE)?
    } else {
        MononokeScubaSampleBuilder::with_discard()
    };
    scuba_sample.add_common_server_data();
    scuba_sample.add("repo_name", repo_name.clone());

    let repo: Repo =
        args::open_repo_by_id_unredacted(ctx.fb, ctx.logger(), matches, repo_id).await?;

    let bookmarks =
        args::open_sql_with_config::<SqlBookmarksBuilder>(ctx.fb, matches, &resolved_repo.config)
            .await?;

    let bookmarks = bookmarks.with_repo_id(repo_id);
    let reporting_handler = build_reporting_handler(ctx, &scuba_sample, attempt_num, &bookmarks);

    let sync_config = resolved_repo
        .config
        .mononoke_cas_sync_config
        .ok_or_else(|| {
            anyhow!(
                "mononoke_cas_sync_config is not found for the repo {}",
                repo_name
            )
        })?;
    let main_bookmark_to_sync = sync_config.main_bookmark_to_sync.as_str();
    let sync_all_bookmarks = sync_config.sync_all_bookmarks;

    // Before beginning any actual processing, check if cancellation has been requested.
    // If yes, then lets return early.
    if cancellation_requested.load(Ordering::Relaxed) {
        info!(ctx.logger(), "sync stopping due to cancellation request");
        return Ok(());
    }
    match matches.subcommand() {
        (MODE_SYNC_LOOP, Some(sub_m)) => {
            let loop_forever = sub_m.is_present("loop-forever");
            let start_id = args::get_u64_opt(&sub_m, "start-id");
            let batch_size = args::get_u64(&sub_m, "batch-size", DEFAULT_BATCH_SIZE);
            let replayed_sync_counter = LatestReplayedSyncCounter::new(&repo)?;
            let exit_path: Option<PathBuf> = sub_m
                .value_of("exit-file")
                .map(|name| Path::new(name).to_path_buf());

            borrowed!(ctx);
            let can_continue = move || {
                let exit_file_exists = match exit_path {
                    Some(ref exit_path) if exit_path.exists() => {
                        info!(ctx.logger(), "path {:?} exists: exiting ...", exit_path);
                        true
                    }
                    _ => false,
                };
                let cancelled = if cancellation_requested.load(Ordering::Relaxed) {
                    info!(ctx.logger(), "sync stopping due to cancellation request");
                    true
                } else {
                    false
                };
                !exit_file_exists && !cancelled
            };

            let start_id = bookmarks::BookmarkUpdateLogId(replayed_sync_counter
                .get_counter(ctx)
                .and_then(move |maybe_counter| {
                    future::ready(maybe_counter.map(|counter| counter.try_into().expect("Counter must be positive")).or(start_id).ok_or_else(|| {
                        format_err!(
                            "{} counter not found. Pass `--start-id` flag to set the counter",
                            LATEST_REPLAYED_REQUEST_KEY
                        )
                    }))
                }).await?);

            let outcome_handler = build_outcome_handler(ctx);
            borrowed!(
                outcome_handler,
                can_continue,
                reporting_handler,
                replayed_sync_counter,
                re_cas_client,
                repo,
            );

            loop_over_log_entries(
                ctx,
                &bookmarks,
                start_id,
                loop_forever,
                &scuba_sample,
                batch_size,
            )
            .try_filter(|entries| future::ready(!entries.is_empty()))
            .fuse()
            .try_next_step(|entries| async move {
                let combined_entry = CombinedBookmarkUpdateLogEntry {
                    components: entries
                        .into_iter()
                        .filter_map(|entry| {
                            if sync_all_bookmarks
                                || entry.bookmark_name.as_str() == main_bookmark_to_sync
                            {
                                Some(entry)
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>(),
                };
                if can_continue() && !combined_entry.components.is_empty() {
                    let (stats, res) = try_sync_single_combined_entry(
                        re_cas_client,
                        repo,
                        ctx,
                        &combined_entry,
                        main_bookmark_to_sync,
                    )
                    .watched(ctx.logger())
                    .timed()
                    .await;

                    let res = bind_sync_result(&combined_entry.components, res);
                    let res = match res {
                        Ok(ok) => Ok((stats, ok)),
                        Err(err) => Err((Some(stats), err)),
                    };
                    let res = reporting_handler(res).watched(ctx.logger()).await;
                    let entry = outcome_handler(res).watched(ctx.logger()).await?;
                    let next_id = get_id_to_search_after(&entry);
                    let success = replayed_sync_counter
                        .set_counter(ctx, next_id.try_into()?)
                        .watched(ctx.logger())
                        .await?;

                    if success {
                        Ok(())
                    } else {
                        bail!("failed to update counter")
                    }
                } else {
                    Ok(())
                }
            })
            .try_collect::<()>()
            .await
        }
        _ => bail!("incorrect mode of operation is specified"),
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let process = MononokeCasSyncProcess::new(fb)?;
    match process.matches.value_of("sharded-service-name") {
        Some(service_name) => {
            // The service name needs to be 'static to satisfy SM contract
            static SM_SERVICE_NAME: OnceLock<String> = OnceLock::new();
            static SM_SERVICE_SCOPE_NAME: OnceLock<String> = OnceLock::new();
            let logger = process.matches.logger().clone();
            let scope_name = process
                .matches
                .value_of("sharded-scope-name")
                .unwrap_or(SM_SERVICE_SCOPE);
            let matches = Arc::clone(&process.matches);
            let mut executor = ShardedProcessExecutor::new(
                process.fb,
                process.matches.runtime().clone(),
                &logger,
                SM_SERVICE_NAME.get_or_init(|| service_name.to_string()),
                SM_SERVICE_SCOPE_NAME.get_or_init(|| scope_name.to_string()),
                SM_CLEANUP_TIMEOUT_SECS,
                Arc::new(process),
                true, // enable shard (repo) level healing
            )?;
            block_execute(
                executor.block_and_execute(&logger, Arc::new(AtomicBool::new(false))),
                fb,
                &std::env::var("TW_JOB_NAME").unwrap_or_else(|_| JOB_NAME.to_string()),
                matches.logger(),
                &matches,
                cmdlib::monitoring::AliveService,
            )?;
        }
        None => {
            let matches = process.matches.clone();
            let (repo_name, _) =
                args::not_shardmanager_compatible::get_config(matches.config_store(), &matches)?;
            let executor = MononokeCasSyncProcessExecutor::new(
                fb,
                Arc::clone(&matches),
                repo_name.to_string(),
            )?;
            block_execute(
                executor.do_execute(),
                fb,
                JOB_NAME,
                matches.logger(),
                &matches,
                cmdlib::monitoring::AliveService,
            )?;
        }
    }
    Ok(())
}
