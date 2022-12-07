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

use anyhow::Error;
use clap::Parser;
use cmdlib_logging::ScribeLoggingArgs;
use context::SessionContainer;
use environment::WarmBookmarksCacheDerivedData;
use fbinit::FacebookInit;
use hostname::get_hostname;
use megarepo_api::MegarepoApi;
use mononoke_app::args::HooksAppExtension;
use mononoke_app::args::RepoFilterAppExtension;
use mononoke_app::args::ShutdownTimeoutArgs;
use mononoke_app::fb303::AliveService;
use mononoke_app::fb303::Fb303AppExtension;
use mononoke_app::MononokeAppBuilder;

const SERVICE_NAME: &str = "megarepo_async_requests_worker";

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
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = MononokeAppBuilder::new(fb)
        .with_warm_bookmarks_cache(WarmBookmarksCacheDerivedData::None)
        .with_app_extension(HooksAppExtension {})
        .with_app_extension(Fb303AppExtension {})
        .with_app_extension(RepoFilterAppExtension {})
        .build::<AsyncRequestsWorkerArgs>()?;
    let args: AsyncRequestsWorkerArgs = app.args()?;
    let request_limit = args.request_limit;
    let jobs_limit = args.jobs;
    let (env, logger, runtime) = (app.environment(), app.logger(), app.runtime());

    let session = SessionContainer::new_with_defaults(env.fb);
    let ctx = session.new_context(logger.clone(), env.scuba_sample_builder.clone());

    let mononoke = Arc::new(
        runtime
            .block_on(app.open_managed_repos())?
            .make_mononoke_api()?,
    );
    let megarepo = Arc::new(runtime.block_on(MegarepoApi::new(&app, mononoke))?);

    let tw_job_cluster = std::env::var("TW_JOB_CLUSTER");
    let tw_job_name = std::env::var("TW_JOB_NAME");
    let tw_task_id = std::env::var("TW_TASK_ID");

    let name = match (tw_job_cluster, tw_job_name, tw_task_id) {
        (Ok(tw_job_cluster), Ok(tw_job_name), Ok(tw_task_id)) => {
            format!("{}/{}/{}", tw_job_cluster, tw_job_name, tw_task_id)
        }
        _ => format!(
            "megarepo_async_requests_worker/{}",
            get_hostname().unwrap_or_else(|_| "unknown_hostname".to_string())
        ),
    };

    let will_exit = Arc::new(AtomicBool::new(false));
    let worker = worker::AsyncMethodRequestWorker::new(megarepo, name);

    app.start_monitoring(SERVICE_NAME, AliveService)?;
    app.start_stats_aggregation()?;

    let run_worker = {
        let will_exit = will_exit.clone();
        move |_app| async move {
            Ok(worker
                .run(&ctx, will_exit, request_limit, jobs_limit)
                .await?)
        }
    };

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
