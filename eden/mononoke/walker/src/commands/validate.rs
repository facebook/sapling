/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::commands::VALIDATE;
use crate::detail::validate::validate;
use crate::detail::validate::ValidateCommand;
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

use crate::args::ValidateCheckTypeArgs;
use crate::args::WalkerCommonArgs;
use crate::commands::JobParams;
use crate::setup::setup_common;
use crate::WalkerArgs;

const SM_SERVICE_SCOPE: &str = "global";
const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;

/// Walk the graph and perform checks on it.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten)]
    pub check_types: ValidateCheckTypeArgs,

    #[clap(flatten)]
    pub common_args: WalkerCommonArgs,
}

/// Struct representing the Walker Validate BP.
pub struct WalkerValidateProcess {
    app: MononokeApp,
    args: CommandArgs,
}

impl WalkerValidateProcess {
    fn new(app: MononokeApp, args: CommandArgs) -> Self {
        Self { app, args }
    }
}

#[async_trait]
impl RepoShardedProcess for WalkerValidateProcess {
    async fn setup(&self, repo_name: &str) -> anyhow::Result<Arc<dyn RepoShardedProcessExecutor>> {
        let logger = self.app.repo_logger(repo_name);
        info!(&logger, "Setting up walker validate for repo {}", repo_name);
        let repos = MultiRepoArgs {
            repo_name: vec![repo_name.to_string()],
            repo_id: vec![],
        };
        let (job_params, command) = setup_validate(&repos, &self.app, &self.args)
            .await
            .with_context(|| {
                format!(
                    "Failure in setting up walker validate for repo {}",
                    &repo_name
                )
            })?;
        info!(
            &logger,
            "Completed walker validate setup for repo {}", repo_name
        );
        Ok(Arc::new(WalkerValidateProcessExecutor::new(
            self.app.fb,
            logger,
            job_params,
            command,
            repo_name.to_string(),
        )))
    }
}

/// Struct representing the execution of Walker Validate
/// BP over the context of a provided repo.
pub struct WalkerValidateProcessExecutor {
    fb: FacebookInit,
    logger: Logger,
    job_params: JobParams,
    command: ValidateCommand,
    cancellation_requested: Arc<AtomicBool>,
    repo_name: String,
}

impl WalkerValidateProcessExecutor {
    fn new(
        fb: FacebookInit,
        logger: Logger,
        job_params: JobParams,
        command: ValidateCommand,
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
impl RepoShardedProcessExecutor for WalkerValidateProcessExecutor {
    async fn execute(&self) -> anyhow::Result<()> {
        info!(
            self.logger,
            "Initiating walker validate execution for repo {}", &self.repo_name,
        );
        validate(
            self.fb,
            self.job_params.clone(),
            self.command.clone(),
            Arc::clone(&self.cancellation_requested),
        )
        .await
        .with_context(|| {
            format!(
                "Error while executing walker validate execution for repo {}",
                &self.repo_name
            )
        })
    }

    async fn stop(&self) -> anyhow::Result<()> {
        info!(
            self.logger,
            "Terminating walker validate execution for repo {}", &self.repo_name,
        );
        self.cancellation_requested.store(true, Ordering::Relaxed);
        Ok(())
    }
}

async fn setup_validate(
    repos: &MultiRepoArgs,
    app: &MononokeApp,
    args: &CommandArgs,
) -> Result<(JobParams, ValidateCommand), Error> {
    let CommandArgs {
        check_types,
        common_args,
    } = args;
    let repo_name = repos.repo_name.clone().pop();
    let logger = match repo_name {
        Some(repo_name) => app.repo_logger(&repo_name),
        None => app.logger().clone(),
    };
    let job_params = setup_common(
        VALIDATE,
        app,
        repos,
        common_args,
        None, // blobstore sampler
        None, // blobstore component sampler
        &logger,
    )
    .await?;

    let command = ValidateCommand {
        include_check_types: check_types.parse_args(),
        progress_options: common_args.progress.parse_args(),
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

/// The run variant for sharded execution of walker validatet.
pub async fn run_sharded(
    app: MononokeApp,
    args: CommandArgs,
    service_name: String,
) -> Result<(), Error> {
    let validate_process = WalkerValidateProcess::new(app, args);
    let logger = validate_process.app.logger().clone();
    // The service name needs to be 'static to satisfy SM contract
    static SM_SERVICE_NAME: OnceCell<String> = OnceCell::new();
    let mut executor = BackgroundProcessExecutor::new(
        validate_process.app.fb,
        validate_process.app.runtime().clone(),
        &logger,
        SM_SERVICE_NAME.get_or_init(|| service_name),
        SM_SERVICE_SCOPE,
        SM_CLEANUP_TIMEOUT_SECS,
        Arc::new(validate_process),
    )?;
    executor.block_and_execute(&logger).await
}

pub async fn run_unsharded(
    repos: &MultiRepoArgs,
    app: MononokeApp,
    args: CommandArgs,
) -> Result<(), Error> {
    let (job_params, command) = setup_validate(repos, &app, &args).await?;
    // When running in unsharded setting, walker validate doesn't need to
    // be cancelled midway.
    validate(
        app.fb,
        job_params,
        command,
        Arc::new(AtomicBool::new(false)),
    )
    .await
}
