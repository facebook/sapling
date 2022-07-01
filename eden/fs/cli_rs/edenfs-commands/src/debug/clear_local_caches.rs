/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl config

use async_trait::async_trait;
use clap::Parser;

use edenfs_client::EdenFsInstance;
use edenfs_error::Result;
use edenfs_error::ResultExt;

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Clears local caches of objects stored in RocksDB")]
pub struct ClearLocalCachesCmd {}

#[async_trait]
impl crate::Subcommand for ClearLocalCachesCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let client = instance.connect(None).await?;
        client.debugClearLocalStoreCaches().await.from_err()?;
        Ok(0)
    }
}
