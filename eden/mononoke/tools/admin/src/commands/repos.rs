/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod list;
mod show_locks;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::MononokeApp;

/// Operations over a whole repo
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(subcommand)]
    subcommand: ReposSubcommand,
}

#[derive(Subcommand)]
pub enum ReposSubcommand {
    /// List configured repositories
    List(list::ReposListArgs),
    /// Show all locks currently active
    ShowLocks(show_locks::ReposShowLocksArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    use ReposSubcommand::*;
    match args.subcommand {
        List(args) => list::repos_list(app, args).await?,
        ShowLocks(args) => show_locks::repos_show_locks(app, args).await?,
    }
    Ok(())
}
