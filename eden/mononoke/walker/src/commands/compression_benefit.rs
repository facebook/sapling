/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

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

use crate::commands::COMPRESSION_BENEFIT;
use crate::detail::graph::Node;
use crate::detail::sampling::WalkSampleMapping;
use crate::detail::sizing::compression_benefit;
use crate::detail::sizing::SizingCommand;
use crate::detail::sizing::SizingSample;

use crate::args::SamplingArgs;
use crate::args::WalkerCommonArgs;
use crate::commands::JobParams;
use crate::setup::setup_common;
use crate::WalkerArgs;

const SM_SERVICE_SCOPE: &str = "global";
const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;

/// Estimate compression benefit.
#[derive(Parser)]
pub struct CommandArgs {
    /// Zstd compression level to use.
    #[clap(long, default_value = "3")]
    pub compression_level: i32,

    #[clap(flatten, next_help_heading = "SAMPLING OPTIONS")]
    pub sampling: SamplingArgs,

    #[clap(flatten)]
    pub common_args: WalkerCommonArgs,
}

/// Struct representing the Walker Sizing BP.
pub struct WalkerSizingProcess {
    app: MononokeApp,
    args: CommandArgs,
}

impl WalkerSizingProcess {
    fn new(app: MononokeApp, args: CommandArgs) -> Self {
        Self { app, args }
    }
}

#[async_trait]
impl RepoShardedProcess for WalkerSizingProcess {
    async fn setup(&self, repo_name: &str) -> anyhow::Result<Arc<dyn RepoShardedProcessExecutor>> {
        let logger = self.app.repo_logger(repo_name);
        info!(&logger, "Setting up walker sizing for repo {}", repo_name);
        let repos = MultiRepoArgs {
            repo_name: vec![repo_name.to_string()],
            repo_id: vec![],
        };
        let (job_params, command) = setup_sizing(&repos, &self.app, &self.args)
            .await
            .with_context(|| {
                format!(
                    "Failure in setting up walker sizing for repo {}",
                    &repo_name
                )
            })?;
        info!(
            &logger,
            "Completed walker sizing setup for repo {}", repo_name
        );
        Ok(Arc::new(WalkerSizingProcessExecutor::new(
            self.app.fb,
            logger,
            job_params,
            command,
            repo_name.to_string(),
        )))
    }
}

/// Struct representing the execution of Walker Sizing
/// BP over the context of a provided repo.
pub struct WalkerSizingProcessExecutor {
    fb: FacebookInit,
    logger: Logger,
    job_params: JobParams,
    command: SizingCommand,
    cancellation_requested: Arc<AtomicBool>,
    repo_name: String,
}

impl WalkerSizingProcessExecutor {
    fn new(
        fb: FacebookInit,
        logger: Logger,
        job_params: JobParams,
        command: SizingCommand,
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
impl RepoShardedProcessExecutor for WalkerSizingProcessExecutor {
    async fn execute(&self) -> anyhow::Result<()> {
        info!(
            self.logger,
            "Initiating walker sizing execution for repo {}", &self.repo_name,
        );
        compression_benefit(
            self.fb,
            self.job_params.clone(),
            self.command.clone(),
            Arc::clone(&self.cancellation_requested),
        )
        .await
        .with_context(|| {
            format!(
                "Error while executing walker sizing for repo {}",
                &self.repo_name
            )
        })
    }

    async fn stop(&self) -> anyhow::Result<()> {
        info!(
            self.logger,
            "Terminating walker sizing execution for repo {}", &self.repo_name,
        );
        self.cancellation_requested.store(true, Ordering::Relaxed);
        Ok(())
    }
}

async fn setup_sizing(
    repos: &MultiRepoArgs,
    app: &MononokeApp,
    args: &CommandArgs,
) -> Result<(JobParams, SizingCommand), Error> {
    let CommandArgs {
        compression_level,
        sampling,
        common_args,
    } = args;

    let sampler = Arc::new(WalkSampleMapping::<Node, SizingSample>::new());
    let repo_name = repos.repo_name.clone().pop();
    let logger = match repo_name {
        Some(repo_name) => app.repo_logger(&repo_name),
        None => app.logger().clone(),
    };
    let job_params = setup_common(
        COMPRESSION_BENEFIT,
        app,
        repos,
        common_args,
        Some(sampler.clone()), // blobstore sampler
        None,                  // blobstore component sampler
        &logger,
    )
    .await?;

    let command = SizingCommand {
        compression_level: *compression_level,
        progress_options: common_args.progress.parse_args(),
        sampling_options: sampling.parse_args(100 /* default_sample_rate */)?,
        sampler,
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

/// The run variant for sharded execution of walker sizing.
pub async fn run_sharded(
    app: MononokeApp,
    args: CommandArgs,
    service_name: String,
) -> Result<(), Error> {
    let sizing_process = WalkerSizingProcess::new(app, args);
    let logger = sizing_process.app.logger().clone();
    // The service name needs to be 'static to satisfy SM contract
    static SM_SERVICE_NAME: OnceCell<String> = OnceCell::new();
    let mut executor = BackgroundProcessExecutor::new(
        sizing_process.app.fb,
        sizing_process.app.runtime().clone(),
        &logger,
        SM_SERVICE_NAME.get_or_init(|| service_name),
        SM_SERVICE_SCOPE,
        SM_CLEANUP_TIMEOUT_SECS,
        Arc::new(sizing_process),
    )?;
    executor.block_and_execute(&logger).await
}

pub async fn run_unsharded(
    repos: &MultiRepoArgs,
    app: MononokeApp,
    args: CommandArgs,
) -> Result<(), Error> {
    let (job_params, command) = setup_sizing(repos, &app, &args).await?;
    // When running in unsharded setting, walker sizing doesn't need to
    // be cancelled midway.
    compression_benefit(
        app.fb,
        job_params,
        command,
        Arc::new(AtomicBool::new(false)),
    )
    .await
}
