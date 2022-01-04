/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl config

use async_trait::async_trait;
use structopt::{clap::AppSettings, StructOpt};

use edenfs_client::EdenFsInstance;
use edenfs_error::Result;

use crate::{ExitCode, Subcommand};

mod clear_local_caches;
mod compact_local_storage;

#[derive(StructOpt, Debug)]
#[structopt(
    about = "Internal commands for examining eden state",
    setting = AppSettings::DisableHelpFlags,
)]
pub struct DebugCmd {
    #[structopt(subcommand)]
    subcommand: DebugSubcommand,
}

#[derive(StructOpt, Debug)]
pub enum DebugSubcommand {
    ClearLocalCaches(clear_local_caches::ClearLocalCachesCmd),
    CompactLocalStorage(compact_local_storage::CompactLocalStorageCmd),
}

#[async_trait]
impl Subcommand for DebugCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        use DebugSubcommand::*;
        let sc: &(dyn Subcommand + Send + Sync) = match &self.subcommand {
            ClearLocalCaches(cmd) => cmd,
            CompactLocalStorage(cmd) => cmd,
        };
        sc.run(instance).await
    }
}
