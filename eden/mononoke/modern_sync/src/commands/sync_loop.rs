/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use async_trait::async_trait;
use clap::Parser;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use mononoke_app::MononokeApp;
use mononoke_app::args::SourceRepoArgs;
use repo_blobstore::RepoBlobstoreRef;
use sharding_ext::RepoShard;

use crate::ModernSyncArgs;
use crate::Repo;
use crate::sync::ExecutionType;

const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;

/// Replays bookmark's moves
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(flatten, next_help_heading = "SYNC OPTIONS")]
    sync_args: crate::sync::SyncArgs,

    #[clap(long = "start-id", help = "Start id for the sync [default: 0]")]
    start_id: Option<u64>,
    #[clap(long, help = "Print sent items without actually syncing")]
    dry_run: bool,
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
        // we cannot trust the repo arguments from app arguments, we need to parse them from the shard
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

        tracing::info!(
            "Setting up sharded sync from repo {} to repo {}",
            source_repo_name,
            target_repo_name,
        );

        let ctx =
            crate::sync::build_context(self.app.clone(), &source_repo_name, self.sync_args.dry_run);

        let source_repo_args = SourceRepoArgs::with_name(source_repo_name.clone());
        let repo: Repo = self.app.open_repo_unredacted(&source_repo_args).await?;
        let config = repo.repo_config.modern_sync_config.clone().ok_or(anyhow!(
            "No modern sync config found for repo {}",
            source_repo_name
        ))?;
        let repo_blobstore = repo.repo_blobstore();

        // Check that we can connect to the target repo
        let app_args = self.app.args::<ModernSyncArgs>()?;
        _ = crate::sync::build_edenfs_client(
            ctx,
            &app_args,
            &target_repo_name,
            &config,
            repo_blobstore,
        )
        .await
        .with_context(|| format!("checking that we can connect to {}", target_repo_name))?;

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
        let cancellation_requested = Arc::new(AtomicBool::new(false));
        crate::sync::sync(
            self.app.clone(),
            self.sync_args.start_id,
            self.source_repo_args.clone(),
            self.dest_repo_name.clone(),
            ExecutionType::Tail,
            self.sync_args.dry_run,
            self.app.args::<ModernSyncArgs>()?.exit_file.clone(),
            None,
            None,
            cancellation_requested,
        )
        .await?;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let args = Arc::new(args);
    let app_args = &app.args::<ModernSyncArgs>()?;

    let process = Arc::new(ModernSyncProcess::new(Arc::new(app), args.clone()));
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
        tracing::info!("Running sharded sync loop");
        executor
            .block_and_execute(&logger, Arc::new(AtomicBool::new(false)))
            .await?;
    } else {
        tracing::info!("Running unsharded sync loop");

        // we can trust the repo arguments
        let sync_args = &args.clone().sync_args;
        let source_repo: Repo = process.app.clone().open_repo(&sync_args.repo).await?;
        let source_repo_name = source_repo.repo_identity.name().to_string();
        let target_repo_name = &sync_args
            .dest_repo_name
            .clone()
            .unwrap_or(source_repo_name.clone());
        let target_repo_name = target_repo_name.to_string();

        let source_repo_args = SourceRepoArgs::with_name(source_repo_name.clone());

        tracing::info!(
            "Setting up unsharded sync from repo {:?} to repo {:?}",
            source_repo_name,
            target_repo_name,
        );

        let cancellation_requested = Arc::new(AtomicBool::new(false));
        crate::sync::sync(
            process.app.clone(),
            process.sync_args.start_id.clone(),
            source_repo_args,
            target_repo_name,
            ExecutionType::Tail,
            process.sync_args.dry_run.clone(),
            exit_file.clone(),
            None,
            None,
            cancellation_requested,
        )
        .await?;
    }
    Ok(())
}
