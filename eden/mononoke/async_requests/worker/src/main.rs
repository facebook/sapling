/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(async_closure)]

mod methods;
mod worker;

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use async_requests::AsyncMethodRequestQueue;
use async_requests_client::open_blobstore;
use async_requests_client::open_sql_connection;
use async_trait::async_trait;
use blobstore::Blobstore;
use clap::Parser;
use cloned::cloned;
use cmdlib_logging::ScribeLoggingArgs;
use context::CoreContext;
use context::SessionContainer;
use environment::BookmarkCacheDerivedData;
use environment::BookmarkCacheKind;
use environment::BookmarkCacheOptions;
use executor_lib::args::ShardedExecutorArgs;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use fbinit::FacebookInit;
use megarepo_api::MegarepoApi;
use metaconfig_types::ShardedService;
use mononoke_api::Mononoke;
use mononoke_api::Repo;
use mononoke_api::RepositoryId;
use mononoke_app::args::HooksAppExtension;
use mononoke_app::args::RepoFilterAppExtension;
use mononoke_app::args::ShutdownTimeoutArgs;
use mononoke_app::args::WarmBookmarksCacheExtension;
use mononoke_app::fb303::AliveService;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeAppBuilder;
use mononoke_app::MononokeReposManager;
use requests_table::LongRunningRequestsQueue;
use requests_table::SqlLongRunningRequestsQueue;
use sharding_ext::RepoShard;
use slog::info;

const SERVICE_NAME: &str = "async_requests_worker";

const SM_CLEANUP_TIMEOUT_SECS: u64 = 60;

/// Processes the megarepo async requests
#[derive(Parser)]
struct AsyncRequestsWorkerArgs {
    #[clap(flatten)]
    pub sharded_executor_args: ShardedExecutorArgs,

    #[clap(flatten)]
    shutdown_timeout_args: ShutdownTimeoutArgs,
    #[clap(flatten)]
    scribe_logging_args: ScribeLoggingArgs,
    /// The number of requests to process before exiting
    #[clap(long)]
    request_limit: Option<usize>,
    /// The number of requests / jobs to be processed concurrently
    #[clap(long, short = 'j', default_value = "1")]
    jobs: usize,
    /// If true, the worker will process requests for the global queue.
    #[clap(long)]
    process_global_queue: bool,
}

pub struct WorkerProcess {
    ctx: Arc<CoreContext>,
    args: Arc<AsyncRequestsWorkerArgs>,
    repos_mgr: Arc<MononokeReposManager<Repo>>,
    mononoke: Arc<Mononoke<Repo>>,
    megarepo: Arc<MegarepoApi<Repo>>,
    sql_connection: Arc<SqlLongRunningRequestsQueue>,
    blobstore: Arc<dyn blobstore::Blobstore>,
    will_exit: Arc<AtomicBool>,
}

impl WorkerProcess {
    pub(crate) fn new(
        ctx: Arc<CoreContext>,
        args: Arc<AsyncRequestsWorkerArgs>,
        repos_mgr: Arc<MononokeReposManager<Repo>>,
        mononoke: Arc<Mononoke<Repo>>,
        megarepo: Arc<MegarepoApi<Repo>>,
        sql_connection: Arc<SqlLongRunningRequestsQueue>,
        blobstore: Arc<dyn blobstore::Blobstore>,
        will_exit: Arc<AtomicBool>,
    ) -> Self {
        Self {
            ctx,
            args,
            repos_mgr,
            mononoke,
            megarepo,
            sql_connection,
            blobstore,
            will_exit,
        }
    }
}

#[async_trait]
impl RepoShardedProcess for WorkerProcess {
    async fn setup(&self, repo: &RepoShard) -> Result<Arc<dyn RepoShardedProcessExecutor>> {
        let repo_name = repo.repo_name.as_str();
        let logger = self.repos_mgr.repo_logger(repo_name);
        info!(&logger, "Setting up repo {}", repo_name);

        let repo = self
            .repos_mgr
            .add_repo(repo_name)
            .await
            .with_context(|| format!("Failure in setting up repo {}", repo_name))?;
        let repos = vec![repo.repo_identity.id()];
        info!(&logger, "Completed setup for repos {:?}", repos);

        let queue = Arc::new(AsyncMethodRequestQueue::new(
            self.sql_connection.clone(),
            self.blobstore.clone(),
            Some(repos),
        ));

        let executor = worker::AsyncMethodRequestWorker::new(
            self.args.clone(),
            self.ctx.clone(),
            queue,
            self.mononoke.clone(),
            self.megarepo.clone(),
            self.will_exit.clone(),
        )
        .await?;
        Ok(Arc::new(executor))
    }
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = MononokeAppBuilder::new(fb)
        .with_bookmarks_cache(BookmarkCacheOptions {
            cache_kind: BookmarkCacheKind::Local,
            derived_data: BookmarkCacheDerivedData::NoDerivation,
        })
        .with_app_extension(WarmBookmarksCacheExtension {})
        .with_app_extension(HooksAppExtension {})
        .with_app_extension(Fb303AppExtension {})
        .with_app_extension(RepoFilterAppExtension {})
        .build::<AsyncRequestsWorkerArgs>()?;

