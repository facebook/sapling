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
}

#[async_trait]
impl crate::Subcommand for ListCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let mounts = get_mounts(instance).await?;
        if self.json {
            println!("{}", serde_json::to_string_pretty(&mounts)?);
        } else {
            for (_, mount) in mounts {
                println!("{}", mount);
            }
        }

        Ok(0)
    }
}
