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
use cmdlib_cross_repo::create_commit_syncers_from_app;
use context::CoreContext;
use cross_repo_sync::Syncers;
use environment::MononokeEnvironment;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use mononoke_app::MononokeApp;
use mononoke_app::args::SourceAndTargetRepoArgs;
use repo_identity::RepoIdentityRef;
use sharding_ext::RepoShard;
use slog::error;
use slog::info;

use crate::run::loop_forever;

type Repo = cross_repo_sync::ConcreteRepo;

/// Struct representing the Bookmark Validate BP.
pub struct BookmarkValidateProcess {
    ctx: Arc<CoreContext>,
    pub(crate) app: Arc<MononokeApp>,
}

impl BookmarkValidateProcess {
    pub(crate) fn new(ctx: Arc<CoreContext>, app: Arc<MononokeApp>) -> Self {
        Self { app, ctx }
    }
}

#[async_trait]
impl RepoShardedProcess for BookmarkValidateProcess {
    async fn setup(&self, repo: &RepoShard) -> Result<Arc<dyn RepoShardedProcessExecutor>> {
        let logger = self.ctx.logger();

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
            "Setting up bookmark validate command from repo {} to repo {}",
            source_repo_name,
            target_repo_name,
        );
        let repo_args = SourceAndTargetRepoArgs::with_source_and_target_repo_name(
            source_repo_name.clone(),
            target_repo_name.clone(),
        );

        let executor =
            BookmarkValidateProcessExecutor::new(self.ctx.clone(), self.app.clone(), repo_args)
                .await?;

        let details = format!(
            "Completed bookmark validate command setup from repo {} to repo {}",
            &source_repo_name, &target_repo_name
        );
        info!(logger, "{}", details,);
        Ok(Arc::new(executor))
    }
}

/// Struct representing the execution of the Bookmark Validate
/// BP over the context of a provided repos.
pub struct BookmarkValidateProcessExecutor {
    syncers: Syncers<Arc<Repo>>,
    ctx: Arc<CoreContext>,
    env: Arc<MononokeEnvironment>,
    cancellation_requested: Arc<AtomicBool>,
    source_repo_name: String,
    target_repo_name: String,
}

impl BookmarkValidateProcessExecutor {
    pub(crate) async fn new(
        ctx: Arc<CoreContext>,
        app: Arc<MononokeApp>,
        repo_args: SourceAndTargetRepoArgs,
    ) -> Result<Self> {
        let env = app.environment().clone();
        let logger = ctx.logger();

        let source_repo: Arc<Repo> = app.open_repo_unredacted(&repo_args.source_repo).await?;
        let target_repo: Arc<Repo> = app.open_repo_unredacted(&repo_args.target_repo).await?;

        let syncers = create_commit_syncers_from_app(
            &ctx,
            app.as_ref(),
            source_repo.clone(),
            target_repo.clone(),
        )
        .await?;

        let source_repo_id = source_repo.repo_identity().id();
        let source_repo_name = source_repo.repo_identity().name();
        let target_repo_name = target_repo.repo_identity().name();

        if syncers.large_to_small.get_large_repo().repo_identity().id() != source_repo_id {
            let details = format!(
                "Source repo must be a large repo!. Source repo: {}, Target repo: {}",
                &source_repo_name, &target_repo_name
            );
            error!(logger, "{}", details);
            bail!("{}", details);
        }

        Ok(Self {
            syncers,
            ctx,
            env,
            source_repo_name: source_repo_name.to_string(),
            target_repo_name: target_repo_name.to_string(),
            cancellation_requested: Arc::new(AtomicBool::new(false)),
        })
    }
}

#[async_trait]
impl RepoShardedProcessExecutor for BookmarkValidateProcessExecutor {
    async fn execute(&self) -> anyhow::Result<()> {
        info!(
            self.ctx.logger(),
            "Initiating bookmark validate command execution for repo pair {}-{}",
            &self.source_repo_name,
            &self.target_repo_name,
        );
        loop_forever(
            self.ctx.as_ref(),
            &self.env,
            self.syncers.clone(),
            Arc::clone(&self.cancellation_requested),
        )
        .await
        .with_context(|| {
            format!(
                "Error during bookmark validate command execution for repo pair {}-{}",
                &self.source_repo_name, &self.target_repo_name,
            )
        })?;
        info!(
            self.ctx.logger(),
            "Finished bookmark validate command execution for repo pair {}-{}",
            &self.source_repo_name,
            self.target_repo_name
        );
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        info!(
            self.ctx.logger(),
            "Terminating bookmark validate command execution for repo pair {}-{}",
            self.source_repo_name,
            self.target_repo_name,
        );
        self.cancellation_requested.store(true, Ordering::Relaxed);
        Ok(())
    }
}
