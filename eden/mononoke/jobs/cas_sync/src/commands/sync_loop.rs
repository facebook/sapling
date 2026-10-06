/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
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
use backsyncer::advance_bookmark_counter;
use backsyncer::get_bookmark_counter_or_initialize;
use backsyncer::list_publishing_bookmarks;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkPrefix;
use bookmarks::BookmarkUpdateLogArc;
use bookmarks::BookmarkUpdateLogEntry;
use bookmarks::BookmarkUpdateLogId;
use bookmarks::Freshness;
use borrowed::borrowed;
use cas_client::CasClient;
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
use tracing::Instrument;
use tracing::error;
use tracing::info;
use zk_leader_election::LeaderElection;
use zk_leader_election::ZkMode;

use crate::CasSyncArgs;
use crate::CombinedBookmarkUpdateLogEntry;
use crate::LatestReplayedSyncCounter;
use crate::Repo;
use crate::SLEEP_SECS;
use crate::bind_sync_result;
use crate::build_outcome_handler;
use crate::build_reporting_handler;
use crate::get_id_to_search_after;
use crate::loop_over_log_entries_until;
use crate::try_sync_single_combined_entry;

const JOB_NAME: &str = "mononoke_cas_sync_job";
const DEFAULT_RETRY_DELAY: Duration = Duration::from_secs(1);
const DEFAULT_EXECUTION_RETRY_NUM: usize = 1;
const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;
const SCUBA_TABLE: &str = "mononoke_cas_sync";
const LATEST_REPLAYED_REQUEST_KEY: &str = "latest-replayed-request-cas";
const DEFAULT_BATCH_SIZE: u64 = 10;
const DEFAULT_BOOKMARK_CONCURRENCY: usize = 100;
const BOOKMARK_POLLING_JUST_KNOB: &str = "scm/mononoke:cas_sync_bookmark_polling";

