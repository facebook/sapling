/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::commands::SCRUB;
use crate::detail::graph::Node;
use crate::detail::sampling::WalkSampleMapping;
use crate::detail::scrub::scrub_objects;
use crate::detail::scrub::ScrubCommand;
use crate::detail::scrub::ScrubSample;
use anyhow::Context;
use anyhow::Error;
use async_trait::async_trait;
use clap::Parser;
use executor_lib::BackgroundProcessExecutor;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use fbinit::FacebookInit;
use mononoke_app::args::MultiRepoArgs;
use mononoke_app::MononokeApp;
use once_cell::sync::OnceCell;
use slog::info;
use slog::Logger;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::args::OutputFormat;
use crate::args::SamplingArgs;
use crate::args::ScrubOutputNodeArgs;
use crate::args::ScrubPackLogArgs;
use crate::args::WalkerCommonArgs;
use crate::commands::JobParams;
use crate::setup::setup_common;
use crate::WalkerArgs;

const SM_SERVICE_SCOPE: &str = "global";
const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;

/// Checks the data is present by reading it and counting it.
/// Combine with --enable-scrub-blobstore to check across the multiplex.
#[derive(Parser)]
pub struct CommandArgs {
    /// Set the output format
    #[clap(long, short = 'F', default_value = "PrettyDebug")]
    pub output_format: OutputFormat,

    #[clap(flatten)]
    pub output_nodes: ScrubOutputNodeArgs,

    #[clap(flatten)]
    pub pack_log_info: ScrubPackLogArgs,

    #[clap(flatten, next_help_heading = "SAMPLING OPTIONS")]
    pub sampling: SamplingArgs,

    #[clap(flatten)]
    pub common_args: WalkerCommonArgs,
}

/// Struct representing the Walker Scrub BP.
pub struct WalkerScrubProcess {
    app: MononokeApp,
    args: CommandArgs,
}

impl WalkerScrubProcess {
    fn new(app: MononokeApp, args: CommandArgs) -> Self {
        Self { app, args }
    }
}

#[async_trait]
impl RepoShardedProcess for WalkerScrubProcess {
    async fn setup(&self, repo_name: &str) -> anyhow::Result<Arc<dyn RepoShardedProcessExecutor>> {
        let logger = self.app.repo_logger(repo_name);
        info!(&logger, "Setting up walker scrub for repo {}", repo_name);
        let repos = MultiRepoArgs {
            repo_name: vec![repo_name.to_string()],
            repo_id: vec![],
        };
        let (job_params, command) = setup_scrub(&repos, &self.app, &self.args)
            .await
            .with_context(|| {
                format!("Failure in setting up walker scrub for repo {}", &repo_name)
            })?;
        info!(
            &logger,
            "Completed walker scrub setup for repo {}", repo_name
        );
        Ok(Arc::new(WalkerScrubProcessExecutor::new(
            self.app.fb,
            logger,
            job_params,
            command,
            repo_name.to_string(),
        )))
    }
}

/// Struct representing the execution of the Walker Scrub
/// BP over the context of a provided repo.
pub struct WalkerScrubProcessExecutor {
    fb: FacebookInit,
    logger: Logger,
    job_params: JobParams,
    command: ScrubCommand,
    cancellation_requested: Arc<AtomicBool>,
    repo_name: String,
}

impl WalkerScrubProcessExecutor {
    fn new(
        fb: FacebookInit,
        logger: Logger,
        job_params: JobParams,
        command: ScrubCommand,
        repo_name: String,
    ) -> Self {
        Self {
            cancellation_requested: Arc::new(AtomicBool::new(false)),
            fb,
            logger,
            job_params,
            command,
            repo_name,
        }
    }
}

#[async_trait]
impl RepoShardedProcessExecutor for WalkerScrubProcessExecutor {
    async fn execute(&self) -> anyhow::Result<()> {
        info!(
            self.logger,
            "Initiating walker scrub execution for repo {}", &self.repo_name,
        );
        scrub_objects(
            self.fb,
            self.job_params.clone(),
            self.command.clone(),
            Arc::clone(&self.cancellation_requested),
        )
        .await
        .with_context(|| {
            format!(
                "Error while executing walker scrub execution for repo {}",
                &self.repo_name
            )
        })
    }

    async fn stop(&self) -> anyhow::Result<()> {
        info!(
            self.logger,
            "Terminating walker scrub execution for repo {}", &self.repo_name,
        );
        self.cancellation_requested.store(true, Ordering::Relaxed);
        Ok(())
    }
}

async fn setup_scrub(
    repos: &MultiRepoArgs,
    app: &MononokeApp,
    args: &CommandArgs,
) -> Result<(JobParams, ScrubCommand), Error> {
    let component_sampler = Arc::new(WalkSampleMapping::<Node, ScrubSample>::new());
    let repo_name = repos.repo_name.clone().pop();
    let logger = match repo_name {
        Some(repo_name) => app.repo_logger(&repo_name),
        None => app.logger().clone(),
    };
    let job_params = setup_common(
        SCRUB,
        app,
        repos,
        &args.common_args,
        None,
        Some(component_sampler.clone()),
        &logger,
    )
    .await?;

    let CommandArgs {
        output_format,
        output_nodes,
        pack_log_info,
        sampling,
        common_args,
    } = args;
    let command = ScrubCommand {
        limit_data_fetch: common_args.limit_data_fetch,
        output_format: output_format.clone(),
        output_node_types: output_nodes.parse_args(),
        progress_options: common_args.progress.parse_args(),
        sampling_options: sampling.parse_args(1)?,
        pack_info_log_options: pack_log_info.parse_args(app.fb)?,
        sampler: component_sampler,
    };
    Ok((job_params, command))
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<(), Error> {
    let walker_args = &app.args::<WalkerArgs>()?;
    match &walker_args.sharded_service_name {
        Some(service_name) => run_sharded(app, args, service_name.to_string()).await,
        None => run_unsharded(&walker_args.repos, app, args).await,
    }
}

/// The run variant for sharded execution of walker scrub.
pub async fn run_sharded(
    app: MononokeApp,
    args: CommandArgs,
    service_name: String,
) -> Result<(), Error> {
    let scrub_process = WalkerScrubProcess::new(app, args);
    let logger = scrub_process.app.logger().clone();
    // The service name needs to be 'static to satisfy SM contract
    static SM_SERVICE_NAME: OnceCell<String> = OnceCell::new();
    let mut executor = BackgroundProcessExecutor::new(
        scrub_process.app.fb,
        scrub_process.app.runtime().clone(),
        &logger,
        SM_SERVICE_NAME.get_or_init(|| service_name),
        SM_SERVICE_SCOPE,
        SM_CLEANUP_TIMEOUT_SECS,
        Arc::new(scrub_process),
    )?;
    executor.block_and_execute(&logger).await
}

pub async fn run_unsharded(
    repos: &MultiRepoArgs,
    app: MononokeApp,
    args: CommandArgs,
) -> Result<(), Error> {
    let (job_params, command) = setup_scrub(repos, &app, &args).await?;
    // When running in unsharded setting, walker scrub doesn't have a need to
    // be cancelled midway.
    scrub_objects(
        app.fb,
        job_params,
        command,
        Arc::new(AtomicBool::new(false)),
    )
    .await
}
