/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use async_trait::async_trait;
use backsyncer::Repo;
use context::CoreContext;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use mononoke_app::MononokeApp;
use mononoke_app::args::SourceAndTargetRepoArgs;
use repo_identity::RepoIdentityRef;
use sharding_ext::RepoShard;
use slog::error;
use slog::info;

use crate::run::run_backsyncer;

pub(crate) const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;
pub(crate) const APP_NAME: &str = "backsyncer cmd-line tool";

/// Struct representing the Back Syncer BP.
pub struct BacksyncProcess {
    // TODO(T213755338): remove Arcs from CoreContext in all cross-repo binaries
    ctx: Arc<CoreContext>,
    pub(crate) app: Arc<MononokeApp>,
}

impl BacksyncProcess {
    pub(crate) fn new(ctx: Arc<CoreContext>, app: Arc<MononokeApp>) -> Self {
        Self { app, ctx }
    }
}

#[async_trait]
impl RepoShardedProcess for BacksyncProcess {
    async fn setup(&self, repo: &RepoShard) -> Result<Arc<dyn RepoShardedProcessExecutor>> {
        let logger = self.ctx.logger();

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

        let repo_args = SourceAndTargetRepoArgs::with_source_and_target_repo_name(
            source_repo_name.clone(),
            target_repo_name.clone(),
        );

        let executor =
            BacksyncProcessExecutor::new(self.ctx.clone(), self.app.clone(), repo_args).await?;

        let details = format!(
            "Completed back syncer command setup from repo {} to repo {}",
            source_repo_name, target_repo_name
        );
        info!(logger, "{}", details);
        Ok(Arc::new(executor))
    }
}

/// Struct representing the execution of the Back Syncer
/// BP over the context of a provided repos.
pub struct BacksyncProcessExecutor {
    ctx: Arc<CoreContext>,
    app: Arc<MononokeApp>,
    large_repo: Repo,
    small_repo: Repo,
    cancellation_requested: Arc<AtomicBool>,
}

impl BacksyncProcessExecutor {
    pub(crate) async fn new(
        ctx: Arc<CoreContext>,
        app: Arc<MononokeApp>,
        repo_args: SourceAndTargetRepoArgs,
    ) -> Result<Self> {
        let large_repo: Repo = app.open_repo_unredacted(&repo_args.source_repo).await?;
        let small_repo: Repo = app.open_repo_unredacted(&repo_args.target_repo).await?;

        Ok(Self {
            ctx,
            app,
            large_repo,
            small_repo,
            cancellation_requested: Arc::new(AtomicBool::new(false)),
        })
    }
}

#[async_trait]
impl RepoShardedProcessExecutor for BacksyncProcessExecutor {
    async fn execute(&self) -> anyhow::Result<()> {
        let logger = self.ctx.logger();
        let large_repo_name = self.large_repo.repo_identity().name();
        let small_repo_name = self.small_repo.repo_identity().name();
        info!(
            self.ctx.logger(),
            "Initiating back syncer command execution for repo pair {large_repo_name}-{small_repo_name}",
        );

        let ctx = self.ctx.with_mutated_scuba(|mut scuba_sample| {
            scuba_sample.add("source_repo", self.large_repo.repo_identity().id().id());
            scuba_sample.add("source_repo_name", large_repo_name);
            scuba_sample.add("target_repo", self.small_repo.repo_identity().id().id());
            scuba_sample.add("target_repo_name", small_repo_name);
            scuba_sample
        });

        run_backsyncer(
            Arc::new(ctx),
            self.app.clone(),
            self.large_repo.clone(),
            self.small_repo.clone(),
            Arc::clone(&self.cancellation_requested),
        )
        .await
        .with_context(|| {
            format!(
                "Error during back syncer command execution for repo pair {large_repo_name}-{small_repo_name}",
            )
        })?;
        info!(
            logger,
            "Finished back syncer command execution for repo pair {large_repo_name}-{small_repo_name}",
        );
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        let large_repo_name = self.large_repo.repo_identity().name();
        let small_repo_name = self.small_repo.repo_identity().name();
        info!(
            self.ctx.logger(),
            "Terminating back syncer command execution for repo pair {large_repo_name}-{small_repo_name}",
        );
        self.cancellation_requested.store(true, Ordering::Relaxed);
        Ok(())
    }
}
