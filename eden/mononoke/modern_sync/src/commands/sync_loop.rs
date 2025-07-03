/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::Result;
use clap::Parser;
use mononoke_app::MononokeApp;
use mononoke_app::args::SourceRepoArgs;

use crate::ModernSyncArgs;
use crate::Repo;
use crate::sync::ExecutionType;

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

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let app = Arc::new(app);
    let args = Arc::new(args);
    let app_args = &app.args::<ModernSyncArgs>()?;

    let exit_file = app_args.exit_file.clone();

    tracing::info!("Running unsharded sync loop");

    let sync_args = &args.clone().sync_args;
    let source_repo: Repo = app.clone().open_repo(&sync_args.repo).await?;
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
        app.clone(),
        args.start_id.clone(),
        source_repo_args,
        target_repo_name,
        ExecutionType::Tail,
        args.dry_run.clone(),
        exit_file.clone(),
        None,
        None,
        cancellation_requested,
    )
    .await?;

    Ok(())
}
