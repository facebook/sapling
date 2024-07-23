/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow::Result;
use clap::Parser;
use fbinit::FacebookInit;
use tokio::time::Duration;

#[derive(Debug, Parser)]
pub struct Args {
    /// Seconds between checking for new updates to Mononoke Git repositories.
    #[arg(long = "mononoke-polling-interval", default_value = "5")]
    mononoke_polling_interval: u64,
}

pub async fn tail(_fb: FacebookInit, args: Args) -> Result<()> {
    let mut interval = tokio::time::interval(Duration::from_secs(args.mononoke_polling_interval));

    loop {
        interval.tick().await;
        println!("Future goodness yet to be implemented!");
    }
}
