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

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::MononokeApp;

use self::merge::MergeArgs;
use self::move_commit::MoveArgs;
use self::pushredirection::PushRedirectionArgs;

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
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();

    match args.subcommand {
        MegarepoSubcommand::PushRedirection(args) => pushredirection::run(&ctx, app, args).await?,
        MegarepoSubcommand::Merge(args) => merge::run(&ctx, app, args).await?,
        MegarepoSubcommand::MoveCommit(args) => move_commit::run(&ctx, app, args).await?,
    }

    Ok(())
}
