/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl notify get-states

use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_asserted_states_client::AssertedStatesClient;
use edenfs_client::utils::get_mount_point;

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Gets and lists all currently asserted EdenFS notifications states")]
pub struct GetStatesCmd {
    #[clap(help = "Path to the mount point. Defaults to cwd if not specified")]
    mount_point: Option<PathBuf>,

    #[clap(long, help = "Print the output in JSON format")]
    json: bool,
}

#[async_trait]
impl crate::Subcommand for GetStatesCmd {
    #[cfg(not(fbcode_build))]
    async fn run(&self) -> Result<ExitCode> {
        eprintln!("not supported in non-fbcode build");
        Ok(1)
    }

    #[cfg(fbcode_build)]
    async fn run(&self) -> Result<ExitCode> {
        let asserted_states_client =
            AssertedStatesClient::new(&get_mount_point(&self.mount_point)?)?;
        let states = asserted_states_client.get_asserted_states()?;
        if self.json {
            println!("{}", serde_json::to_string(&states)?);
        } else if states.is_empty() {
            println!("No states found");
        } else {
            println!("Asserted States:");
            for state in states {
                println!("{}", state);
            }
        }
        Ok(0)
    }
}
