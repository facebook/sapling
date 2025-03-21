/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::bail;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use mononoke_app::args::SourceRepoArgs;
use mononoke_app::MononokeApp;
use sharding_ext::RepoShard;
use slog::info;

use crate::sync::ExecutionType;
use crate::ModernSyncArgs;
use crate::Repo;

const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;
pub const CHUNK_SIZE_DEFAULT: u64 = 1000;

/// Replays bookmark's moves
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(long = "start-id", help = "Start id for the sync [default: 0]")]
    start_id: Option<u64>,
    #[clap(long, help = "Print sent items without actually syncing")]
    dry_run: bool,
    #[clap(long, help = "Chunk size for the sync [default: 1000]")]
    chunk_size: Option<u64>,
}

pub struct ModernSyncProcess {
    app: Arc<MononokeApp>,
    sync_args: Arc<CommandArgs>,
}

impl ModernSyncProcess {
    fn new(app: Arc<MononokeApp>, sync_args: Arc<CommandArgs>) -> Self {
        Self { app, sync_args }
    }
}

#[async_trait]
impl RepoShardedProcess for ModernSyncProcess {
    async fn setup(&self, repo: &RepoShard) -> anyhow::Result<Arc<dyn RepoShardedProcessExecutor>> {
        let logger = self.app.logger().clone();

        let source_repo_name = repo.repo_name.clone();
        let target_repo_name = match repo.target_repo_name.clone() {
            Some(repo_name) => repo_name,
            None => {
                let details = format!(
                    "Only source repo name {} provided, target repo name missing in {}",
                    source_repo_name, repo
                );
                bail!("{}", details)
            }
        };

        info!(
            logger,
            "Setting up sharded sync from repo {} to repo {}", source_repo_name, target_repo_name,
        );

        let source_repo_args = SourceRepoArgs::with_name(source_repo_name.clone());

        Ok(Arc::new(ModernSyncProcessExecutor {
            app: self.app.clone(),
            sync_args: self.sync_args.clone(),
            source_repo_args,
            dest_repo_name: target_repo_name,
        }))
    }
}

pub struct ModernSyncProcessExecutor {
    app: Arc<MononokeApp>,
    sync_args: Arc<CommandArgs>,
    source_repo_args: SourceRepoArgs,
    dest_repo_name: String,
}

#[async_trait]
impl RepoShardedProcessExecutor for ModernSyncProcessExecutor {
    async fn execute(&self) -> Result<()> {
        crate::sync::sync(
            self.app.clone(),
            self.sync_args.start_id,
            self.source_repo_args.clone(),
            self.dest_repo_name.clone(),
            ExecutionType::Tail,
            self.sync_args.dry_run,
            self.sync_args.chunk_size.unwrap_or(CHUNK_SIZE_DEFAULT),
            self.app.args::<ModernSyncArgs>()?.exit_file.clone(),
            false,
            None,
        )
        .await?;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let app_args = &app.args::<ModernSyncArgs>()?;

    let process = Arc::new(ModernSyncProcess::new(Arc::new(app), Arc::new(args)));
    let logger = process.app.logger().clone();

    let exit_file = app_args.exit_file.clone();

    if let Some(mut executor) = app_args.sharded_executor_args.clone().build_executor(
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

        let source_repo: Repo = process.app.clone().open_repo(&app_args.repo).await?;
        let source_repo_name = source_repo.repo_identity.name().to_string();
        let target_repo_name = app_args
            .dest_repo_name
            .clone()
            .unwrap_or(source_repo_name.clone());

        let source_repo_args = SourceRepoArgs::with_name(source_repo_name.clone());

        info!(
            logger,
            "Setting up unsharded sync from repo {:?} to repo {:?}",
            source_repo_name,
            target_repo_name,
        );

        crate::sync::sync(
            process.app.clone(),
            process.sync_args.start_id.clone(),
            source_repo_args,
            target_repo_name,
            ExecutionType::Tail,
            process.sync_args.dry_run.clone(),
            process
                .sync_args
                .chunk_size
                .clone()
                .unwrap_or(CHUNK_SIZE_DEFAULT),
            exit_file.clone(),
            false,
            None,
        )
        .await?;
    }
    Ok(())
}
