/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Parser;
use cmdlib_scrubbing::ScrubAppExtension;
use fbinit::FacebookInit;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;

mod bookmark_log_entry;
mod commands;
mod commit_id;

/// Administrate Mononoke
#[derive(Parser)]
struct AdminArgs {}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let subcommands = commands::subcommands();
    let app = MononokeAppBuilder::new(fb)
        .with_app_extension(ScrubAppExtension::new())
        .build_with_subcommands::<AdminArgs>(subcommands)?;
    app.run(async_main)
}

async fn async_main(app: MononokeApp) -> Result<()> {
    commands::dispatch(app).await
}
