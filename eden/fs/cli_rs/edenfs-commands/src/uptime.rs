/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl uptime

use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::daemon_info::DaemonHealthy;
use edenfs_utils::humantime::HumanTime;

use crate::ExitCode;
use crate::get_edenfs_instance;

#[derive(Parser, Debug)]
#[clap(about = "Determine uptime of running edenfs daemon")]
pub struct UptimeCmd {}

#[async_trait]
impl crate::Subcommand for UptimeCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let client = instance.get_client();
        let health = client.get_health(None).await;

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
