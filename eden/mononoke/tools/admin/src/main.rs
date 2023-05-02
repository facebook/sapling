/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

use anyhow::Result;
use clap::Parser;
use cmdlib_scrubbing::ScrubAppExtension;
use fbinit::FacebookInit;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;

mod bookmark_log_entry;
mod commands;
mod commit_id;
#[cfg(fbcode_build)]
mod facebook;

/// Administrate Mononoke
#[derive(Parser)]
struct AdminArgs {}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    #[cfg(not(fbcode_build))]
    let subcommands = commands::subcommands();

    #[cfg(fbcode_build)]
    let subcommands = {
        let mut subcommands = commands::subcommands();
        subcommands.extend(facebook::commands::subcommands());
        subcommands
    };

    let app = MononokeAppBuilder::new(fb)
        .with_app_extension(ScrubAppExtension::new())
        .build_with_subcommands::<AdminArgs>(subcommands)?;
    app.run_basic(async_main)
}

async fn async_main(app: MononokeApp) -> Result<()> {
    #[cfg(not(fbcode_build))]
    commands::dispatch(app).await?;

    #[cfg(fbcode_build)]
    {
        if commands::subcommand_is_in_scope(&app) {
            commands::dispatch(app).await?;
        } else if facebook::commands::subcommand_is_in_scope(&app) {
            facebook::commands::dispatch(app).await?;
        }
    }

    Ok(())
}
