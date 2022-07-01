/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl du

use anyhow::anyhow;
use async_trait::async_trait;
use clap::Parser;
use colored::Colorize;
use comfy_table::Cell;
use comfy_table::CellAlignment;
use comfy_table::Color;
use comfy_table::Row;
use comfy_table::Table;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::fs::DirEntry;
use std::path::Path;
use std::path::PathBuf;
use subprocess::Exec;
use subprocess::Redirection;

use edenfs_client::checkout::find_checkout;
use edenfs_client::checkout::EdenFsCheckout;
use edenfs_client::redirect::get_effective_redirections;
use edenfs_client::EdenFsClient;
use edenfs_client::EdenFsInstance;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use edenfs_utils::bytes_from_path;
use edenfs_utils::get_buck_command;
use edenfs_utils::get_env_with_buck_version;
use edenfs_utils::get_environment_suitable_for_subprocess;
use edenfs_utils::metadata::MetadataExt;
use edenfs_utils::path_from_bytes;

use crate::ExitCode;

#[derive(Parser, Debug)]
#[clap(about = "Show disk space usage for a checkout")]
pub struct DiskUsageCmd {
    #[clap(help = "Names of the mount points")]
    mounts: Vec<PathBuf>,

    #[clap(
        long,
        help = "Performs automated cleanup",
        conflicts_with = "deep-clean",
        conflicts_with = "json"
    )]
    clean: bool,

    #[clap(
        long,
        help = "Performs automated cleanup (--clean) and removes fsck dirs. \
        Unlike --clean this will destroy unrecoverable data. If you have any \
        local changes you hope to recover, recover them before you run this command.",
        conflicts_with = "json"
    )]
    deep_clean: bool,

    #[clap(long, help = "Print the output in JSON format")]
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
        }
    }
}

fn format_size(size: u64) -> String {
    if size > 1000000000 {
        format!("{:.1} GB", size as f64 / 1000000000.0)
    } else if size > 1000000 {
        format!("{:.1} MB", size as f64 / 1000000.0)
    } else if size > 1000 {
        format!("{:.1} KB", size as f64 / 1000.0)
    } else if size > 0 {
        format!("{} B", size)
    } else {
        "0".to_string()
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
            usage_for_dir(&entry.path(), Some(parent_device_id))
        } else {
            Ok((0, vec![]))
        }
    } else {
        Ok((symlink_metadata.eden_file_size(), vec![]))
    }
}

