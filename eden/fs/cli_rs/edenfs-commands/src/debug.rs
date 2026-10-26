/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl debug

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;

use crate::ExitCode;
use crate::Subcommand;

mod bench;
mod counters;
mod stress;
mod subscribe;
#[cfg(target_os = "linux")]
mod systemd;

#[derive(Parser, Debug)]
#[clap(about = "Internal commands for examining eden state")]
pub struct DebugCmd {
    #[clap(subcommand)]
    subcommand: DebugSubcommand,
}

#[derive(Parser, Debug)]
pub enum DebugSubcommand {
    #[clap(subcommand)]
    Counters(counters::CountersCmd),
    Subscribe(subscribe::SubscribeCmd),
    #[clap(subcommand)]
    Stress(stress::StressCmd),
    #[clap(subcommand)]
    Bench(bench::cmd::BenchCmd),
    #[cfg(target_os = "linux")]
    Systemd(systemd::SystemdCmd),
}

#[async_trait]
impl Subcommand for DebugCmd {
    async fn run(&self) -> Result<ExitCode> {
        use DebugSubcommand::*;
        let sc: &(dyn Subcommand + Send + Sync) = match &self.subcommand {
            Counters(cmd) => cmd,
            Subscribe(cmd) => cmd,
            Stress(cmd) => cmd,
            Bench(cmd) => cmd,
            #[cfg(target_os = "linux")]
            Systemd(cmd) => cmd,
        };
        sc.run().await
    }
}
