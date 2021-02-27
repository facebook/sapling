/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl pid

use structopt::StructOpt;

use edenfs_client::EdenFsInstance;
use edenfs_error::Result;

use crate::ExitCode;

#[derive(StructOpt, Debug)]
#[structopt(about = "Print the daemon's process ID if running")]
pub struct PidCmd {}

impl PidCmd {
    pub async fn run(self, instance: EdenFsInstance) -> Result<ExitCode> {
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
