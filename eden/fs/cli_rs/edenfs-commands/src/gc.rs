/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl gc

use std::io::stderr;
use std::io::Write;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::instance::EdenFsInstance;

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Minimize disk and memory usage by freeing caches")]
pub struct GcCmd {}

#[async_trait]
impl crate::Subcommand for GcCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = EdenFsInstance::global();
        let client = instance.get_client(None).await?;

        // TODO: unload inodes

        eprint!("Clearing and compacting local caches...");
        stderr().flush()?;
        client.clear_and_compact_local_store().await?;
        eprintln!();

        // TODO: clear kernel caches here

        Ok(0)
    }
}
