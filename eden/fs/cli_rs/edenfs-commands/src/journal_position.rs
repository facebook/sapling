/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl notify

use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::EdenFsInstance;
use edenfs_utils::bytes_from_path;

use crate::util::locate_repo_root;
use crate::ExitCode;

// TODO: add a --json flag to print the output in JSON format
// TODO: add a --mount_point flag to specify which mount to query
#[derive(Parser, Debug)]
#[clap(about = "Returns the current EdenFS journal position")]
pub struct JournalPositionCmd {}

impl JournalPositionCmd {
    //TODO: refactor subscribe::get_mount_point into a separate crate
    pub fn get_mount_point(&self) -> Result<PathBuf> {
        locate_repo_root(
            &std::env::current_dir().context("Unable to retrieve current working directory")?,
        )
        .map(|p| p.to_path_buf())
        .ok_or_else(|| anyhow!("Unable to locate repository root"))
    }
}

#[async_trait]
impl crate::Subcommand for JournalPositionCmd {
    async fn run(&self) -> Result<ExitCode> {
        let instance = EdenFsInstance::global();
        let client = instance.connect(None).await?;
        let mount_point_path = self.get_mount_point()?;
        let mount_point = bytes_from_path(mount_point_path)?;
        let position = client.getCurrentJournalPosition(&mount_point).await?;
        println!(
            "{}:{}:{}",
            position.mountGeneration,
            position.sequenceNumber,
            hex::encode(position.snapshotHash)
        );
        Ok(0)
    }
}
