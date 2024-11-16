/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl notify changes-since

use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::ChangeNotification;
use edenfs_client::EdenFsInstance;
use edenfs_client::LargeChangeNotification;
use edenfs_client::SmallChangeNotification;
use edenfs_utils::path_from_bytes;
use hg_util::path::expand_path;
use thrift_types::edenfs::JournalPosition;

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
    fn display_small_change_notifcation(
        &self,
        small_change_notification: &SmallChangeNotification,
    ) {
        print!("small: ");
        match small_change_notification {
            SmallChangeNotification::added(added) => println!(
                "added ({}): '{}'",
                added.fileType,
                path_from_bytes(&added.path)
                    .expect("Invalid path.")
                    .to_string_lossy()
            ),
            SmallChangeNotification::modified(modified) => println!(
                "modified ({}): '{}'",
                modified.fileType,
                path_from_bytes(&modified.path)
                    .expect("Invalid path.")
                    .to_string_lossy()
            ),
            SmallChangeNotification::renamed(renamed) => println!(
                "renamed ({}): '{}' -> '{}'",
                renamed.fileType,
                path_from_bytes(&renamed.from)
                    .expect("Invalid path.")
                    .to_string_lossy(),
                path_from_bytes(&renamed.to)
                    .expect("Invalid path.")
                    .to_string_lossy()
            ),
            SmallChangeNotification::replaced(replaced) => println!(
                "replaced ({}): '{}' -> '{}'",
                replaced.fileType,
                path_from_bytes(&replaced.from)
                    .expect("Invalid path.")
                    .to_string_lossy(),
                path_from_bytes(&replaced.to)
                    .expect("Invalid path.")
                    .to_string_lossy()
            ),
            SmallChangeNotification::removed(removed) => println!(
                "removed ({}): '{}'",
                removed.fileType,
                path_from_bytes(&removed.path)
                    .expect("Invalid path.")
                    .to_string_lossy()
            ),
            _ => println!("unknown: {:?}", small_change_notification),
        }
    }

    fn display_large_change_notifcation(
        &self,
        large_change_notification: &LargeChangeNotification,
    ) {
        print!("large: ");
        match large_change_notification {
            LargeChangeNotification::commitTransition(commit_transition) => {
                println!(
                    "commit transition: '{}' -> '{}'",
                    hex::encode(&commit_transition.from),
                    hex::encode(&commit_transition.to)
                )
            }
            LargeChangeNotification::directoryRenamed(directory_renamed) => println!(
                "directory renamed: '{}' -> '{}'",
                path_from_bytes(&directory_renamed.from)
                    .expect("Invalid path.")
                    .to_string_lossy(),
                path_from_bytes(&directory_renamed.to)
                    .expect("Invalid path.")
                    .to_string_lossy(),
            ),
            _ => println!("unknonwn: {:?}", large_change_notification),
        }
    }

    fn display_change_notifcation(&self, change_notification: &ChangeNotification) {
        match change_notification {
            ChangeNotification::smallChange(small_change) => {
                self.display_small_change_notifcation(small_change)
            }
            ChangeNotification::largeChange(large_change) => {
                self.display_large_change_notifcation(large_change)
            }
            _ => {
                println!("unknonwn: {:?}", change_notification);
            }
        }
    }

    #[cfg(fbcode_build)]
    async fn get_changes_since(
        &self,
        instance: &EdenFsInstance,
    ) -> edenfs_error::Result<JournalPosition> {
        // TODO: add support for timeout (see `status::get_status_blocking_on_startup`)
        let result = instance
            .get_changes_since(&self.mount_point, &self.position, None)
            .await?;
        result.changes.iter().for_each(|change| {
            self.display_change_notifcation(change);
        });
        Ok(result.toPosition)
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
            "position: {}:{}:{}",
            postition.mountGeneration,
            postition.sequenceNumber,
            hex::encode(postition.snapshotHash)
        );
        Ok(0)
    }
}
