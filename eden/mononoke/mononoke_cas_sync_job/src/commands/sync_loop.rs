/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::bail;
use anyhow::format_err;
use assembly_line::TryAssemblyLine;
use async_trait::async_trait;
use bookmarks::BookmarkUpdateLogArc;
use borrowed::borrowed;
use cas_client::build_mononoke_cas_client;
use changesets_uploader::CasChangesetsUploader;
use clap::Parser;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use context::CoreContext;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use fbinit::FacebookInit;
use futures::future;
use futures::future::TryFutureExt;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_retry::retry;
use futures_stats::futures03::TimedFutureExt;
use futures_watchdog::WatchdogExt;
use metaconfig_types::RepoConfigRef;
use mononoke_app::MononokeApp;
use mononoke_app::args::RepoArg;
use repo_identity::RepoIdentityRef;
use repourl::encode_repo_name;
use scuba_ext::MononokeScubaSampleBuilder;
use sharding_ext::RepoShard;
use slog::error;
use slog::info;
use tracing::Instrument;
use zk_leader_election::LeaderElection;
use zk_leader_election::ZkMode;

use crate::CasSyncArgs;
use crate::CombinedBookmarkUpdateLogEntry;
use crate::LatestReplayedSyncCounter;
use crate::Repo;
use crate::bind_sync_result;
use crate::build_outcome_handler;
use crate::build_reporting_handler;
use crate::get_id_to_search_after;
use crate::loop_over_log_entries;
use crate::try_sync_single_combined_entry;

const JOB_NAME: &str = "mononoke_cas_sync_job";
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(1);
const DEFAULT_EXECUTION_RETRY_NUM: usize = 1;
const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;
const SCUBA_TABLE: &str = "mononoke_cas_sync";
const LATEST_REPLAYED_REQUEST_KEY: &str = "latest-replayed-request-cas";
const DEFAULT_BATCH_SIZE: u64 = 10;

#[derive(Parser)]
// Replays bookmark's moves
pub struct CommandArgs {
    #[clap(
        long = "start-id",
        help = "if current counter is not set then `start-id` will be used"
    )]
    start_id: Option<u64>,
    #[clap(
        long = "batch-size",
        help = "how many entries from the bookmark update log to process in one batch"
    )]
    batch_size: Option<u64>,
    #[clap(
        long = "loop-forever",
        help = "If set job will loop forever even if there are no new entries in db or if there was an error"
    )]
    loop_forever: bool,
    #[clap(
        long = "exit-file",
        help = "If you provide this argument, the sync loop will gracefully exit once this file exists"
    )]
    exit_file: Option<PathBuf>,
}

/// Struct representing the Mononoke to CAS sync.
pub struct MononokeCasSyncProcess {
    app: Arc<MononokeApp>,
    args: Arc<CommandArgs>,
}

impl MononokeCasSyncProcess {
    fn new(app: MononokeApp, args: CommandArgs) -> Result<Self> {
        Ok(Self {
            app: Arc::new(app),
            args: Arc::new(args),
        })
    }
}

#[async_trait]
impl RepoShardedProcess for MononokeCasSyncProcess {
    async fn setup(&self, repo: &RepoShard) -> anyhow::Result<Arc<dyn RepoShardedProcessExecutor>> {
        let repo_name = repo.repo_name.as_str();
        let logger = self.app.logger().clone();

        info!(
            logger,
            "Setting up mononoke cas sync command for repo {}", repo_name
        );
        let executor = MononokeCasSyncProcessExecutor::new(
            self.app.clone(),
            repo_name.to_string(),
            self.args.clone(),
        )?;
        info!(
            logger,
            "Completed mononoke cas sync command setup for repo {}", repo_name
        );
        Ok(Arc::new(executor))
    }
}

/// Struct representing the execution of the Mononoke RE CAS Sync.
/// BP over the context of a provided repo.
pub struct MononokeCasSyncProcessExecutor {
    fb: FacebookInit,
    app: Arc<MononokeApp>,
    args: Arc<CommandArgs>,
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
    fn new(app: Arc<MononokeApp>, repo_name: String, args: Arc<CommandArgs>) -> Result<Self> {
        let ctx = CoreContext::new_with_logger_and_client_info(
            app.fb,
            app.logger().clone(),
            ClientInfo::default_with_entry_point(ClientEntryPoint::MononokeCasSync),
        )
        .clone_with_repo_name(&repo_name);

        Ok(Self {
            fb: app.fb,
            app,
            args,
            ctx,
            repo_name,
            cancellation_requested: Arc::new(AtomicBool::new(false)),
        })
    }