fn usage_for_dir(path: &Path, device_id: Option<u64>) -> std::io::Result<(u64, Vec<PathBuf>)> {
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
                failed_to_check_files.push(path.to_path_buf());
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
            &checkout
                .get_snapshot()?
                .working_copy_parent
                .as_bytes()
                .to_vec(),
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

fn get_hg_cache_path() -> Result<PathBuf> {
    let output = Exec::cmd("hg")
        .args(&["config", "remotefilelog.cachepath"])
        .stdout(Redirection::Pipe)
        .stderr(Redirection::Pipe)
        .env_clear()
        .env_extend(&get_environment_suitable_for_subprocess())
        .capture()
        .from_err()?;

    if output.success() {
        let raw_path = output.stdout_str();
        let raw_path = raw_path.trim();
        assert!(!raw_path.is_empty());
        Ok(PathBuf::from(raw_path))
    } else {
        Err(EdenFsError::Other(anyhow!(
            "Failed to execute `hg config remotefilelog.cachepath`, stderr: {}, exit status: {:?}",
            output.stderr_str(),
            output.exit_status,
        )))
    }
}

fn write_title(title: &str) {
    println!("\n{}", title);
    println!("{}", "-".repeat(title.len()));
}

fn write_failed_to_check_files_message(file_paths: &HashSet<PathBuf>) {
    if !file_paths.is_empty() {
        println!(
            "\n{}",
            "Warning: failed to check paths due to file not found or permission errors:".yellow()
        );
        for f in file_paths {
            println!("{}", format!("{}", f.display()).yellow());
        }
        println!(
            "\n{}",
            "Note that we also cannot clean these paths.".yellow()
        );
    }
}

impl DiskUsageCmd {
    fn should_clean(&self) -> bool {
        self.clean || self.deep_clean
    }
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
        let mut backing_failed_file_checks = HashSet::new();
        let mut mount_failed_file_checks = HashSet::new();
        let mut redirection_failed_file_checks = HashSet::new();
        let mut backing_repos = HashSet::new();
        let mut backed_working_copy_repos = HashSet::new();
        let mut redirections = HashSet::new();
        let mut fsck_dirs = Vec::new();
        for mount in &mounts {
            let checkout = find_checkout(&instance, mount)?;

            if let Some(b) = checkout.backing_repo() {
                // GET SUMMARY INFO for backing counts
                let (usage_count, failed_file_checks) = usage_for_dir(&b, None).from_err()?;
                aggregated_usage_counts.backing += usage_count;
                backing_failed_file_checks.extend(failed_file_checks);

                // GET BACKING REPO INFO
                backing_repos.insert(b.clone());

                // GET BACKED WORKING COPY REPOS
                // if the backing repo folder contains ".hg" and
                // has more than just the .hg directory inside it,
                // then it is a backed working copy repo
                if b.join(".hg").is_dir() && fs::read_dir(&b).from_err()?.count() > 1 {
                    backed_working_copy_repos.insert(b);
                }
            }

            // GET SUMMARY INFO for materialized counts
            let overlay_dir = checkout.data_dir().join("local");
            let (usage_count, failed_file_checks) = usage_for_dir(&overlay_dir, None).from_err()?;
            aggregated_usage_counts.materialized += usage_count;
            mount_failed_file_checks.extend(failed_file_checks);

            // GET SUMMARY INFO for ignored counts
            aggregated_usage_counts.ignored +=
                ignored_usage_counts_for_mount(&checkout, &client).await?;

            // GET SUMMARY INFO for fsck
            let fsck_dir = checkout.fsck_dir();
            if fsck_dir.exists() {
                let (usage_count, failed_file_checks) =
                    usage_for_dir(&fsck_dir, None).from_err()?;
                aggregated_usage_counts.fsck += usage_count;
                mount_failed_file_checks.extend(failed_file_checks);
                fsck_dirs.push(fsck_dir);
            }

            for (_, redir) in get_effective_redirections(&checkout)? {
                // GET SUMMARY INFO for redirections
                if let Some(target) = redir.expand_target_abspath(&checkout)? {
                    let (usage_count, failed_file_checks) =
                        usage_for_dir(&target, None).from_err()?;
                    aggregated_usage_counts.redirection += usage_count;
                    redirection_failed_file_checks.extend(failed_file_checks);
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
        let backing_failed_file_checks = backing_failed_file_checks;
        let mount_failed_file_checks = mount_failed_file_checks;
        let redirection_failed_file_checks = redirection_failed_file_checks;
        let backing_repos = backing_repos;
        let backed_working_copy_repos = backed_working_copy_repos;
        let redirections = redirections;

        // GET SUMMARY INFO for shared usage
        let mut shared_failed_file_checks = HashSet::new();
        let (logs_dir_usage, failed_logs_dir_file_checks) =
            usage_for_dir(&instance.logs_dir(), None).from_err()?;
        aggregated_usage_counts.shared += logs_dir_usage;
        shared_failed_file_checks.extend(failed_logs_dir_file_checks);
        let (storage_dir_usage, failed_storage_dir_file_checks) =
            usage_for_dir(&instance.storage_dir(), None).from_err()?;
        aggregated_usage_counts.shared += storage_dir_usage;
        shared_failed_file_checks.extend(failed_storage_dir_file_checks);

        // Make immutable
        let shared_failed_file_checks = shared_failed_file_checks;
        let aggregated_usage_counts = aggregated_usage_counts;

        // GET HGCACHE PATH
        let hg_cache_path = get_hg_cache_path()?;

        // PRINT OUTPUT
        if self.json {
            println!(
                "{}",
                serde_json::to_string(&aggregated_usage_counts).from_err()?
            );
        } else {
            if self.should_clean() {
                println!(
                    "{}",
                    "WARNING: --clean/--deep-clean options don't remove ignored files. \
                    Materialized files will be de-materialized once committed. \
                    Use `hg status -i` to see Ignored files, `hg clean --all` \
                    to remove them but be careful: it will remove untracked files as well! \
                    It is best to use `eden redirect` or the `mkscratch` utility to relocate \
                    files outside the repo rather than to ignore and clean them up."
                        .yellow()
                );
            }

            // PRINT MOUNTS
            write_title("Mounts");
            for path in &mounts {
                println!("{}", path.display());
            }
            write_failed_to_check_files_message(&mount_failed_file_checks);

            // CLEAN MOUNTS
            if self.should_clean() {
                if self.deep_clean {
                    println!();
                    for dir in &fsck_dirs {
                        println!(
                            "\n{}",
                            format!("Reclaiming space from directory: {}", dir.display()).blue()
                        );
                        match fs::remove_dir_all(&dir) {
                            Ok(_) => println!("{}", "Space reclaimed. Directory removed.".blue()),
                            Err(e) => println!(
                                "{}",
                                format!("Failed to remove {} : {:?}", dir.display(), e).yellow()
                            ),
                        };
                    }
                } else if self.clean {
                    let fsck_dir_strings: Vec<String> = fsck_dirs
                        .iter()
                        .map(|path| path.display().to_string())
                        .collect();

                    if !fsck_dir_strings.is_empty() {
                        println!(
                            "\n{}",
                            format!(
                                "A filesytem check recovered data and stored it at:
- {}

If you have recovered all that you need from these locations, you can remove that directory to reclaim the disk space.

To automatically remove this directory, run `eden du --deep-clean`.",
                                fsck_dir_strings.join("\n- ")
                            )
                            .blue()
                        )
                    }
                }
            }

            // PRINT REDIRECTIONS
            write_title("Redirections");
            if redirections.is_empty() {
                println!("No redirections");
            } else {
                for redir in &redirections {
                    println!("{}", redir.display());
                }
            }
            write_failed_to_check_files_message(&redirection_failed_file_checks);

            // CLEAN REDIRECTIONS
            if !redirections.is_empty() {
                if self.should_clean() {
                    for redir in redirections {
                        println!(
                            "\n{}",
                            format!("Reclaiming space from redirection: {}", redir.display())
                                .blue()
                        );
                        if let Some(basename) = redir.parent() {
                            let output = Exec::cmd(get_buck_command())
                                .arg("clean")
                                .stderr(Redirection::Pipe)
                                .cwd(basename)
                                .env_clear()
                                .env_extend(&get_env_with_buck_version(basename)?)
                                .capture()
                                .from_err()?;

                            if output.success() {
                                println!("{}", "Space reclaimed".blue());
                            } else {
                                return Err(EdenFsError::Other(anyhow!(
                                    "Failed to execute buck clean from {}, stderr: {}, exit status: {:?}",
                                    basename.display(),
                                    output.stderr_str(),
                                    output.exit_status,
                                )));
                            }
                        } else {
                            return Err(EdenFsError::Other(anyhow!(
                                "Found invalid redirection: {}",
                                redir.display()
                            )));
                        };
                    }
                } else {
                    println!(
                        "\nTo reclaim space from buck-out directories, run `buck clean` from the \
                        parent of the buck-out directory."
                    );
                }
            }

            // PRINT BACKING REPOS
            if !backing_repos.is_empty() || !backing_failed_file_checks.is_empty() {
                write_title("Backing repos");
            }
            if !backing_repos.is_empty() {
                for backing in backing_repos {
                    println!("{}", backing.display());
                }
                println!(
                    "\n{}",
                    "CAUTION: You can lose work and break things by manually deleting data \
                    from the backing repo directory!"
                        .yellow()
                );
            }
            write_failed_to_check_files_message(&backing_failed_file_checks);

            println!("\nTo reclaim space from the hgcache directory, run:");
            if cfg!(windows) {
                println!("\n`rmdir {}`", hg_cache_path.display());
            } else {
                println!("\n`rm -rf {}/*`", hg_cache_path.display());
            }
            println!(
                "\nNOTE: The hgcache should manage its size itself. You should only run the command \
                above if you are completely out of space and quickly need to reclaim some space \
                temporarily. This will affect other users if you run this command on a shared machine."
            );

            if !backed_working_copy_repos.is_empty() {
                println!(
                    "\nWorking copy detected in backing repo.  This is not generally useful \
                    and just takes up space.  You can make this a bare repo to reclaim \
                    space by running:\n"
                );
                for backed_working_copy in backed_working_copy_repos {
                    println!("hg -R {} checkout null", backed_working_copy.display());
                }
            }

            // PRINT SHARED SPACE
            write_title("Shared space");
            if self.should_clean() {
                println!(
                    "{}",
                    "Cleaning shared space used by the storage engine...".blue()
                );
                let output = Exec::cmd("eden")
                    .arg("gc")
                    .stdout(Redirection::Pipe)
                    .stderr(Redirection::Pipe)
                    .capture()
                    .from_err()?;

                if output.success() {
                    println!("{}", "Finished cleaning shared space.".blue())
                } else {
                    return Err(EdenFsError::Other(anyhow!(
                        "Failed to execute `eden gc`, stderr: {}, exit status: {:?}",
                        output.stderr_str(),
                        output.exit_status,
                    )));
                }
            } else {
                println!("Run `eden gc` to reduce the space used by the storage engine.");
            }
            write_failed_to_check_files_message(&shared_failed_file_checks);

            // PRINT SUMMARY
            write_title("Summary");
            let mut table = Table::new();
            table.load_preset(comfy_table::presets::NOTHING);

            if aggregated_usage_counts.materialized > 0 {
                let mut row = Row::new();
                row.add_cell(Cell::new("Materialized files:").set_alignment(CellAlignment::Right));
                row.add_cell(Cell::new(format_size(aggregated_usage_counts.materialized)));
                if self.should_clean() {
                    row.add_cell(
                        Cell::new("Not cleaned. Please see WARNING above").fg(Color::Yellow),
                    );
                }
                table.add_row(row);
            }
            if aggregated_usage_counts.redirection > 0 {
                let mut row = Row::new();
                row.add_cell(Cell::new("Redirections:").set_alignment(CellAlignment::Right));
                row.add_cell(Cell::new(format_size(aggregated_usage_counts.redirection)));
                if self.should_clean() {
                    row.add_cell(Cell::new("Cleaned").fg(Color::Green));
                }
                table.add_row(row);
            }
            if aggregated_usage_counts.ignored > 0 {
                let mut row = Row::new();
                row.add_cell(Cell::new("Ignored files:").set_alignment(CellAlignment::Right));
                row.add_cell(Cell::new(format_size(aggregated_usage_counts.ignored)));
                if self.should_clean() {
                    row.add_cell(
                        Cell::new("Not cleaned. Please see WARNING above").fg(Color::Yellow),
                    );
                }
                table.add_row(row);
            }
            if aggregated_usage_counts.backing > 0 {
                let mut row = Row::new();
                row.add_cell(Cell::new("Backing repos:").set_alignment(CellAlignment::Right));
                row.add_cell(Cell::new(format_size(aggregated_usage_counts.backing)));
                if self.should_clean() {
                    row.add_cell(
                        Cell::new("Not cleaned. Please see CAUTION above").fg(Color::Yellow),
                    );
                }
                table.add_row(row);
            }
            if aggregated_usage_counts.shared > 0 {
                let mut row = Row::new();
                row.add_cell(Cell::new("Shared space:").set_alignment(CellAlignment::Right));
                row.add_cell(Cell::new(format_size(aggregated_usage_counts.shared)));
                if self.should_clean() {
                    row.add_cell(Cell::new("Cleaned").fg(Color::Green));
                }
                table.add_row(row);
            }
            if aggregated_usage_counts.fsck > 0 {
                let mut row = Row::new();
                row.add_cell(
                    Cell::new("Filesystem Check recovered files:")
                        .set_alignment(CellAlignment::Right),
                );
                row.add_cell(Cell::new(format_size(aggregated_usage_counts.fsck)));
                if self.deep_clean {
                    row.add_cell(Cell::new("Cleaned").fg(Color::Green));
                } else if self.clean {
                    row.add_cell(
                        Cell::new(
                            "Not cleaned. Directories listed above. Check and remove manually",
                        )
                        .fg(Color::Yellow),
                    );
                }
                table.add_row(row);
            }

            println!("{}", table.to_string());

            if !self.should_clean() {
                println!(
                    "{}",
                    "\nTo perform automated cleanup, run `eden du --clean`".blue()
                );
            }
        }
        Ok(0)
    }
}
