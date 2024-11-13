/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl notify get-position

use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::EdenFsInstance;
use hg_util::path::expand_path;

use crate::ExitCode;

// TODO: add a --json flag to print the output in JSON format
#[derive(Parser, Debug)]
#[clap(about = "Returns the current EdenFS journal position")]
pub struct GetPositionCmd {
    #[clap(parse(from_str = expand_path))]
    /// Path to the mount point
    mount_point: Option<PathBuf>,
}

#[async_trait]
impl crate::Subcommand for GetPositionCmd {
    #[cfg(not(fbcode_build))]
    async fn run(&self) -> Result<ExitCode> {
        eprintln!("not supported in non-fbcode build");
        Ok(1)
    }

    #[cfg(fbcode_build)]
    async fn run(&self) -> Result<ExitCode> {
        let instance = EdenFsInstance::global();
        let position = instance
            .get_journal_position(&self.mount_point, None)
            .await?;
        println!(
            "{}:{}:{}",
            position.mountGeneration,
            position.sequenceNumber,
            hex::encode(position.snapshotHash)
        );
        Ok(0)
    }
}
