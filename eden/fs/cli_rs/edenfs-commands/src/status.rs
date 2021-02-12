/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl status

use std::time::Duration;

use anyhow::Result;
use structopt::StructOpt;

use edenfs_client::{DaemonHealthy, EdenFsInstance};

use crate::ExitCode;

#[derive(StructOpt, Debug)]
pub struct StatusCmd {
    /// Wait up to TIMEOUT seconds for the daemon to respond
    #[structopt(long, default_value = "3")]
    timeout: u64,
}

impl StatusCmd {
    async fn get_status(&self, instance: &EdenFsInstance) -> Result<i32> {
        let health = instance
            .get_health(Some(Duration::from_secs(self.timeout)))
            .await;

        if let Ok(health) = health {
            if health.is_healthy() {
                return Ok(health.pid);
            }
        }

        instance.status_from_lock()
    }

    pub async fn run(self, instance: EdenFsInstance) -> ExitCode {
        let status = self.get_status(&instance).await;

        match status {
            Ok(pid) => {
                println!("EdenFS is running normally (pid {})", pid);
                0
            }
            Err(cause) => {
                println!("EdenFS is not healthy: {}", cause);
                1
            }
        }
    }
}