    async fn do_execute(&self) -> anyhow::Result<()> {
        async {
            info!(
                self.ctx.logger(),
                "Initiating mononoke RE CAS sync command execution",
            );

            let args = self.app.args::<CasSyncArgs>()?;

            let base_retry_delay = args
                .base_retry_delay_ms
                .map_or(DEFAULT_RETRY_DELAY, Duration::from_millis);

            let retry_num = args.retry_num.unwrap_or(DEFAULT_EXECUTION_RETRY_NUM);

            let mode: ZkMode = args.leader_only.into();

            retry(
                async |attempt| {
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
                                run_sync(
                                    attempt,
                                    self.fb,
                                    &self.ctx,
                                    self.app.clone(),
                                    self.args.clone(),
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
                base_retry_delay,
            ).binary_exponential_backoff()
            .max_attempts(
            retry_num)
    .inspect_err(|attempt, _err| info!(self.ctx.logger(), "attempt {attempt} of {retry_num} failed"))
            .await?;
            info!(
                self.ctx.logger(),
                "Finished mononoke RE CAS sync command execution for repo {}", &self.repo_name,
            );
            Ok(())
    }.instrument(tracing::info_span!("execute", repo = %self.repo_name))
    .await
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

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let process = Arc::new(MononokeCasSyncProcess::new(app, args)?);
    let app_args = &process.app.args::<CasSyncArgs>()?;
    let logger = process.app.logger().clone();

    if let Some(executor) = app_args.sharded_executor_args.clone().build_executor(
        process.app.fb,
        process.app.runtime().clone(),
        &logger,
        || process.clone(),
        true, // enable shard (repo) level healing
        SM_CLEANUP_TIMEOUT_SECS,
    )? {
        slog::info!(logger, "Running sharded sync loop");
        let (sender, receiver) = tokio::sync::oneshot::channel::<bool>();
        executor.block_and_execute(&logger, receiver).await?;
        drop(sender);
        Ok(())
    } else {
        let repo_arg = app_args
            .repo
            .as_repo_arg()
            .clone()
            .ok_or(anyhow::anyhow!("Running unsharded mode with no repo arg"))?;
        let repo: Repo = process.app.clone().open_repo(&repo_arg).await?;
        let repo_name = repo.repo_identity.name().to_string();

        let executor = MononokeCasSyncProcessExecutor::new(
            process.app.clone(),
            repo_name.to_string(),
            process.args.clone(),
        )?;

        executor.do_execute().await
    }
}

async fn run_sync(
    attempt_num: usize,
    fb: FacebookInit,
    ctx: &CoreContext,
    app: Arc<MononokeApp>,
    args: Arc<CommandArgs>,
    repo_name: String,
    cancellation_requested: Arc<AtomicBool>,
) -> Result<(), Error> {
    let repo: Repo = app.open_repo(&RepoArg::Name(repo_name.clone())).await?;

    let sync_config = repo
        .repo_config()
        .mononoke_cas_sync_config
        .as_ref()
        .ok_or_else(|| {
            format_err!(
                "mononoke_cas_sync_config is not found for the repo {}",
                repo_name
            )
        })?;

    let re_cas_client = CasChangesetsUploader::new(build_mononoke_cas_client(
        fb,
        ctx.clone(),
        &repo_name,
        false,
        &sync_config.use_case_public,
    )?);

    info!(
        ctx.logger(),
        "using repo \"{}\" repoid {:?}",
        repo.repo_identity().name(),
        repo.repo_identity().id()
    );

    let log_to_scuba = app.args::<CasSyncArgs>()?.log_to_scuba;
    let mut scuba_sample = if log_to_scuba {
        MononokeScubaSampleBuilder::new(ctx.fb, SCUBA_TABLE)?
    } else {
        MononokeScubaSampleBuilder::with_discard()
    };

    scuba_sample.add_common_server_data();
    scuba_sample.add("repo_name", repo_name.clone());

    let reporting_handler = build_reporting_handler(
        ctx,
        &scuba_sample,
        attempt_num,
        repo.bookmark_update_log_arc(),
    );

    let main_bookmark_to_sync = sync_config.main_bookmark_to_sync.as_str();
    let sync_all_bookmarks = sync_config.sync_all_bookmarks;

    // Before beginning any actual processing, check if cancellation has been requested.
    // If yes, then lets return early.
    if cancellation_requested.load(Ordering::Relaxed) {
        info!(ctx.logger(), "sync stopping due to cancellation request");
        return Ok(());
    }

    let loop_forever = args.loop_forever;
    let start_id = args.start_id;
    let exit_path = args.exit_file.clone();
    let batch_size = args.batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
    let replayed_sync_counter = LatestReplayedSyncCounter::new(&repo)?;

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

    let start_id = bookmarks::BookmarkUpdateLogId(
        replayed_sync_counter
            .get_counter(ctx)
            .and_then(move |maybe_counter| {
                future::ready(
                    maybe_counter
                        .map(|counter| counter.try_into().expect("Counter must be positive"))
                        .or(start_id)
                        .ok_or_else(|| {
                            format_err!(
                                "{} counter not found. Pass `--start-id` flag to set the counter",
                                LATEST_REPLAYED_REQUEST_KEY
                            )
                        }),
                )
            })
            .await?,
    );

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
        repo.bookmark_update_log_arc(),
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
                    if sync_all_bookmarks || entry.bookmark_name.as_str() == main_bookmark_to_sync {
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
