/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::bail;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use executor_lib::BackgroundProcessExecutor;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use fbinit::FacebookInit;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use once_cell::sync::OnceCell;
use slog::info;
use slog::Logger;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::time;

/// Test application for validating integration behavior with ShardManager.
/// The below can be used with new/existing jobs before migrating them to
/// the sharded execution model.
/// The application can be used to validate:
/// -> Proper repo distribution according to assigned weights.
/// -> Sanity check of core functionality for target BP before onboarding.

/// Struct representing the Background Process that needs to be executed
/// by the Mononoke Sharded Process Manager. This struct can contain state
/// that applies to all repos and need to be one-time initialized. Examples
/// include FacebookInit, MononokeApp, CoreContext, etc.
pub struct TestProcess {
    app: MononokeApp,
}

impl TestProcess {
    fn new(app: MononokeApp) -> Self {
        Self { app }
    }
}

#[async_trait]
impl RepoShardedProcess for TestProcess {
    /// Method responsible for performing the initial setup of the BP in context of
    /// the provided repo. This method should ONLY contain code necessary to build
    /// state (in form of the struct that implements RepoShardedProcessExecutor trait)
    /// that is required to execute the job. The repo-name (or related entity)
    /// should be included as part of the RepoShardedProcessExecutor state.
    async fn setup(&self, repo_name: &str) -> Result<Arc<dyn RepoShardedProcessExecutor>> {
        // Since this is a test application, there is no actual state construction
        // to be performed here. In common cases, this would involve generating the
        // Repo struct or related entity utilizing a factory and then storing the
        // generated entities as part of the returned struct.
        Ok(Arc::new(TestProcessExecutor::new(
            &self.app,
            repo_name.to_string(),
        )))
    }
}

/// Struct representing the execution of the Backgroung Process over
/// a particular repo. This struct can contain state that is specific
/// to an individual repo's execution. It can also include signalling
/// mechanism to indicate that the execution of a particular repo needs
/// to be stopped or paused.
pub struct TestProcessExecutor {
    terminate_execution: Arc<AtomicBool>,
    repo_name: String,
    logger: Logger,
}

impl TestProcessExecutor {
    fn new(app: &MononokeApp, repo_name: String) -> Self {
        Self {
            terminate_execution: Arc::new(AtomicBool::new(false)),
            logger: app.logger().clone(),
            repo_name,
        }
    }
}

/// __NOTE__: This is a free-function and is invoked by both the sharded
/// and non-sharded execution. This model should be utilized when migrating
/// existing jobs with the following objectives:
/// 1. Separation and isolation of sharded execution vs non-sharded execution
/// 2. No changes to functionality (in case the BP has multi-repo logic built
///    into its core, e.g. Walker)
/// 3. Preventing rewrite of the existing repo-parsing logic from CLI args.
/// Note that even for existing jobs, the eventual goal would be to migrate
/// to the below model.

/// For new jobs or existing jobs that do not have the above properties,
/// the below function should not be a free function. It should instead be
/// included as part of the RepoShardedProcessExecutor trait. In case of SM
/// based invocation, the executor will use this method to dynamically provide
/// repos to the BP. In case of non-SM or CLI based invocation, the `run`
/// method will parse the repos from CLI args and then pass them to the
/// `execute()` method within the trait. The entire logic will be housed in
/// the structs that implement these traits and both the BPE and CLI run
/// function will invoke the same methods.

/// Function representing the work that need to be done on the repo
/// as part of this BP.
async fn do_busy_work(
    logger: &Logger,
    repo_name: String,
    terminate_execution: Arc<AtomicBool>,
) -> Result<()> {
    info!(
        logger,
        "Beginning execution of test process for repo {}", repo_name,
    );
    let mut iteration = 1;
    loop {
        info!(
            logger,
            "Executing iteration {} of test process for repo {}", iteration, repo_name,
        );
        // Equivalent to heavy work being done by the process for a repo.
        time::sleep(time::Duration::from_secs(SECS_IN_MINUTE)).await;
        // Completed one iteration of the heavy work. Good time to check if
        // the manager expects us to stop working on this repo.
        if terminate_execution.load(Ordering::Relaxed) {
            // Termination requested. Time to perform clean-up activities
            // before we give up the repo. E.g. pushing out write from memory
            // to actual storage, flushing logs, releasing locks, etc.
            info!(
                logger,
                "Finishing execution of test process for repo {} after {} iterations",
                repo_name,
                iteration,
            );
            // Finally return
            return Ok(());
        } else {
            // The manager doesn't expect us to give up the current repo at the
            // moment. Continue executing future iterations.
            iteration += 1;
        }
        // Fail at every 10th iteration and let the framework reassign this task
        if iteration % 10 == 0 {
            bail!(
                "Sample transient error for repo {} after {} iterations",
                repo_name,
                iteration
            )
        }
    }
}

