/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod create;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::MononokeApp;

use self::create::CreateBundleArgs;

/// Perform git related operations.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(subcommand)]
    subcommand: GitBundleSubcommand,
}

#[derive(Subcommand)]
pub enum GitBundleSubcommand {
    /// Create Git bundle
    Create(CreateBundleArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    let ctx = app.new_basic_context();
    match args.subcommand {
        GitBundleSubcommand::Create(create_args) => create::create(&ctx, create_args).await?,
    }
    Ok(())
}
