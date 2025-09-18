/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl notify enter-state

use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_asserted_states_client::AssertedStatesClient;
use edenfs_client::utils::get_mount_point;

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Enters the listed EdenFS notifications state until cancelled")]
pub struct EnterStateCmd {
    #[clap(help = "Name of the state being entered")]
    name: String,

    #[clap(
        short = 'd',
        long,
        help = "How long to hold the state for in seconds. Default=until cancelled"
    )]
    duration: Option<u64>,

    #[clap(help = "Path to the mount point. Defaults to cwd if not specified")]
    mount_point: Option<PathBuf>,
}

#[async_trait]
impl crate::Subcommand for EnterStateCmd {
    #[cfg(not(fbcode_build))]
    async fn run(&self) -> Result<ExitCode> {
        eprintln!("not supported in non-fbcode build");
        Ok(1)
    }

    #[cfg(fbcode_build)]
    async fn run(&self) -> Result<ExitCode> {
        let asserted_states_client =
            AssertedStatesClient::new(&get_mount_point(&self.mount_point)?)?;
        let _state = asserted_states_client.enter_state(&self.name);
        match self.duration {
            Some(duration) => {
                println!("Holding state for {} seconds", duration);
                std::thread::sleep(std::time::Duration::from_secs(duration));
            }
            None => {
                println!("Press enter to release state");
                let _ = std::io::stdin().read_line(&mut String::new())?;
            }
        }
        // State is released when the dropped as the program exits

        Ok(0)
    }
}
