/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl gc

use std::io::Write;
use std::io::stderr;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;

use crate::ExitCode;
use crate::get_edenfs_instance;

#[derive(Parser, Debug)]
#[clap(about = "Minimize disk and memory usage by freeing caches")]
pub struct GcCmd {}

#[async_trait]
impl crate::Subcommand for GcCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = get_edenfs_instance();
        let client = instance.get_client();

        // TODO: unload inodes

        eprint!("Clearing and compacting local caches...");
        stderr().flush()?;
        eprintln!();

        // TODO: clear kernel caches here

        Ok(0)
    }
}
