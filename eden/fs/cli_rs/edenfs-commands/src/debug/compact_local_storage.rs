/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl config

use async_trait::async_trait;
use structopt::StructOpt;

use edenfs_client::EdenFsInstance;
use edenfs_error::{Result, ResultExt};

use crate::ExitCode;

#[derive(StructOpt, Debug)]
#[structopt(about = "Asks RocksDB to compact its storage")]
pub struct CompactLocalStorageCmd {}

#[async_trait]
impl crate::Subcommand for CompactLocalStorageCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let client = instance.connect(None).await?;
        client.debugCompactLocalStorage().await.from_err()?;
        Ok(0)
    }
}
