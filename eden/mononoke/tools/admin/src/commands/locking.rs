/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod lock;
mod status;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::MononokeApp;

/// Show and control repository locks.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(subcommand)]
    subcommand: LockingSubcommand,
}

#[derive(Subcommand)]
pub enum LockingSubcommand {
    /// Show the status of repository locks.
    Status(status::LockingStatusArgs),
    /// Lock a repository.
    Lock(lock::LockingLockArgs),
    /// Unlock a repository.
    Unlock(lock::LockingUnlockArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    use LockingSubcommand::*;
    match args.subcommand {
        Status(args) => status::locking_status(&app, args).await?,
        Lock(args) => lock::locking_lock(&app, args).await?,
        Unlock(args) => lock::locking_unlock(&app, args).await?,
    }
    Ok(())
}
