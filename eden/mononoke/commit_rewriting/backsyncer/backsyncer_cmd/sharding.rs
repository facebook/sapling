/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Context;
use async_trait::async_trait;
use clap::Arg;
use clap::SubCommand;
use cmdlib::args;
use cmdlib::args::MononokeMatches;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use fbinit::FacebookInit;
use sharding_ext::RepoShard;
use slog::error;
use slog::info;
use tokio::runtime::Runtime;

use crate::run::run_backsyncer;

pub(crate) const DEFAULT_SHARDED_SCOPE_NAME: &str = "global";
pub(crate) const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;
pub(crate) const APP_NAME: &str = "backsyncer cmd-line tool";

pub(crate) const ARG_MODE_BACKSYNC_FOREVER: &str = "backsync-forever";
pub(crate) const ARG_MODE_BACKSYNC_ALL: &str = "backsync-all";
pub(crate) const ARG_MODE_BACKSYNC_COMMITS: &str = "backsync-commits";
pub(crate) const ARG_BATCH_SIZE: &str = "batch-size";
pub(crate) const ARG_INPUT_FILE: &str = "INPUT_FILE";

pub(crate) const SCUBA_TABLE: &str = "mononoke_xrepo_backsync";

/// Struct representing the Back Syncer BP.
pub struct BacksyncProcess {
    pub(crate) matches: Arc<MononokeMatches<'static>>,
    pub(crate) fb: FacebookInit,
    _runtime: Runtime,
}

impl BacksyncProcess {
    pub(crate) fn new(fb: FacebookInit) -> anyhow::Result<Self> {
        let app = args::MononokeAppBuilder::new(APP_NAME)
            .with_fb303_args()
            .with_source_and_target_repos()
            .with_dynamic_repos()
            .with_scribe_args()
            .with_default_scuba_dataset(SCUBA_TABLE)
            .with_scuba_logging_args()
            .build();
        let backsync_forever_subcommand = SubCommand::with_name(ARG_MODE_BACKSYNC_FOREVER)
            .about("Backsyncs all new bookmark moves");

        let sync_loop = SubCommand::with_name(ARG_MODE_BACKSYNC_COMMITS)
            .about("Syncs all commits from the file")
            .arg(
                Arg::with_name(ARG_INPUT_FILE)
                    .takes_value(true)
                    .required(true)
                    .help("list of hg commits to backsync"),
            )
            .arg(
                Arg::with_name(ARG_BATCH_SIZE)
                    .long(ARG_BATCH_SIZE)
                    .takes_value(true)
                    .required(false)
                    .help("how many commits to backsync at once"),
            );

        let backsync_all_subcommand = SubCommand::with_name(ARG_MODE_BACKSYNC_ALL)
            .about("Backsyncs all new bookmark moves once");
        let app = app
            .subcommand(backsync_all_subcommand)
            .subcommand(backsync_forever_subcommand)
            .subcommand(sync_loop);
        let (matches, _runtime) = app.get_matches(fb)?;
        let matches = Arc::new(matches);
        Ok(Self {
            matches,
            fb,
            _runtime,
        })
    }
}

#[async_trait]
impl RepoShardedProcess for BacksyncProcess {
    async fn setup(&self, repo: &RepoShard) -> anyhow::Result<Arc<dyn RepoShardedProcessExecutor>> {
        let logger = self.matches.logger();
        // For backsyncer, two repos (i.e. source and target) are required as input
        let source_repo_name = repo.repo_name.clone();
        let target_repo_name = match repo.target_repo_name.clone() {
            Some(repo_name) => repo_name,
            None => {
                let details = format!(
                    "Only source repo name {} provided, target repo name missing in {}",
                    source_repo_name, repo
                );
                error!(logger, "{}", details);
                bail!("{}", details)
            }
        };
        info!(
            logger,
            "Setting up back syncer command from repo {} to repo {}",
            source_repo_name,
            target_repo_name,
        );
        let details = format!(
            "Completed back syncer command setup from repo {} to repo {}",
            source_repo_name, target_repo_name
        );
        let executor = BacksyncProcessExecutor::new(
            self.fb,
            Arc::clone(&self.matches),
            source_repo_name,
            target_repo_name,
        );
        info!(logger, "{}", details);
        Ok(Arc::new(executor))
    }
}

/// Struct representing the execution of the Back Syncer
/// BP over the context of a provided repos.
pub struct BacksyncProcessExecutor {
    fb: FacebookInit,
    matches: Arc<MononokeMatches<'static>>,
    source_repo_name: String,
    target_repo_name: String,
    cancellation_requested: Arc<AtomicBool>,
}

impl BacksyncProcessExecutor {
    pub(crate) fn new(
        fb: FacebookInit,
        matches: Arc<MononokeMatches<'static>>,
        source_repo_name: String,
        target_repo_name: String,
        // ctx: Arc<CoreContext>,
        // app: Arc<MononokeApp>,
        // repo_args: SourceAndTargetRepoArgs,
    ) -> Self {
        Self {
            fb,
            matches,
            source_repo_name,
            target_repo_name,
            cancellation_requested: Arc::new(AtomicBool::new(false)),
        }
    }
}

#[async_trait]
impl RepoShardedProcessExecutor for BacksyncProcessExecutor {
    async fn execute(&self) -> anyhow::Result<()> {
        info!(
            self.matches.logger(),
            "Initiating back syncer command execution for repo pair {}-{}",
            &self.source_repo_name,
            &self.target_repo_name,
        );
        run_backsyncer(
            self.fb,
            Arc::clone(&self.matches),
            self.source_repo_name.clone(),
            self.target_repo_name.clone(),
            Arc::clone(&self.cancellation_requested),
        )
        .await
        .with_context(|| {
            format!(
                "Error during back syncer command execution for repo pair {}-{}",
                &self.source_repo_name, &self.target_repo_name,
            )
        })?;
        info!(
            self.matches.logger(),
            "Finished back syncer command execution for repo pair {}-{}",
            &self.source_repo_name,
            self.target_repo_name
        );
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        info!(
            self.matches.logger(),
            "Terminating back syncer command execution for repo pair {}-{}",
            &self.source_repo_name,
            self.target_repo_name,
        );
        self.cancellation_requested.store(true, Ordering::Relaxed);
        Ok(())
    }
}
