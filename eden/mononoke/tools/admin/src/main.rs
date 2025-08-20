/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(duration_constructors_lite)]
#![feature(trait_alias)]

use anyhow::Result;
use clap::Parser;
use cmdlib_scrubbing::ScrubAppExtension;
use fbinit::FacebookInit;
use mononoke_app::MononokeApp;
use mononoke_app::MononokeAppBuilder;
use mononoke_app::monitoring::AliveService;
use mononoke_app::monitoring::MonitoringAppExtension;

mod bookmark_log_entry;
mod commands;
#[cfg(fbcode_build)]
mod facebook;

/// Administrate Mononoke
#[derive(Parser)]
struct AdminArgs {
    #[clap(long)]
    use_monitoring: bool,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    let mut subcommands = commands::subcommands();

    #[cfg(fbcode_build)]
    {
        subcommands.extend(facebook::commands::subcommands());
    }

    subcommands.sort_unstable_by(|a, b| a.get_name().cmp(b.get_name()));

    let app = MononokeAppBuilder::new(fb)
        .with_app_extension(ScrubAppExtension::new())
        .with_app_extension(MonitoringAppExtension {})
        .build_with_subcommands::<AdminArgs>(subcommands)?;

    let args: AdminArgs = app.args()?;
    if args.use_monitoring {
        app.run_with_monitoring_and_logging(async_main, "admin", AliveService)
    } else {
        app.run_basic(async_main)
    }
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
