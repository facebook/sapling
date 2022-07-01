/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl status

use std::time::Duration;

use anyhow::anyhow;
use async_trait::async_trait;
use clap::Parser;
use tokio::time;
use tracing::event;
use tracing::Level;

use edenfs_client::DaemonHealthy;
use edenfs_client::EdenFsInstance;
use edenfs_error::Result;

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Check the health of the EdenFS service")]
pub struct StatusCmd {
    /// Wait up to TIMEOUT seconds for the daemon to respond
    #[clap(long, default_value = "3")]
    timeout: u64,
}

impl StatusCmd {
    async fn get_status(&self, instance: &EdenFsInstance) -> Result<i32> {
        let timeout = Duration::from_secs(self.timeout);
        let health = instance.get_health(Some(timeout));
        let health = time::timeout(timeout, health).await;

        event!(Level::TRACE, ?health, "get_health");

        match health {
            Ok(Ok(health)) if health.is_healthy() => return Ok(health.pid),
            Ok(Ok(health)) => {
                event!(
                    Level::DEBUG,
                    ?health,
                    "Connected to EdenFS daemon but daemon reported unhealthy status"
                );
                if let Some(status) = health.status {
                    return Err(anyhow!("EdenFS is {}", status).into());
                }
            }
            Ok(Err(e)) => {
                event!(Level::DEBUG, ?e, "Error while connecting to EdenFS daemon");
            }
            Err(_) => {
                event!(Level::DEBUG, ?timeout, "Timeout exceeded");
            }
        }

        Ok(instance.status_from_lock()?)
    }
}

#[async_trait]
impl crate::Subcommand for StatusCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let status = self.get_status(&instance).await;

        Ok(match status {
            Ok(pid) => {
                println!("EdenFS is running normally (pid {})", pid);
                0
            }
            Err(cause) => {
                println!("EdenFS is not healthy: {}", cause);
                1
            }
        })
    }
}
