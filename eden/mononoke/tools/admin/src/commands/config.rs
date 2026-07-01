/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! `mononoke_admin config` — config-layer subcommands. Today: `check`.

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use mononoke_app::MononokeApp;

mod check;

/// Mononoke config operations.
#[derive(Parser)]
pub struct CommandArgs {
    #[clap(subcommand)]
    subcommand: ConfigSubcommand,
}

#[derive(Subcommand)]
pub enum ConfigSubcommand {
    /// Validate per-repo `.cconf` config changes by loading each affected
    /// repo through the same `MononokeApp` + `RepoFactory` code path the
    /// Mononoke server uses at startup. Intended to run pre-`jf submit`
    /// on a config diff.
    Check(check::CheckArgs),
}

pub async fn run(app: MononokeApp, args: CommandArgs) -> Result<()> {
    match args.subcommand {
        ConfigSubcommand::Check(check_args) => check::run(app, check_args).await,
    }
}
