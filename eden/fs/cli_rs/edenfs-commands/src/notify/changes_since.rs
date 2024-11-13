/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl notify changes-since

use std::path::PathBuf;
use std::time::Duration;

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::EdenFsInstance;
use edenfs_error::EdenFsError;
use futures::stream::StreamExt;
use hg_util::path::expand_path;
use thrift_types::edenfs::JournalPosition;
use tokio::time;

use crate::ExitCode;

/// Parse journal position string into a JournalPosition.
/// Format: "<mount-generation>:<sequence-number>:<hexified-snapshot-hash>"
fn parse_journal_position(position: &str) -> Result<JournalPosition> {
    let parts = position.split(':').collect::<Vec<&str>>();
    if parts.len() != 3 {
        return Err(anyhow!(format!(
            "Invalid journal position format: {}",
            position
        )));
    }

    let mount_generation = parts[0].parse::<i64>()?;
    let sequence_number = parts[1].parse::<i64>()?;
    let snapshot_hash = hex::decode(parts[2])?;
    Ok(JournalPosition {
        mountGeneration: mount_generation,
        sequenceNumber: sequence_number,
        snapshotHash: snapshot_hash,
        ..Default::default()
    })
}

// TODO: add a --timeout flag
// TODO: add a --json flag to print the output in JSON format
#[derive(Parser, Debug)]
#[clap(about = "Returns the changes since the given EdenFS journal position")]
pub struct ChangesSinceCmd {
    #[clap(long, short = 'p', allow_hyphen_values = true, parse(try_from_str = parse_journal_position))]
    /// Journal position to start from
    position: JournalPosition,

    #[clap(parse(from_str = expand_path))]
    /// Path to the mount point
    mount_point: Option<PathBuf>,
}

impl ChangesSinceCmd {
    #[cfg(fbcode_build)]
    async fn get_changes_since(
        &self,
        instance: &EdenFsInstance,
    ) -> edenfs_error::Result<JournalPosition> {
        let result_and_stream = instance.get_changes_since(&self.mount_point, &self.position, None);
        // TODO: add support for timeout (see `status::get_status_blocking_on_startup`)
        let result_and_stream = time::timeout(Duration::MAX, result_and_stream)
            .await
            .map_err(edenfs_error::EdenFsError::RequestTimeout)?;

        match result_and_stream {
            Ok((result, mut stream)) => {
                while let Some(value) = stream.next().await {
                    match value {
                        Ok(change) => {
                            println!("{:?}", change);
                        }
                        Err(e) => {
                            println!("Error received from EdenFS while starting: {}", e);
                            break;
                        }
                    }
                }
                Ok(result.toPosition)
            }
            Err(EdenFsError::Other(e)) => Err(EdenFsError::Other(e)),
            Err(e) => Err(e),
        }
    }
}

#[async_trait]
impl crate::Subcommand for ChangesSinceCmd {
    #[cfg(not(fbcode_build))]
    async fn run(&self) -> Result<ExitCode> {
        eprintln!("not supported in non-fbcode build");
        Ok(1)
    }

    #[cfg(fbcode_build)]
    async fn run(&self) -> Result<ExitCode> {
        let instance = EdenFsInstance::global();
        let postition = self.get_changes_since(instance).await?;
        println!(
            "{}:{}:{}",
            postition.mountGeneration,
            postition.sequenceNumber,
            hex::encode(postition.snapshotHash)
        );
        Ok(0)
    }
}
