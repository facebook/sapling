/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use anyhow::ensure;
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
use fbinit::FacebookInit;
use mononoke_app::args::MultiRepoArgs;
use mononoke_app::args::RepoArg;
use mononoke_app::MononokeApp;
use mutable_counters::MutableCountersRef;
use repo_identity::RepoIdentityRef;
use sharding_ext::RepoShard;
use slog::info;
use slog::Logger;

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
        let repo_name = repo.repo_name.as_str();

        let repos = MultiRepoArgs {
            repo_name: vec![repo_name.to_string()],
            repo_id: vec![],
        };
        setup_sync(&repos, &self.app).await?;

        Ok(Arc::new(ModernSyncProcessExecutor {
            fb: self.app.fb,
            logger: self.app.logger().clone(),
            repo_name: repo_name.to_string(),
            app: self.app.clone(),
            sync_args: self.sync_args.clone(),
        }))
    }
}

pub struct ModernSyncProcessExecutor {
    fb: FacebookInit,
    logger: Logger,
    repo_name: String,
    app: Arc<MononokeApp>,
    sync_args: Arc<CommandArgs>,
}

#[async_trait]
impl RepoShardedProcessExecutor for ModernSyncProcessExecutor {
    async fn execute(&self) -> Result<()> {
        info!(
            self.logger,
            "Starting up modern sync for repo {}", &self.repo_name
        );
        let ctx = CoreContext::new_with_logger_and_client_info(
            self.fb,
            self.logger.clone(),
            ClientInfo::default_with_entry_point(ClientEntryPoint::ModernSync),
        )
        .clone_with_repo_name(&self.repo_name);
        sync(
            self.repo_name.clone(),
            &ctx,
            self.app.clone(),
            self.sync_args.clone(),
        )
        .await?;
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!(self.logger, "Finishing modern sync {}", &self.repo_name);
        Ok(())
    }
}

pub async fn setup_sync(repos_args: &MultiRepoArgs, app: &MononokeApp) -> Result<String> {
    let repo_count = repos_args.repo_name.len();
    ensure!(
        repo_count == 1,
        "Modern sync only supports one repo at a time for now "
    );

    info!(app.logger(), "Syncing repos {:?}", repos_args.repo_name);

    Ok(repos_args.repo_name[0].clone())
}

pub async fn sync(
    repo_name: String,
    ctx: &CoreContext,
    app: Arc<MononokeApp>,
    sync_args: Arc<CommandArgs>,
) -> Result<()> {
    let repo: Repo = app.open_repo(&RepoArg::Name(repo_name)).await?;
    let repo_id = repo.repo_identity().id();

    info!(app.logger(), "Syncing repo with id {}", repo_id);
    let _bookmark_update_log = repo.bookmark_update_log();
    let start_id;

    if let Some(id) = sync_args.start_id {
        start_id = id
    } else {
        start_id = repo
            .mutable_counters()
            .get_counter(ctx, MODERN_SYNC_COUNTER_NAME)
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
    let sync_args = &app.args::<ModernSyncArgs>()?;

    let process = Arc::new(ModernSyncProcess::new(Arc::new(app), Arc::new(args)));
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
        let repo_name = setup_sync(&sync_args.repos, &process.app).await?;
        let ctx = CoreContext::new_with_logger_and_client_info(
            process.app.fb,
            process.app.logger().clone(),
            ClientInfo::default_with_entry_point(ClientEntryPoint::ModernSync),
        )
        .clone_with_repo_name(&repo_name);
        sync(
            repo_name,
            &ctx,
            process.app.clone(),
            process.sync_args.clone(),
        )
        .await?;
    }
    Ok(())
}
