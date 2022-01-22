/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl du

use async_trait::async_trait;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::fs::DirEntry;
use std::path::PathBuf;
use structopt::StructOpt;

use anyhow::anyhow;
use edenfs_client::checkout::{find_checkout, EdenFsCheckout};
use edenfs_client::redirect::get_effective_redirections;
use edenfs_client::{EdenFsClient, EdenFsInstance};
use edenfs_error::{EdenFsError, Result, ResultExt};
use edenfs_utils::metadata::MetadataExt;
use edenfs_utils::{bytes_from_path, path_from_bytes};

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

/// Intended to only be called by [usage_for_dir]
fn usage_for_dir_entry(
    dirent: std::io::Result<DirEntry>,
    parent_device_id: u64,
) -> std::io::Result<(u64, Vec<PathBuf>)> {
    let entry = dirent?;
    let symlink_metadata = fs::symlink_metadata(entry.path())?;
    if symlink_metadata.is_dir() {
        // Don't recurse onto different filesystems
        if cfg!(windows) || symlink_metadata.eden_dev() == parent_device_id {
            usage_for_dir(entry.path(), Some(parent_device_id))
        } else {
            Ok((0, vec![]))
        }
    } else {
        Ok((symlink_metadata.eden_file_size(), vec![]))
    }
}

fn usage_for_dir(path: PathBuf, device_id: Option<u64>) -> std::io::Result<(u64, Vec<PathBuf>)> {
    let device_id = match device_id {
        Some(device_id) => device_id,
        None => fs::metadata(&path)?.eden_dev(),
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

async fn ignored_usage_counts_for_mount(
    checkout: &EdenFsCheckout,
    client: &EdenFsClient,
) -> Result<u64> {
    let scm_status = client
        .getScmStatus(
            &bytes_from_path(checkout.path())?,
            true,
            &checkout.get_snapshot()?.as_bytes().to_vec(),
        )
        .await
        .from_err()?;

    let mut aggregated_usage_counts_ignored = 0;
    for (rel_path, _file_status) in scm_status.entries {
        let path = checkout.path().join(path_from_bytes(&rel_path)?);
        aggregated_usage_counts_ignored += match fs::symlink_metadata(path) {
            Ok(metadata) => Ok(metadata.len()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // Status can show files that were present in the overlay
                // before a redirection was mounted over the top of it,
                // which makes them inaccessible here.  Alternatively,
                // someone may have raced with us and removed the file
                // between the status call and our attempt to stat it.
                // Just absorb the error here and ignore it.
                Ok(0)
            }
            Err(e) => Err(e),
        }
        .from_err()?;
    }
    Ok(aggregated_usage_counts_ignored)
}

fn write_title(title: &str) {
    println!("\n{}", title);
    println!("{}", "-".repeat(title.len()));
}

#[async_trait]
impl crate::Subcommand for DiskUsageCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let client = instance.connect(None).await?;

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

        let mut aggregated_usage_counts = AggregatedUsageCounts::new();
        let mut backing_repos = Vec::new();
        let mut redirections = HashSet::new();
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

            // GET SUMMARY INFO for ignored counts
            aggregated_usage_counts.ignored +=
                ignored_usage_counts_for_mount(&checkout, &client).await?;

            // GET SUMMARY INFO for fsck
            let fsck_dir = checkout.data_dir().join("fsck");
            if fsck_dir.exists() {
                let (usage_count, _failed_file_checks) =
                    usage_for_dir(fsck_dir, None).from_err()?;
                aggregated_usage_counts.fsck += usage_count;
            }

            for (_, redir) in get_effective_redirections(&checkout)? {
                // GET SUMMARY INFO for redirections
                if let Some(target) = redir.expand_target_abspath(&checkout)? {
                    let (usage_count, _failed_file_checks) =
                        usage_for_dir(target, None).from_err()?;
                    aggregated_usage_counts.redirection += usage_count;
                } else {
                    return Err(EdenFsError::Other(anyhow!(
                        "Cannot resolve target for redirection: {:?}",
                        redir
                    )));
                }

                // GET REDIRECTIONS LIST
                let repo_path = redir.repo_path();
                if let Some(file_name) = repo_path.file_name() {
                    if file_name == "buck-out" {
                        let redir_full_path = checkout.path().join(repo_path);
                        redirections.insert(redir_full_path);
                    }
                }
            }
        }
        // Make immutable
        let aggregated_usage_counts = aggregated_usage_counts;
        let backing_repos = backing_repos;
        let redirections = redirections;

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

            write_title("Redirections");
            if redirections.is_empty() {
                println!("No redirections");
            } else {
                for redir in redirections {
                    println!("{}", redir.display());
                }

                if !self.clean && !self.deep_clean {
                    println!(
                        "\nTo reclaim space from buck-out directories, run `buck clean` from the \
                        parent of the buck-out directory."
                    )
                }
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
