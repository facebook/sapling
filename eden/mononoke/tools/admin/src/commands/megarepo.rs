/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub(crate) mod common;
mod merge;
mod move_commit;
mod pushredirection;
mod run_mover;
mod sync_diamond_merge;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::MononokeApp;

use self::merge::MergeArgs;
use self::move_commit::MoveArgs;
use self::pushredirection::PushRedirectionArgs;
use self::run_mover::RunMoverArgs;
use self::sync_diamond_merge::SyncDiamondMergeArgs;

/// Manage megarepo
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(subcommand)]
    subcommand: MegarepoSubcommand,
}

#[derive(Subcommand)]
enum MegarepoSubcommand {
    /// Manage which repos are pushredirected to the large repo
    PushRedirection(PushRedirectionArgs),
    Merge(MergeArgs),
    MoveCommit(MoveArgs),
    RunMover(RunMoverArgs),
    SyncDiamondMerge(SyncDiamondMergeArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    match args.subcommand {
        MegarepoSubcommand::PushRedirection(args) => pushredirection::run(&ctx, app, args).await?,
        MegarepoSubcommand::Merge(args) => merge::run(&ctx, app, args).await?,
        MegarepoSubcommand::MoveCommit(args) => move_commit::run(&ctx, app, args).await?,
        MegarepoSubcommand::RunMover(args) => run_mover::run(&ctx, app, args).await?,
        MegarepoSubcommand::SyncDiamondMerge(args) => {
            sync_diamond_merge::run(&ctx, app, args).await?
        }
    }

    Ok(())
}
