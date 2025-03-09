/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use mononoke_app::MononokeApp;
use slog::info;

use crate::commands::sync_loop::CHUNK_SIZE_DEFAULT;
use crate::sync::get_unsharded_repo_args;
use crate::sync::ExecutionType;
use crate::ModernSyncArgs;

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

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let app = Arc::new(app);
    let app_args = &app.args::<ModernSyncArgs>()?;
    let (source_repo_args, dest_repo_name) = get_unsharded_repo_args(app.clone(), app_args).await?;

    info!(app.logger(), "Running sync-once loop");
    crate::sync::sync(
        app,
        args.start_id.clone(),
        source_repo_args,
        dest_repo_name,
        ExecutionType::SyncOnce,
        args.dry_run,
        args.chunk_size.clone().unwrap_or(CHUNK_SIZE_DEFAULT),
        PathBuf::from(""),
    )
    .await?;

    Ok(())
}