    let args: Arc<AsyncRequestsWorkerArgs> = Arc::new(app.args()?);
    let env = app.environment();
    let logger = app.logger().clone();
    let runtime = app.runtime().clone();
    let session = SessionContainer::new_with_defaults(env.fb);
    let ctx = Arc::new(session.new_context(app.logger().clone(), env.scuba_sample_builder.clone()));

    let sharded_args = args.sharded_executor_args.clone();
    let repos_mgr = if sharded_args.is_sharded() {
        let service_name = Some(ShardedService::AsyncRequestsWorker);
        runtime.block_on(app.open_managed_repos(service_name))
    } else {
        runtime.block_on(app.open_managed_repos(None))
    }?;
    let repos_mgr = Arc::new(repos_mgr);
    let mononoke = Arc::new(repos_mgr.make_mononoke_api()?);
    let megarepo = Arc::new(MegarepoApi::new(&app, mononoke.clone())?);

    let sql_connection = Arc::new(runtime.block_on(open_sql_connection(fb, &app))?);
    let blobstore = runtime.block_on(open_blobstore(fb, &app))?;
    let will_exit = Arc::new(AtomicBool::new(false));

    app.start_monitoring(SERVICE_NAME, AliveService)?;
    app.start_stats_aggregation()?;

    if let Some(mut executor) = args.sharded_executor_args.clone().build_executor(
        app.fb,
        runtime.clone(),
        &logger,
        || {
            Arc::new(WorkerProcess::new(
                ctx.clone(),
                args.clone(),
                repos_mgr.clone(),
                mononoke.clone(),
                megarepo.clone(),
                sql_connection.clone(),
                blobstore.clone(),
                will_exit.clone(),
            ))
        },
        true, // enable shard (repo) level healing
        SM_CLEANUP_TIMEOUT_SECS,
    )? {
        info!(logger, "Starting sharded process");
        // The Sharded Process Executor needs to branch off and execute
        // on its own dedicated task spawned off the common tokio runtime.
        runtime.spawn({
            let logger = logger.clone();
            {
                cloned!(will_exit);
                async move { executor.block_and_execute(&logger, will_exit).await }
            }
        });

        if args.process_global_queue {
            info!(logger, "Starting executor for global queue");
            run_worker_queue(
                &runtime,
                ctx.clone(),
                args.clone(),
                mononoke.clone(),
                megarepo.clone(),
                sql_connection.clone(),
                blobstore.clone(),
                None,
                will_exit.clone(),
            )?;
        }

        app.wait_until_terminated(
            move || will_exit.store(true, Ordering::Relaxed),
            args.shutdown_timeout_args.shutdown_grace_period,
            async {
                info!(logger, "Shutdown");
            },
            args.shutdown_timeout_args.shutdown_timeout,
        )?;
    } else {
        let logger = logger.clone();

        // Sanity check to avoid a weird nonsensical state. This triggered S460221, so let's be paranoid.
        let repos = mononoke.known_repo_ids();
        if repos.is_empty() {
            panic!("There are no repos configured for this service, cannot continue");
        }

        // all enabled repos
        info!(
            logger,
            "Starting unsharded executor for repos {:?}",
            repos.clone()
        );
        run_worker_queue(
            &runtime,
            ctx.clone(),
            args.clone(),
            mononoke.clone(),
            megarepo.clone(),
            sql_connection.clone(),
            blobstore.clone(),
            Some(repos.clone()),
            will_exit.clone(),
        )?;

        // global queue
        if args.process_global_queue {
            info!(logger, "Starting unsharded executor for global queue");
            run_worker_queue(
                &runtime,
                ctx.clone(),
                args.clone(),
                mononoke.clone(),
                megarepo.clone(),
                sql_connection.clone(),
                blobstore.clone(),
                None,
                will_exit.clone(),
            )?;
        }

        app.wait_until_terminated(
            move || will_exit.store(true, Ordering::Relaxed),
            args.shutdown_timeout_args.shutdown_grace_period,
            async {
                info!(logger, "Shutdown");
            },
            args.shutdown_timeout_args.shutdown_timeout,
        )?;
    }

    Ok(())
}

fn run_worker_queue(
    runtime: &tokio::runtime::Handle,
    ctx: Arc<CoreContext>,
    args: Arc<AsyncRequestsWorkerArgs>,
    mononoke: Arc<Mononoke<Repo>>,
    megarepo: Arc<MegarepoApi<Repo>>,
    sql_connection: Arc<dyn LongRunningRequestsQueue>,
    blobstore: Arc<dyn Blobstore>,
    repos: Option<Vec<RepositoryId>>,
    will_exit: Arc<AtomicBool>,
) -> Result<()> {
    let executor = {
        let queue = Arc::new(AsyncMethodRequestQueue::new(
            sql_connection,
            blobstore,
            repos,
        ));

        runtime.block_on(worker::AsyncMethodRequestWorker::new(
            args.clone(),
            ctx.clone(),
            queue.clone(),
            mononoke.clone(),
            megarepo.clone(),
            will_exit.clone(),
        ))?
    };
    runtime.spawn(async move { executor.execute().await });
    Ok(())
}
