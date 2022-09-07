/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl pid

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::EdenFsInstance;

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Print the daemon's process ID if running")]
pub struct PidCmd {}

#[async_trait]
impl crate::Subcommand for PidCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let health = instance.get_health(None).await;

        Ok(match health {
            Ok(health) => {
                println!("{}", health.pid);
                0
            }
            Err(cause) => {
                eprintln!("edenfs not healthy: {}", cause);
                1
            }
        })
    }
}
