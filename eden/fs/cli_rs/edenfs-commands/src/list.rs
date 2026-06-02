/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl list

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::checkout::get_mounts;

use crate::ExitCode;
use crate::get_edenfs_instance;

#[derive(Parser, Debug)]
#[clap(about = "List available checkouts")]
pub struct ListCmd {
    #[clap(long, help = "Print the output in JSON format")]
    json: bool,

    #[clap(
        long,
        short,
        help = "Show additional details such as filesystem channel and transport type"
    )]
    verbose: bool,
}

#[async_trait]
impl crate::Subcommand for ListCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let mounts = get_mounts(instance).await?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&mounts)?);
        } else {
            for mount in mounts.values() {
                println!("{}", mount.display(self.verbose));
            }
        }

        Ok(0)
    }
}
