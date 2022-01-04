/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl du

use async_trait::async_trait;
use serde::Serialize;
use std::fs;
use std::fs::{DirEntry, Metadata};
#[cfg(target_os = "linux")]
use std::os::linux::fs::MetadataExt;
#[cfg(target_os = "macos")]
use std::os::unix::fs::MetadataExt;
#[cfg(windows)]
use std::os::windows::fs::MetadataExt;
use std::path::PathBuf;
use structopt::StructOpt;

use anyhow::anyhow;
use edenfs_client::checkout::find_checkout;
use edenfs_client::EdenFsInstance;
use edenfs_error::{EdenFsError, Result, ResultExt};

use crate::ExitCode;

#[derive(StructOpt, Debug)]
#[structopt(about = "Show disk space usage for a checkout")]
pub struct DiskUsageCmd {
    #[structopt(help = "Names of the mount points")]
    mounts: Vec<PathBuf>,

    #[structopt(long, help = "Performs automated cleanup")]
    clean: bool,

    #[structopt(
        long,
        help = "Performs automated cleanup (--clean) and removes fsck dirs. \
        Unlike --clean this will destroy unrecoverable data. If you have any \
        local changes you hope to recover, recover them before you run this command."
    )]
    deep_clean: bool,

    #[structopt(long, help = "Print the output in JSON format")]
    json: bool,
}

#[derive(Serialize)]
struct AggregatedUsageCounts {
    materialized: u64,
    ignored: u64,
    redirection: u64,
    backing: u64,
    shared: u64,
    fsck: u64,
    legacy: u64,
}

impl AggregatedUsageCounts {
    fn new() -> AggregatedUsageCounts {
        AggregatedUsageCounts {
            materialized: 0,
            ignored: 0,
            redirection: 0,
            backing: 0,
            shared: 0,
            fsck: 0,
            legacy: 0,
        }
    }
}

/// Metadata helper methods that map equivalent methods for the
/// purposes of disk usage calculations
trait MetadataDuExt {
    /// Returns the ID of the device containing the file
    fn du_dev(&self) -> u64;

    /// Returns the file size
    fn du_file_size(&self) -> u64;
}

#[cfg(windows)]
impl MetadataDuExt for Metadata {
    fn du_dev(&self) -> u64 {
        0
    }

    fn du_file_size(&self) -> u64 {
        self.file_size()
    }
}

#[cfg(target_os = "linux")]
impl MetadataDuExt for Metadata {
    fn du_dev(&self) -> u64 {
        self.st_dev()
    }

    fn du_file_size(&self) -> u64 {
        // Use st_blocks as this represents the actual amount of
        // disk space allocated by the file, not its apparent
        // size.
        self.st_blocks() * 512
    }
}

#[cfg(target_os = "macos")]
impl MetadataDuExt for Metadata {
    fn du_dev(&self) -> u64 {
        self.dev()
    }

    fn du_file_size(&self) -> u64 {
        self.blocks() * 512
    }
}

/// Intended to only be called by [usage_for_dir]
fn usage_for_dir_entry(
    dirent: std::io::Result<DirEntry>,
    parent_device_id: u64,
) -> std::io::Result<(u64, Vec<PathBuf>)> {
    let entry = dirent?;
    if entry.path().is_dir() {
        // Don't recurse onto different filesystems
        if cfg!(windows) || entry.metadata()?.du_dev() == parent_device_id {
            usage_for_dir(entry.path(), Some(parent_device_id))
        } else {
            Ok((0, vec![]))
        }
    } else {
        let metadata = entry.metadata()?;
        Ok((metadata.du_file_size(), vec![]))
    }
}

fn usage_for_dir(path: PathBuf, device_id: Option<u64>) -> std::io::Result<(u64, Vec<PathBuf>)> {
    let device_id = match device_id {
        Some(device_id) => device_id,
        None => fs::metadata(&path)?.du_dev(),
    };

    let mut total_size = 0;
    let mut failed_to_check_files = Vec::new();
    for dirent in fs::read_dir(&path)? {
        match usage_for_dir_entry(dirent, device_id) {
            Ok((subtotal_size, mut failed_files)) => {
                total_size += subtotal_size;
                failed_to_check_files.append(&mut failed_files);
                Ok(())
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::NotFound
                    || e.kind() == std::io::ErrorKind::PermissionDenied =>
            {
                failed_to_check_files.push(path.clone());
                Ok(())
            }
            Err(e) => Err(e),
        }?;
    }
    Ok((total_size, failed_to_check_files))
}

fn write_title(title: &str) {
    println!("\n{}", title);
    println!("{}", "-".repeat(title.len()));
}

#[async_trait]
impl crate::Subcommand for DiskUsageCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let mut aggregated_usage_counts = AggregatedUsageCounts::new();

        // GET MOUNT INFO
        let mounts = if !self.mounts.is_empty() {
            (&self.mounts).to_vec()
        } else {
            let config_paths: Vec<PathBuf> = instance
                .get_configured_mounts_map()?
                .keys()
                .cloned()
                .collect();
            if config_paths.is_empty() {
                return Err(EdenFsError::Other(anyhow!("No EdenFS mount found")));
            }
            config_paths
        };

        let mut backing_repos = Vec::new();
        for mount in &mounts {
            let checkout = find_checkout(&instance, mount)?;

            // GET BACKING REPO INFO
            if let Some(b) = checkout.backing_repo() {
                backing_repos.push(b);
            }

            // GET SUMMARY INFO for materialized counts
            let overlay_dir = checkout.data_dir().join("local");
            // TODO: print failed_file_checks
            let (usage_count, _failed_file_checks) = usage_for_dir(overlay_dir, None).from_err()?;
            aggregated_usage_counts.materialized += usage_count;
        }
        // Make immutable
        let backing_repos = backing_repos;
        let aggregated_usage_counts = aggregated_usage_counts;

        // PRINT OUTPUT
        if self.json {
            println!(
                "{}",
                serde_json::to_string(&aggregated_usage_counts).from_err()?
            );
        } else {
            write_title("Mounts");
            for path in &mounts {
                println!("{}", path.display());
            }

            write_title("Backing repos");
            for backing in backing_repos {
                println!("{}", backing.display());
            }
            println!(
                "\nCAUTION: You can lose work and break things by manually deleting data \
                from the backing repo directory!"
            );
        }
        Ok(0)
    }
}
