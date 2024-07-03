/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

pub mod disable;
pub mod enable;
pub mod show;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use context::CoreContext;
use mononoke_app::MononokeApp;

use self::disable::disable;
use self::disable::DisableArgs;
use self::enable::enable;
use self::enable::EnableArgs;
use self::show::show;
use self::show::ShowArgs;

/// Manage pushredirect configuration
#[derive(Parser)]
pub struct PushRedirectionArgs {
    #[clap(subcommand)]
    subcommand: PushRedirectionSubcommand,
}

#[derive(Subcommand)]
enum PushRedirectionSubcommand {
    Disable(DisableArgs),
    Enable(EnableArgs),
    Show(ShowArgs),
}

pub async fn run(ctx: &CoreContext, app: MononokeApp, args: PushRedirectionArgs) -> Result<()> {
    match args.subcommand {
        PushRedirectionSubcommand::Disable(args) => disable(ctx, app, args).await?,
        PushRedirectionSubcommand::Enable(args) => enable(ctx, app, args).await?,
        PushRedirectionSubcommand::Show(args) => show(ctx, app, args).await?,
    }

    Ok(())
}
