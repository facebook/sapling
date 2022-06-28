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

use fbinit::FacebookInit;

use anyhow::Error;
use clap::value_t;
use clap::Arg;
use cmdlib::args;
use cmdlib::helpers::serve_forever;
use cmdlib::monitoring::start_fb303_server;
use cmdlib::monitoring::AliveService;
use context::SessionContainer;
use hostname::get_hostname;
use megarepo_api::MegarepoApi;
use mononoke_api::Mononoke;
use mononoke_api::MononokeApiEnvironment;
use mononoke_api::WarmBookmarksCacheDerivedData;
use repo_factory::RepoFactory;
use scuba_ext::MononokeScubaSampleBuilder;

const ARG_REQUEST_LIMIT: &str = "request-limit";
const ARG_CONCURRENT_JOBS_LIMIT: &str = "jobs-limit";
const SERVICE_NAME: &str = "megarepo_async_requests_worker";

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<(), Error> {
    let app = args::MononokeAppBuilder::new("Processes the megarepo async requests.")
        .with_advanced_args_hidden()
        .with_all_repos()
        .with_shutdown_timeout_args()
        .with_scuba_logging_args()
        .with_scribe_args()
        .with_fb303_args()
        .build()
        .arg(
            Arg::with_name(ARG_REQUEST_LIMIT)
                .long("request-limit")
                .value_name("LIMIT")
                .help("Process LIMIT requests and exit."),
        )
        .arg(
            Arg::with_name(ARG_CONCURRENT_JOBS_LIMIT)
                .short("j")
                .long("jobs")
                .value_name("JOBS")
                .default_value("1")
                .help("Process at most JOBS requests concurrently."),
        );

    let matches = app.get_matches(fb)?;

    let request_limit = matches
        .value_of(ARG_REQUEST_LIMIT)
        .map(|_limit| value_t!(matches, ARG_REQUEST_LIMIT, usize).unwrap_or_else(|e| e.exit()));
    let jobs_limit = value_t!(matches, ARG_CONCURRENT_JOBS_LIMIT, usize)?;
    let runtime = matches.runtime();
    let logger = matches.logger();

    let session = SessionContainer::new_with_defaults(fb);
    let ctx = session.new_context(logger.clone(), matches.scuba_sample_builder());

    let config_store = matches.config_store();
    let repo_configs = args::load_repo_configs(&config_store, &matches)?;
    let repo_factory = RepoFactory::new(matches.environment().clone(), &repo_configs.common);
    let env = MononokeApiEnvironment {
        repo_factory: repo_factory.clone(),
        warm_bookmarks_cache_derived_data: WarmBookmarksCacheDerivedData::None,
        warm_bookmarks_cache_enabled: true,
        warm_bookmarks_cache_scuba_sample_builder: MononokeScubaSampleBuilder::with_discard(),
        skiplist_enabled: true,
    };
    let mononoke = Arc::new(runtime.block_on(Mononoke::new(&env, repo_configs.clone()))?);
    let megarepo = Arc::new(runtime.block_on(MegarepoApi::new(
        matches.environment(),
        repo_configs,
        repo_factory,
        mononoke,
    ))?);

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

    start_fb303_server(fb, SERVICE_NAME, &logger, &matches, AliveService)?;
    serve_forever(
        runtime,
        {
            let will_exit = will_exit.clone();
            async move || {
                Ok(worker
                    .run(&ctx, will_exit.clone(), request_limit, jobs_limit)
                    .await?)
            }
        }(),
        &logger,
        move || will_exit.store(true, Ordering::Relaxed),
        args::get_shutdown_grace_period(&matches)?,
        async {
            // the code to gracefully stop things goes here
        },
        args::get_shutdown_timeout(&matches)?,
    )?;

    Ok(())
}
