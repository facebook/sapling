/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl uptime

use std::time::Duration;

use async_trait::async_trait;
use clap::Parser;

use edenfs_client::DaemonHealthy;
use edenfs_client::EdenFsInstance;
use edenfs_error::Result;
use edenfs_utils::humantime::HumanTime;

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Determine uptime of running edenfs daemon")]
pub struct UptimeCmd {}

#[async_trait]
impl crate::Subcommand for UptimeCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let health = instance.get_health(None).await;

        match health {
            Ok(health) => {
                if health.is_healthy() {
                    let uptime = Duration::from_secs_f32(health.uptime.unwrap_or(0.0));
                    println!(
                        "edenfs uptime (pid: {}): {:#}",
                        health.pid,
                        HumanTime::from(uptime)
                    );
                } else {
                    println!("edenfs (pid: {}) is not healthy", health.pid);
                }
                Ok(0)
            }
            Err(cause) => {
                println!("edenfs not healthy: {}", cause);
                Ok(1)
            }
        }
    }
}
