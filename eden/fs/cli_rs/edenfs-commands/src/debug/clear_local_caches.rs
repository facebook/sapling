/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl config

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;

use crate::ExitCode;
use crate::get_edenfs_instance;

#[derive(Parser, Debug)]
#[clap(about = "Clears local caches of objects stored in RocksDB")]
pub struct ClearLocalCachesCmd {}

#[async_trait]
impl crate::Subcommand for ClearLocalCachesCmd {
    async fn run(&self) -> Result<ExitCode> {
        // noop
        Ok(0)
    }
}
