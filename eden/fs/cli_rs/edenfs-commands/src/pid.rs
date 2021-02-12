/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl pid

use std::time::Duration;

use structopt::StructOpt;

use edenfs_client::EdenFsInstance;

use crate::ExitCode;

#[derive(StructOpt, Debug)]
pub struct PidCmd {
    /// Wait up to TIMEOUT seconds for the daemon to respond
    #[structopt(long, default_value = "3")]
    timeout: u64,
}

impl PidCmd {
    pub async fn run(self, instance: EdenFsInstance) -> ExitCode {
        let health = instance.get_health(Duration::from_secs(self.timeout)).await;

        match health {
            Ok(health) => {
                println!("{}", health.pid);
                0
            }
            Err(cause) => {
                println!("edenfs not healthy: {}", cause);
                1
            }
        }
    }
}
