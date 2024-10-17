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

use anyhow::Result;
use clap::Parser;
use cmdlib_logging::ScribeLoggingArgs;
use context::SessionContainer;
use environment::BookmarkCacheDerivedData;
use environment::BookmarkCacheKind;
use environment::BookmarkCacheOptions;
use executor_lib::RepoShardedProcessExecutor;
use fbinit::FacebookInit;
use megarepo_api::MegarepoApi;
use metaconfig_types::ShardedService;
use mononoke_api::Repo;
use mononoke_app::args::HooksAppExtension;
use mononoke_app::args::RepoFilterAppExtension;
use mononoke_app::args::ShutdownTimeoutArgs;
use mononoke_app::args::WarmBookmarksCacheExtension;
use mononoke_app::fb303::AliveService;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeAppBuilder;

const SERVICE_NAME: &str = "async_requests_worker";

/// Processes the megarepo async requests
#[derive(Parser)]
struct AsyncRequestsWorkerArgs {
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
    /// If true, the worker will process requests for any repo. If false (the default), it will only process requests
    /// for some repos, inferred from the loaded config, possibly filtered.
    #[clap(long)]
    process_all_repos: bool,
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
    let runtime = app.runtime().clone();
    let session = SessionContainer::new_with_defaults(env.fb);
    let ctx = session.new_context(app.logger().clone(), env.scuba_sample_builder.clone());

    let mononoke = Arc::new(
        runtime
            .block_on(app.open_managed_repos::<Repo>(Some(ShardedService::AsyncRequestsWorker)))?
            .make_mononoke_api()?,
    );
    let repos = mononoke.known_repo_ids();
    let megarepo = Arc::new(MegarepoApi::new(&app, mononoke.clone())?);

    let will_exit = Arc::new(AtomicBool::new(false));
    let filter_repos = if args.process_all_repos {
        None
    } else {
        Some(repos)
    };
    let queue = Arc::new(runtime.block_on(async_requests_client::build(fb, &app, filter_repos))?);
    let worker = runtime.block_on(worker::AsyncMethodRequestWorker::new(
        args.clone(),
        Arc::new(ctx),
        queue,
        mononoke,
        megarepo,
        will_exit.clone(),
    ))?;

    app.start_monitoring(SERVICE_NAME, AliveService)?;
    app.start_stats_aggregation()?;

    let run_worker = { move |_app| async move { worker.execute().await } };

    app.run_until_terminated(
        run_worker,
        move || will_exit.store(true, Ordering::Relaxed),
        args.shutdown_timeout_args.shutdown_grace_period,
        async {
            // the code to gracefully stop things goes here
        },
        args.shutdown_timeout_args.shutdown_timeout,
    )?;

    Ok(())
}