#[derive(Parser)]
// Replays bookmark's moves
pub struct CommandArgs {
    #[clap(
        long = "use-case",
        help = "CAS use case override. Sync state is isolated by use case when set"
    )]
    use_case: Option<String>,
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

        info!(
            "Setting up mononoke cas sync command for repo {}",
            repo_name
        );
        let executor = MononokeCasSyncProcessExecutor::new(
            self.app.clone(),
            repo_name.to_string(),
            self.args.clone(),
        )?;
        info!(
            "Completed mononoke cas sync command setup for repo {}",
            repo_name
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

fn namespaced_state_key(base: &str, use_case: Option<&str>) -> String {
    match use_case {
        Some(use_case) => format!("{base}-{}", encode_repo_name(use_case)),
        None => base.to_owned(),
    }
}

fn shared_lock_path(repo_name: &str, use_case: Option<&str>) -> String {
    match use_case {
        Some(use_case) => format!(
            "{JOB_NAME}:{}:{}",
            encode_repo_name(repo_name),
            encode_repo_name(use_case),
        ),
        None => format!("{JOB_NAME}_{}", encode_repo_name(repo_name)),
    }
}

async fn sync_combined_entries(
    attempt_num: usize,
    ctx: &CoreContext,
    repo: &Repo,
    re_cas_client: &CasChangesetsUploader<impl CasClient>,
    scuba_sample: &MononokeScubaSampleBuilder,
    main_bookmark: &str,
    entries: Vec<BookmarkUpdateLogEntry>,
) -> Result<Vec<BookmarkUpdateLogEntry>, Error> {
    let combined_entry = CombinedBookmarkUpdateLogEntry {
        components: entries,
    };
    let (stats, res) =
        try_sync_single_combined_entry(re_cas_client, repo, ctx, &combined_entry, main_bookmark)
            .watched()
            .timed()
            .await;

    let res = bind_sync_result(&combined_entry.components, res);
    let res = match res {
        Ok(ok) => Ok((stats, ok)),
        Err(err) => Err((Some(stats), err)),
    };
    let res = build_reporting_handler(
        ctx,
        scuba_sample,
        attempt_num,
        repo.bookmark_update_log_arc(),
    )(res)
    .watched()
    .await;
    build_outcome_handler(ctx)(res).watched().await
}

fn bookmark_polling_enabled(repo_name: &str) -> bool {
    justknobs::eval(BOOKMARK_POLLING_JUST_KNOB, None, Some(repo_name))
}

async fn resolve_start_id(
    ctx: &CoreContext,
    replayed_sync_counter: &LatestReplayedSyncCounter,
    configured_start_id: Option<u64>,
) -> Result<BookmarkUpdateLogId, Error> {
    if let Some(counter) = replayed_sync_counter.get_counter(ctx).await? {
        return Ok(counter.try_into()?);
    }
    configured_start_id.map(BookmarkUpdateLogId).ok_or_else(|| {
        format_err!(
            "{} counter not found. Pass `--start-id` flag to set the counter",
            replayed_sync_counter.counter_name
        )
    })
}

async fn bookmarks_to_sync(
    ctx: &CoreContext,
    repo: &Repo,
    main_bookmark: &str,
    sync_all_bookmarks: bool,
) -> Result<HashSet<BookmarkKey>, Error> {
    let mut bookmarks = if sync_all_bookmarks {
        list_publishing_bookmarks(ctx, repo, &BookmarkPrefix::empty()).await?
    } else {
        HashSet::new()
    };
    // Keep polling the configured main bookmark even if it is temporarily
    // absent so a deletion followed by recreation resumes from the same
    // durable cursor.
    bookmarks.insert(BookmarkKey::new(main_bookmark)?);
    Ok(bookmarks)
}

async fn sync_bookmark_once(
    attempt_num: usize,
    ctx: &CoreContext,
    repo: &Repo,
    re_cas_client: &CasChangesetsUploader<impl CasClient>,
    scuba_sample: &MononokeScubaSampleBuilder,
    main_bookmark: &str,
    base_counter_name: &str,
    legacy_safe_floor: BookmarkUpdateLogId,
    bookmark: BookmarkKey,
    batch_size: u64,
) -> Result<bool, Error> {
    let bookmark_counter =
        LatestReplayedSyncCounter::for_bookmark(repo, base_counter_name, &bookmark)?;
    // The legacy cursor is a proven floor for the same scope that bookmark
    // mode polls: legacy mode advances it only after syncing either the main
    // bookmark or every publishing bookmark, according to
    // `sync_all_bookmarks`. Therefore entries for this bookmark at or below
    // the floor have already reached CAS. Once created, the per-bookmark
    // cursor is authoritative and the helper never fast-forwards it again.
    let Some(mut counter) = get_bookmark_counter_or_initialize(
        ctx,
        &bookmark_counter.mutable_counters,
        &bookmark_counter.counter_name,
        legacy_safe_floor,
    )
    .await?
    else {
        return Ok(false);
    };
    let entries = repo
        .bookmark_update_log_arc()
        .read_next_bookmark_log_entries_by_bookmark(
            ctx.clone(),
            bookmark,
            counter,
            batch_size,
            Freshness::MaybeStale,
        )
        .try_collect::<Vec<_>>()
        .watched()
        .await?;
    if entries.is_empty() {
        return Ok(false);
    }

    let entries = sync_combined_entries(
        attempt_num,
        ctx,
        repo,
        re_cas_client,
        scuba_sample,
        main_bookmark,
        entries,
    )
    .await?;
    let next_id = get_id_to_search_after(&entries);
    advance_bookmark_counter(
        ctx,
        &bookmark_counter.mutable_counters,
        &bookmark_counter.counter_name,
        &mut counter,
        next_id,
    )
    .await?;
    Ok(true)
}

#[derive(Default)]
struct BookmarkSyncOutcome {
    made_progress: bool,
    failed_bookmarks: usize,
}

async fn sync_bookmarks_once(
    attempt_num: usize,
    ctx: &CoreContext,
    repo: &Repo,
    re_cas_client: &CasChangesetsUploader<impl CasClient>,
    scuba_sample: &MononokeScubaSampleBuilder,
    main_bookmark: &str,
    sync_all_bookmarks: bool,
    base_counter_name: &str,
    legacy_safe_floor: BookmarkUpdateLogId,
    batch_size: u64,
) -> Result<BookmarkSyncOutcome, Error> {
    let bookmarks = bookmarks_to_sync(ctx, repo, main_bookmark, sync_all_bookmarks).await?;
    let results = futures::stream::iter(bookmarks)
        .map(|bookmark| async move {
            let result = sync_bookmark_once(
                attempt_num,
                ctx,
                repo,
                re_cas_client,
                scuba_sample,
                main_bookmark,
                base_counter_name,
                legacy_safe_floor,
                bookmark.clone(),
                batch_size,
            )
            .await;
            (bookmark, result)
        })
        .buffer_unordered(DEFAULT_BOOKMARK_CONCURRENCY)
        .collect::<Vec<_>>()
        .await;

    let mut outcome = BookmarkSyncOutcome::default();
    for (bookmark, result) in results {
        match result {
            Ok(made_progress) => outcome.made_progress |= made_progress,
            Err(error) => {
                error!("failed to sync CAS for bookmark {bookmark}: {error:#}");
                outcome.failed_bookmarks += 1;
            }
        }
    }
    Ok(outcome)
}

#[async_trait]
impl LeaderElection for MononokeCasSyncProcessExecutor {
    fn get_shared_lock_path(&self) -> String {
        shared_lock_path(&self.repo_name, self.args.use_case.as_deref())
    }
}

impl MononokeCasSyncProcessExecutor {
    fn new(app: Arc<MononokeApp>, repo_name: String, args: Arc<CommandArgs>) -> Result<Self> {
        let ctx = CoreContext::new_with_client_info(
            app.fb,
            ClientInfo::default_with_entry_point(ClientEntryPoint::MononokeCasSync),
        );

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
            info!("Initiating mononoke RE CAS sync command execution",);

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
                        info!("sync stopping due to cancellation request at attempt {}", attempt);
                    } else {
                        match self.maybe_become_leader(mode).await {
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
                                        self.repo_name, attempt
                                    )
                                })?;
                            },
                            Err(e) => {
                                error!("Failed to become leader {:#}", e);
                            }
                        }
                    }
                    anyhow::Ok(())
                },
                base_retry_delay,
            ).binary_exponential_backoff()
            .max_attempts(
            retry_num)
    .inspect_err(|attempt, _err| info!("attempt {attempt} of {retry_num} failed"))
            .await?;
            info!("Finished mononoke RE CAS sync command execution for repo {}", &self.repo_name,);
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
            "Terminating mononoke RE CAS sync command execution for repo {}",
            &self.repo_name,
        );
        self.cancellation_requested.store(true, Ordering::Relaxed);
        Ok(())
    }
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let process = Arc::new(MononokeCasSyncProcess::new(app, args)?);
    let app_args = &process.app.args::<CasSyncArgs>()?;

    if let Some(executor) = app_args.sharded_executor_args.clone().build_executor(
        process.app.fb,
        process.app.runtime().clone(),
        || process.clone(),
        true, // enable shard (repo) level healing
        SM_CLEANUP_TIMEOUT_SECS,
    )? {
        info!("Running sharded sync loop");
        let (sender, receiver) = tokio::sync::oneshot::channel::<bool>();
        executor.block_and_execute(receiver).await?;
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

async fn run_legacy_sync_mode(
    attempt_num: usize,
    ctx: &CoreContext,
    repo: &Repo,
    re_cas_client: &CasChangesetsUploader<impl CasClient>,
    scuba_sample: &MononokeScubaSampleBuilder,
    main_bookmark: &str,
    sync_all_bookmarks: bool,
    replayed_sync_counter: &LatestReplayedSyncCounter,
    configured_start_id: Option<u64>,
    loop_forever: bool,
    batch_size: u64,
    can_continue: Arc<dyn Fn() -> bool + Send + Sync>,
    repo_name: &str,
) -> Result<(), Error> {
    let start_id = resolve_start_id(ctx, replayed_sync_counter, configured_start_id).await?;
    let continue_legacy: Arc<dyn Fn() -> bool + Send + Sync> = {
        let can_continue = Arc::clone(&can_continue);
        let repo_name = repo_name.to_owned();
        Arc::new(move || can_continue() && !bookmark_polling_enabled(&repo_name))
    };
    let continue_legacy_stream = Arc::clone(&continue_legacy);

    borrowed!(replayed_sync_counter, re_cas_client, repo, scuba_sample);
    loop_over_log_entries_until(
        ctx,
        repo.bookmark_update_log_arc(),
        start_id,
        loop_forever,
        scuba_sample,
        batch_size,
        move || continue_legacy_stream(),
    )
    .try_filter(|entries| future::ready(!entries.is_empty()))
    .fuse()
    .try_next_step(|entries| {
        let continue_legacy = Arc::clone(&continue_legacy);
        async move {
            let entries = entries
                .into_iter()
                .filter(|entry| sync_all_bookmarks || entry.bookmark_name.as_str() == main_bookmark)
                .collect::<Vec<_>>();
            if continue_legacy() && !entries.is_empty() {
                let entries = sync_combined_entries(
                    attempt_num,
                    ctx,
                    repo,
                    re_cas_client,
                    scuba_sample,
                    main_bookmark,
                    entries,
                )
                .watched()
                .await?;
                let next_id = get_id_to_search_after(&entries);
                if replayed_sync_counter
                    .set_counter(ctx, next_id.try_into()?)
                    .watched()
                    .await?
                {
                    Ok(())
                } else {
                    bail!("failed to update counter")
                }
            } else {
                Ok(())
            }
        }
    })
    .try_collect::<()>()
    .await
}

async fn run_bookmark_sync_mode(
    attempt_num: usize,
    ctx: &CoreContext,
    repo: &Repo,
    re_cas_client: &CasChangesetsUploader<impl CasClient>,
    scuba_sample: &MononokeScubaSampleBuilder,
    main_bookmark: &str,
    sync_all_bookmarks: bool,
    replayed_sync_counter: &LatestReplayedSyncCounter,
    configured_start_id: Option<u64>,
    loop_forever: bool,
    batch_size: u64,
    can_continue: Arc<dyn Fn() -> bool + Send + Sync>,
    repo_name: &str,
) -> Result<(), Error> {
    loop {
        if !can_continue() || !bookmark_polling_enabled(repo_name) {
            return Ok(());
        }
        // Keep the legacy cursor frozen while bookmark mode is active. The
        // current per-bookmark counters cannot establish a global frontier:
        // dormant bookmarks do not advance, and bookmark discovery changes as
        // bookmarks are created or deleted. If the JK is disabled, legacy
        // mode resumes from this last globally proven cursor and may replay
        // entries already handled here. CAS uploads and derived-data
        // derivation are idempotent, so that overlap is the safe rollback
        // behavior.
        let legacy_safe_floor =
            resolve_start_id(ctx, replayed_sync_counter, configured_start_id).await?;
        let outcome = match sync_bookmarks_once(
            attempt_num,
            ctx,
            repo,
            re_cas_client,
            scuba_sample,
            main_bookmark,
            sync_all_bookmarks,
            &replayed_sync_counter.counter_name,
            legacy_safe_floor,
            batch_size,
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(error) if loop_forever => {
                error!("failed to discover CAS sync bookmarks: {error:#}");
                tokio::time::sleep(Duration::from_secs(SLEEP_SECS)).await;
                continue;
            }
            Err(error) => return Err(error),
        };

        if outcome.failed_bookmarks > 0 {
            if !loop_forever {
                bail!("{} CAS bookmark workers failed", outcome.failed_bookmarks);
            }
            tokio::time::sleep(Duration::from_secs(SLEEP_SECS)).await;
        } else if !outcome.made_progress {
            if !loop_forever {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_secs(SLEEP_SECS)).await;
        }
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
            format_err!("mononoke_cas_sync_config is not found for the repo {repo_name}")
        })?;

    let use_case = args
        .use_case
        .as_deref()
        .unwrap_or(&sync_config.use_case_public);
    let re_cas_client = CasChangesetsUploader::new(build_mononoke_cas_client(
        fb,
        ctx.clone(),
        &repo_name,
        false,
        use_case,
    )?);

    info!(
        "using repo \"{}\" repoid {:?} and CAS use case \"{}\"",
        repo.repo_identity().name(),
        repo.repo_identity().id(),
        use_case,
    );

    let log_to_scuba = app.args::<CasSyncArgs>()?.log_to_scuba;
    let mut scuba_sample = if log_to_scuba {
        MononokeScubaSampleBuilder::new(ctx.fb, SCUBA_TABLE)?
    } else {
        MononokeScubaSampleBuilder::with_discard()
    };

    scuba_sample.add_common_server_data();
    scuba_sample.add("repo_name", repo_name.clone());
    scuba_sample.add("use_case", use_case);

    let main_bookmark_to_sync = sync_config.main_bookmark_to_sync.as_str();
    let sync_all_bookmarks = sync_config.sync_all_bookmarks;

    // Before beginning any actual processing, check if cancellation has been requested.
    // If yes, then lets return early.
    if cancellation_requested.load(Ordering::Relaxed) {
        info!("sync stopping due to cancellation request");
        return Ok(());
    }

    let loop_forever = args.loop_forever;
    let start_id = args.start_id;
    let exit_path = args.exit_file.clone();
    let batch_size = args.batch_size.unwrap_or(DEFAULT_BATCH_SIZE);
    let counter_name = namespaced_state_key(LATEST_REPLAYED_REQUEST_KEY, args.use_case.as_deref());
    let replayed_sync_counter = LatestReplayedSyncCounter::new(&repo, counter_name.clone())?;

    let can_continue: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
        let exit_file_exists = match exit_path {
            Some(ref exit_path) if exit_path.exists() => {
                info!("path {:?} exists: exiting ...", exit_path);
                true
            }
            _ => false,
        };
        let cancelled = if cancellation_requested.load(Ordering::Relaxed) {
            info!("sync stopping due to cancellation request");
            true
        } else {
            false
        };
        !exit_file_exists && !cancelled
    });

    while can_continue() {
        if bookmark_polling_enabled(&repo_name) {
            run_bookmark_sync_mode(
                attempt_num,
                ctx,
                &repo,
                &re_cas_client,
                &scuba_sample,
                main_bookmark_to_sync,
                sync_all_bookmarks,
                &replayed_sync_counter,
                start_id,
                loop_forever,
                batch_size,
                Arc::clone(&can_continue),
                &repo_name,
            )
            .await?;
        } else {
            run_legacy_sync_mode(
                attempt_num,
                ctx,
                &repo,
                &re_cas_client,
                &scuba_sample,
                main_bookmark_to_sync,
                sync_all_bookmarks,
                &replayed_sync_counter,
                start_id,
                loop_forever,
                batch_size,
                Arc::clone(&can_continue),
                &repo_name,
            )
            .await?;
        }
        if !loop_forever {
            break;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use bookmarks::BookmarkCategory;
    use bookmarks::BookmarkName;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn shared_lock_paths_have_unambiguous_components() {
        let legacy_hyphenated_repo = shared_lock_path("foo-bar", None);
        let namespaced_repo = shared_lock_path("foo", Some("bar"));
        assert_ne!(
            legacy_hyphenated_repo, namespaced_repo,
            "A legacy repo name must not collide with a namespaced lock"
        );

        let hyphenated_use_case = shared_lock_path("foo", Some("bar-baz"));
        let hyphenated_repo = shared_lock_path("foo-bar", Some("baz"));
        assert_ne!(
            hyphenated_use_case, hyphenated_repo,
            "Repo and use-case boundaries must be preserved"
        );
    }

    #[mononoke::test]
    fn shared_lock_path_preserves_legacy_format() {
        assert_eq!(
            shared_lock_path("foo-bar", None),
            "mononoke_cas_sync_job_foo-bar",
            "The production lock path must remain unchanged"
        );
    }

    #[mononoke::test]
    fn bookmark_counter_names_are_readable_and_namespaced() -> Result<()> {
        let branch = BookmarkKey::new("feature/cas-sync")?;
        assert_eq!(
            crate::format_cas_bookmark_counter("latest-replayed-request-cas", &branch)?,
            "latest-replayed-request-cas-by-bookmark-v1-branch-feature/cas-sync"
        );

        let tag = BookmarkKey::with_name_and_category(
            BookmarkName::new("feature/cas-sync")?,
            BookmarkCategory::Tag,
        );
        assert_ne!(
            crate::format_cas_bookmark_counter("latest-replayed-request-cas", &branch)?,
            crate::format_cas_bookmark_counter("latest-replayed-request-cas", &tag)?,
        );
        assert_ne!(
            crate::format_cas_bookmark_counter("latest-replayed-request-cas-use-case-a", &branch,)?,
            crate::format_cas_bookmark_counter("latest-replayed-request-cas-use-case-b", &branch,)?,
        );
        Ok(())
    }
}
