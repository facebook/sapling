/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::ensure;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use mononoke_app::args::MultiRepoArgs;
use mononoke_app::MononokeApp;
use sharding_ext::RepoShard;
use slog::info;
use slog::Logger;

use crate::ModernSyncArgs;

const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;

/// Replays bookmark's moves
#[derive(Parser)]
pub struct CommandArgs {}

pub async fn setup_sync(repos_args: &MultiRepoArgs, app: &MononokeApp) -> Result<()> {
    let repos = app.multi_repo_configs(repos_args.ids_or_names()?)?;
    let repo_count = repos.len();
    ensure!(
        repo_count == 1,
        "Modern sync only supports one repo at a time for now "
    );

    info!(
        app.logger(),
        "Syncing repos {:?}",
        repos.iter().map(|repo| &repo.0).collect::<Vec<_>>()
    );

    Ok(())
}

pub struct ModernSyncProcess {
    app: MononokeApp,
}

impl ModernSyncProcess {
    fn new(app: MononokeApp) -> Self {
        Self { app }
    }
}

#[async_trait]
impl RepoShardedProcess for ModernSyncProcess {
    async fn setup(&self, repo: &RepoShard) -> anyhow::Result<Arc<dyn RepoShardedProcessExecutor>> {
        let repo_name = repo.repo_name.as_str();

        let repos = MultiRepoArgs {
            repo_name: vec![repo_name.to_string()],
            repo_id: vec![],
        };
        setup_sync(&repos, &self.app).await?;

        Ok(Arc::new(ModernSyncProcessExecutor {
            logger: self.app.logger().clone(),
            repo_name: repo_name.to_string(),
        }))
    }
}

pub async fn sync() {}
pub struct ModernSyncProcessExecutor {
    logger: Logger,
    repo_name: String,
}

#[async_trait]
impl RepoShardedProcessExecutor for ModernSyncProcessExecutor {
    async fn execute(&self) -> Result<()> {
        info!(
            self.logger,
            "Starting up modern sync for repo {}", &self.repo_name
        );
        sync().await;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!(self.logger, "Finishing modern sync {}", &self.repo_name);
        Ok(())
    }
}

pub async fn run(app: MononokeApp, _args: CommandArgs) -> Result<()> {
    let sync_args = &app.args::<ModernSyncArgs>()?;
    let process = Arc::new(ModernSyncProcess::new(app));
    let logger = process.app.logger().clone();
    if let Some(mut executor) = sync_args.sharded_executor_args.clone().build_executor(
        process.app.fb,
        process.app.runtime().clone(),
        &logger,
        || process.clone(),
        true, // enable shard (repo) level healing
        SM_CLEANUP_TIMEOUT_SECS,
    )? {
        info!(logger, "Running sharded sync loop");
        executor
            .block_and_execute(&logger, Arc::new(AtomicBool::new(false)))
            .await?;
    } else {
        info!(logger, "Running unsharded sync loop");
        setup_sync(&sync_args.repos, &process.app).await?;
        sync().await;
    }
    Ok(())
}
