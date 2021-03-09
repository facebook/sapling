/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl top

use async_trait::async_trait;
use structopt::StructOpt;

use edenfs_client::EdenFsInstance;
use edenfs_error::Result;

use crate::ExitCode;

#[derive(StructOpt, Debug)]
#[structopt(about = "Monitor EdenFS accesses by process.")]
pub struct TopCmd {
    /// Don't accumulate data; refresh the screen every update
    /// cycle.
    #[structopt(short, long)]
    ephemeral: bool,

    /// Specify the rate (in seconds) at which eden top updates.
    #[structopt(short, long, default_value = "1")]
    refresh_rate: u64,
}

#[async_trait]
impl crate::Subcommand for TopCmd {
    async fn run(&self, _instance: EdenFsInstance) -> Result<ExitCode> {
        println!("eden top");
        Ok(0)
    }
}
