/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl debug systemd

use anyhow::Context as _;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;

use crate::ExitCode;
use crate::get_edenfs_instance;

/// Print the systemd unit name for this EdenFS instance.
#[derive(Parser, Debug)]
#[clap(about = "Print the systemd unit name for this EdenFS instance")]
pub struct SystemdCmd {}

#[async_trait]
impl crate::Subcommand for SystemdCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let unit = edenfs_client::daemon::get_systemd_unit(instance)
            .context("Failed to get systemd unit name")?;
        println!("{unit}");
        Ok(0)
    }
}
