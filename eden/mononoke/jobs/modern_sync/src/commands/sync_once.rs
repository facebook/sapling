/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use anyhow::Result;
use clap::Parser;
use mononoke_app::MononokeApp;

use crate::sync::ExecutionType;
use crate::sync::get_unsharded_repo_args;

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
    let (source_repo_args, _, dest_repo_name) =
        get_unsharded_repo_args(app.clone(), &args.sync_args).await?;

    tracing::info!("Running sync-once loop");
    let cancellation_requested = Arc::new(AtomicBool::new(false));
    crate::sync::sync(
        app,
        args.start_id.clone(),
        source_repo_args,
        dest_repo_name,
        ExecutionType::SyncOnce,
        args.dry_run,
        PathBuf::from(""),
        None,
        None,
        cancellation_requested,
    )
    .await?;

    Ok(())
}
