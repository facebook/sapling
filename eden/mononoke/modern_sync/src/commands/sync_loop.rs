/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::format_err;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkUpdateLogRef;
use clap::Parser;
use clientinfo::ClientEntryPoint;
use clientinfo::ClientInfo;
use context::CoreContext;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use mononoke_app::args::AsRepoArg;
use mononoke_app::args::RepoArg;
use mononoke_app::MononokeApp;
use mutable_counters::MutableCountersRef;
use repo_identity::RepoIdentityRef;
use sharding_ext::RepoShard;
use slog::info;

use crate::ModernSyncArgs;
use crate::Repo;

const SM_CLEANUP_TIMEOUT_SECS: u64 = 120;
const MODERN_SYNC_COUNTER_NAME: &str = "modern_sync";

/// Replays bookmark's moves
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(long = "start-id", help = "Start id for the sync [default: 0]")]
    start_id: Option<u64>,
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
        Ok(Arc::new(ModernSyncProcessExecutor {
            app: self.app.clone(),
            sync_args: self.sync_args.clone(),
            repo_arg: RepoArg::Name(repo.repo_name.clone()),
        }))
    }
}

pub struct ModernSyncProcessExecutor {
    app: Arc<MononokeApp>,
    sync_args: Arc<CommandArgs>,
    repo_arg: RepoArg,
}

#[async_trait]
impl RepoShardedProcessExecutor for ModernSyncProcessExecutor {
    async fn execute(&self) -> Result<()> {
        sync(
            self.app.clone(),
            self.sync_args.clone(),
            self.repo_arg.clone(),
        )
        .await?;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        Ok(())
    }
}

pub async fn sync(
    app: Arc<MononokeApp>,
    sync_args: Arc<CommandArgs>,
    repo_arg: RepoArg,
) -> Result<()> {
    let repo: Repo = app.open_repo(&repo_arg).await?;
    let _repo_id = repo.repo_identity().id();
    let repo_name = repo.repo_identity().name().to_string();

    let ctx = CoreContext::new_with_logger_and_client_info(
        app.fb,
        app.logger().clone(),
        ClientInfo::default_with_entry_point(ClientEntryPoint::ModernSync),
    )
    .clone_with_repo_name(&repo_name);

    let _bookmark_update_log = repo.bookmark_update_log();
    let start_id;

    if let Some(id) = sync_args.start_id {
        start_id = id
    } else {
        start_id = repo
            .mutable_counters()
            .get_counter(&ctx, MODERN_SYNC_COUNTER_NAME)
            .await?
            .map(|val| val.try_into())
            .transpose()?
            .ok_or_else(|| {
                format_err!(
                    "No start-id or mutable counter {} provided",
                    MODERN_SYNC_COUNTER_NAME
                )
            })?;
    };

    info!(app.logger(), "Starting with value {}", start_id);

    Ok(())
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let app_args = &app.args::<ModernSyncArgs>()?;

    let process = Arc::new(ModernSyncProcess::new(Arc::new(app), Arc::new(args)));
    let logger = process.app.logger().clone();

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
        sync(
            process.app.clone(),
            process.sync_args.clone(),
            app_args.repo.as_repo_arg().clone(),
        )
        .await?;
    }
    Ok(())
}
