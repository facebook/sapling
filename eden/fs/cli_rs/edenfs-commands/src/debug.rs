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
mod clear_local_caches;
mod compact_local_storage;
mod counters;
mod stress;
mod subscribe;

#[derive(Parser, Debug)]
#[clap(
    about = "Internal commands for examining eden state",
    disable_help_flag = true,
    disable_help_subcommand = true
)]
pub struct DebugCmd {
    #[clap(subcommand)]
    subcommand: DebugSubcommand,
}

#[derive(Parser, Debug)]
pub enum DebugSubcommand {
    ClearLocalCaches(clear_local_caches::ClearLocalCachesCmd),
    CompactLocalStorage(compact_local_storage::CompactLocalStorageCmd),
    #[clap(subcommand)]
    Counters(counters::CountersCmd),
    Subscribe(subscribe::SubscribeCmd),
    #[clap(subcommand)]
    Stress(stress::StressCmd),
    #[clap(subcommand)]
    Bench(bench::cmd::BenchCmd),
}

#[async_trait]
impl Subcommand for DebugCmd {
    async fn run(&self) -> Result<ExitCode> {
        use DebugSubcommand::*;
        let sc: &(dyn Subcommand + Send + Sync) = match &self.subcommand {
            ClearLocalCaches(cmd) => cmd,
            CompactLocalStorage(cmd) => cmd,
            Counters(cmd) => cmd,
            Subscribe(cmd) => cmd,
            Stress(cmd) => cmd,
            Bench(cmd) => cmd,
        };
        sc.run().await
    }
}
