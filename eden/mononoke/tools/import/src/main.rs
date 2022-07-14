/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use clap::Parser;
use fbinit::FacebookInit;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;

mod commands;

#[derive(Parser)]
struct ImportArgs {}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let subcommands = commands::subcommands();
    let app = MononokeAppBuilder::new(fb).build_with_subcommands::<ImportArgs>(subcommands)?;
    app.run(async_main)
}

async fn async_main(app: MononokeApp) -> Result<()> {
    commands::dispatch(app).await
}
