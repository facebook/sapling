/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl gc

use std::io::{stderr, Write};

use anyhow::Result;
use structopt::StructOpt;

use edenfs_client::EdenFsInstance;

use crate::ExitCode;

#[derive(StructOpt, Debug)]
#[structopt(about = "Minimize disk and memory usage by freeing caches")]
pub struct GcCmd {}

impl GcCmd {
    pub async fn run(self, instance: EdenFsInstance) -> Result<ExitCode> {
        let client = instance.connect(None).await?;

        // TODO: unload inodes

        eprint!("Clearing and compacting local caches...");
        stderr().flush()?;
        client.clearAndCompactLocalStore().await?;
        eprintln!();

        // TODO: clear kernel caches here

        Ok(0)
    }
}
