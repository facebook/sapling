/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use mononoke_app::args::AsRepoArg;
use mononoke_app::MononokeApp;
use slog::info;

use crate::commands::sync_loop::CHUNK_SIZE_DEFAULT;
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
    let app_args = &app.args::<ModernSyncArgs>()?;

    info!(app.logger(), "Running sync-once loop");
    crate::sync::sync(
        Arc::new(app),
        args.start_id.clone(),
        app_args.repo.as_repo_arg().clone(),
        ExecutionType::SyncOnce,
        args.dry_run,
        args.chunk_size.clone().unwrap_or(CHUNK_SIZE_DEFAULT),
    )
    .await?;

    Ok(())
}