#[async_trait]
impl RepoShardedProcessExecutor for TestProcessExecutor {
    /// Core logic for executing the BPE + logic to check some form of
    /// signal from the corresponding stop() method. The stop() method
    /// will only give a heads-up, the actual cleaning and state-save
    /// activities need to be performed as part of the execute() call.
    /// Post the completion of stop() callback, the execute() method
    /// has timeout secs to finish its book-keeping activities.
    async fn execute(&self) -> Result<()> {
        do_busy_work(
            &self.logger,
            self.repo_name.to_string(),
            Arc::clone(&self.terminate_execution),
        )
        .await
    }

    /// Note that this method is responsible ONLY for signalling the termination
    /// of the execution of the repo for the given BP. Once the control returns
    /// from this method, the executor waits for timeout seconds (specified during
    /// executor construction) before initiating forced termination of the executing
    /// process. If there are any book-keeping activities pending, they should be
    /// completed in the main execution method during this timeout period.
    async fn stop(&self) -> Result<()> {
        // Signal the BP to stop executing this repo.
        self.terminate_execution.store(true, Ordering::Relaxed);
        Ok(())
    }
}

/// Arguments for a sample command line app.
#[derive(Parser)]
struct TestArgs {
    /// The name of ShardManager service to be used when the walker
    /// functionality is desired to be executed in a sharded setting.
    #[clap(long)]
    pub sharded_service_name: Option<String>,
    /// The repo for which the job needs to be executed when sharded
    /// execution is not desired.
    #[clap(long, conflicts_with = "sharded-service-name")]
    pub repo_name: Option<String>,
}

/// Keep it global until regional deployment is desired.
const SM_SERVICE_SCOPE: &str = "global";
/// Adjust the value based on the time taken to perform
/// cleanup of a BP execution instance over a repo. Max is
/// 180 seconds. Ideally, should be under 60 seconds.
const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;
/// Constant representing seconds in a minute.
const SECS_IN_MINUTE: u64 = 60;

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let app = MononokeAppBuilder::new(fb).build::<TestArgs>()?;
    app.run(run)
}

async fn run(app: MononokeApp) -> Result<()> {
    // The below flag can be used to determine if the current BP execution
    // should occur in sharded or non-sharded manner. The flag can be read
    // from static config, or DB table or based on CLI value specified along
    // with the actual command. If the sharded-service-name is non-empty, it
    // indicates that the job needs to run in sharded mode.
    match &app.args::<TestArgs>()?.sharded_service_name {
        // If sharded execution is not desired, the repo passed to on-repo-
        // load shoud be derived through CLI args or some other source.
        None => run_unsharded(app).await,
        Some(service_name) => run_sharded(app, service_name.to_string()).await,
    }
}

async fn run_sharded(app: MononokeApp, sharded_service_name: String) -> Result<()> {
    let process = TestProcess::new(app);
    let logger = process.app.logger().clone();
    /// The name of the deployed ShardManager job that will orchestrate
    /// the below BP. For testing purposes, keep it mononoke.shardmanager.test
    /// and deploy the resultant binary to mononoke_shardmanager_test TW job.
    /// mononoke.shardmanager.test currently works with 28 repos and 7 task
    /// replicas.
    // The service name needs to be 'static to satisfy SM contract
    static SM_SERVICE_NAME: OnceCell<String> = OnceCell::new();
    // For sharded execution, we need to first create the executor.
    let mut executor = BackgroundProcessExecutor::new(
        process.app.fb,
        process.app.runtime().clone(),
        &logger,
        SM_SERVICE_NAME.get_or_init(|| sharded_service_name),
        SM_SERVICE_SCOPE,
        SM_CLEANUP_TIMEOUT_SECS,
        Arc::new(process),
    )?;
    executor.block_and_execute(&logger).await
}

async fn run_unsharded(app: MononokeApp) -> Result<()> {
    let repo_name = app
        .args::<TestArgs>()?
        .repo_name
        .expect("Repo name needs to be provided when executing in unsharded mode");
    // Terminate execution can still be used to halt execution even in unsharded mode.
    // For this example, we are immediately terminating after one loop.
    let terminate_execution = Arc::new(AtomicBool::new(true));
    do_busy_work(app.logger(), repo_name, Arc::clone(&terminate_execution)).await
}
