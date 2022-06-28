/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use anyhow::Result;
use clap::Parser;
use fbinit::FacebookInit;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;

mod commands;

/// Tools for Mononoke Tests
#[derive(Parser)]
struct TestToolArgs {}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let subcommands = commands::subcommands();
    let app = MononokeAppBuilder::new(fb).build_with_subcommands::<TestToolArgs>(subcommands)?;
    app.run(async_main)
}

async fn async_main(app: MononokeApp) -> Result<()> {
    if app.is_production() {
        return Err(anyhow!(
            "mononoke-testtool cannot be run against production"
        ));
    }
    commands::dispatch(app).await
}
