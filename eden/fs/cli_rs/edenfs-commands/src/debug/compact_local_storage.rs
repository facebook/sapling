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
#[clap(about = "Asks RocksDB to compact its storage")]
pub struct CompactLocalStorageCmd {}

#[async_trait]
impl crate::Subcommand for CompactLocalStorageCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let client = instance.get_client();

        client.debug_compact_local_storage().await?;
        Ok(0)
    }
}
