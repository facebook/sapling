/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl du

use std::collections::BTreeSet;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::fmt;
use std::fs;
use std::fs::DirEntry;
use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use colored::Colorize;
use comfy_table::Cell;
use comfy_table::CellAlignment;
use comfy_table::Color;
use comfy_table::Row;
use comfy_table::Table;
use edenfs_client::checkout::find_checkout;
use edenfs_client::checkout::EdenFsCheckout;
use edenfs_client::redirect::get_effective_redirections;
use edenfs_client::redirect::Redirection;
use edenfs_client::EdenFsClient;
use edenfs_client::EdenFsInstance;
use edenfs_utils::bytes_from_path;
use edenfs_utils::get_buck_command;
use edenfs_utils::get_env_with_buck_version;
use edenfs_utils::get_environment_suitable_for_subprocess;
use edenfs_utils::metadata::MetadataExt;
use edenfs_utils::path_from_bytes;
use serde::Serialize;
use subprocess::Exec;
use subprocess::Redirection as SubprocessRedirection;

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
    orphaned_redirections: u64,
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
            orphaned_redirections: 0,
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

impl fmt::Display for AggregatedUsageCounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut table = Table::new();
        table.load_preset(comfy_table::presets::NOTHING);

        if self.materialized > 0 {
            let mut row = Row::new();
            row.add_cell(Cell::new("Materialized files:").set_alignment(CellAlignment::Right));
            row.add_cell(Cell::new(format_size(self.materialized)));
            if f.alternate() {
                row.add_cell(Cell::new("Not cleaned. Please see WARNING above").fg(Color::Yellow));
            }
            table.add_row(row);
        }
        if self.redirection > 0 {
            let mut row = Row::new();
            row.add_cell(Cell::new("Redirections:").set_alignment(CellAlignment::Right));
            row.add_cell(Cell::new(format_size(self.redirection)));
            if f.alternate() {
                row.add_cell(Cell::new("Cleaned").fg(Color::Green));
            }
            table.add_row(row);
        }
        if self.orphaned_redirections > 0 {
            let mut row = Row::new();
            row.add_cell(Cell::new("Orphaned redirections:").set_alignment(CellAlignment::Right));
            row.add_cell(Cell::new(format_size(self.orphaned_redirections)));
            if f.alternate() {
                row.add_cell(Cell::new("Cleaned").fg(Color::Green));
            }
            table.add_row(row);
        }
        if self.ignored > 0 {
            let mut row = Row::new();
            row.add_cell(Cell::new("Ignored files:").set_alignment(CellAlignment::Right));
            row.add_cell(Cell::new(format_size(self.ignored)));
            if f.alternate() {
                row.add_cell(Cell::new("Not cleaned. Please see WARNING above").fg(Color::Yellow));
            }
            table.add_row(row);
        }
        if self.backing > 0 {
            let mut row = Row::new();
            row.add_cell(Cell::new("Backing repos:").set_alignment(CellAlignment::Right));
            row.add_cell(Cell::new(format_size(self.backing)));
            if f.alternate() {
                row.add_cell(Cell::new("Not cleaned. Please see CAUTION above").fg(Color::Yellow));
            }
            table.add_row(row);
        }
        if self.shared > 0 {
            let mut row = Row::new();
            row.add_cell(Cell::new("Shared space:").set_alignment(CellAlignment::Right));
            row.add_cell(Cell::new(format_size(self.shared)));
            if f.alternate() {
                row.add_cell(Cell::new("Cleaned").fg(Color::Green));
            }
            table.add_row(row);
        }
        if self.fsck > 0 {
            let mut row = Row::new();
            row.add_cell(
                Cell::new("Filesystem Check recovered files:").set_alignment(CellAlignment::Right),
            );
            row.add_cell(Cell::new(format_size(self.fsck)));
            if f.alternate() {
                if f.sign_plus() {
                    row.add_cell(Cell::new("Cleaned").fg(Color::Green));
                } else {
                    row.add_cell(
                        Cell::new(
                            "Not cleaned. Directories listed above. Check and remove manually",
                        )
                        .fg(Color::Yellow),
                    );
                }
            }
            table.add_row(row);
        }

        write!(f, "{}", table)
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

fn ignored_io_error(error: &std::io::Error) -> bool {
    error.kind() == std::io::ErrorKind::NotFound
        || error.kind() == std::io::ErrorKind::PermissionDenied
}

fn usage_for_dir(path: &Path, device_id: Option<u64>) -> std::io::Result<(u64, Vec<PathBuf>)> {
    let device_id = match device_id {
        Some(device_id) => device_id,
        None => match fs::metadata(&path) {
            Ok(metadata) => metadata.eden_dev(),
            Err(e) if ignored_io_error(&e) => return Ok((0, vec![path.to_path_buf()])),
            Err(e) => return Err(e),
        },
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
            Err(e) if ignored_io_error(&e) => {
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
        .await?;

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
        }?;
    }
    Ok(aggregated_usage_counts_ignored)
}

fn get_hg_cache_path() -> Result<PathBuf> {
    let output = Exec::cmd("hg")
        .args(&["config", "remotefilelog.cachepath"])
        .stdout(SubprocessRedirection::Pipe)
        .stderr(SubprocessRedirection::Pipe)
        .env_clear()
        .env_extend(&get_environment_suitable_for_subprocess())
        .capture()?;

    if output.success() {
        let raw_path = output.stdout_str();
        let raw_path = raw_path.trim();
        assert!(!raw_path.is_empty());
        Ok(PathBuf::from(raw_path))
    } else {
        Err(anyhow!(
            "Failed to execute `hg config remotefilelog.cachepath`, stderr: {}, exit status: {:?}",
            output.stderr_str(),
            output.exit_status,
        ))
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

    /// Get all the EdenFS mount that `du` should run on.
    ///
    /// This is either the mounts passed as an argument, or all the mounts known to the EdenFS
    /// instance.
    fn get_mounts(&self, instance: &EdenFsInstance) -> Result<Vec<PathBuf>> {
        if !self.mounts.is_empty() {
            Ok((&self.mounts).to_vec())
        } else {
            let config_paths: Vec<PathBuf> = instance
                .get_configured_mounts_map()?
                .keys()
                .cloned()
                .collect();
            if config_paths.is_empty() {
                return Err(anyhow!("No EdenFS mount found"));
            }
            Ok(config_paths)
        }
    }

    /// Remove all the fsck directories if --deep-clean is used.
    fn clean_fsck_directories(&self, fsck_dirs: Vec<PathBuf>) {
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
}

/// Get all the backing repositories associated with the passed in checkouts.
fn get_backing_repos(checkouts: &[EdenFsCheckout]) -> HashSet<PathBuf> {
    checkouts
        .iter()
        .filter_map(|checkout| checkout.backing_repo())
        .collect()
}

/// Get the target folder for all the redirections.
///
/// This returns 2 sets: the target of all redirections, and all the Buck redirections.
fn get_redirections(checkouts: &[EdenFsCheckout]) -> Result<(BTreeSet<PathBuf>, HashSet<PathBuf>)> {
    let mut redirections = BTreeSet::new();
    let mut buck_redirections = HashSet::new();

    for checkout in checkouts.iter() {
        for (_, redir) in get_effective_redirections(checkout).with_context(|| {
            format!(
                "Failed to get redirections for {}",
                checkout.path().display()
            )
        })? {
            if let Some(target) = redir.expand_target_abspath(checkout).with_context(|| {
                format!(
                    "Failed to get redirection destination for {}",
                    redir.repo_path.display()
                )
            })? {
                redirections.insert(target);
            }

            let repo_path = redir.repo_path();
            if let Some(file_name) = repo_path.file_name() {
                if file_name == "buck-out" {
                    buck_redirections.insert(checkout.path().join(repo_path));
                }
            }
        }
    }
    Ok((redirections, buck_redirections))
}

/// Get all the checkous associated with the passed in mounts.
fn get_checkouts(mounts: &[PathBuf], instance: &EdenFsInstance) -> Result<Vec<EdenFsCheckout>> {
    Ok(mounts
        .iter()
        .map(|mount| {
            find_checkout(instance, mount)
                .with_context(|| format!("Failed to find checkout for {}", mount.display()))
        })
        .collect::<Result<_, anyhow::Error>>()?)
}

/// Get all the fsck directories for the pssed in checkouts.
///
/// Some checkouts do not have a fsck directory, the returned Vec will not included them.
fn get_fsck_dirs(checkouts: &[EdenFsCheckout]) -> Vec<PathBuf> {
    checkouts
        .iter()
        .filter_map(|checkout| {
            let fsck_dir = checkout.fsck_dir();
            if fsck_dir.exists() {
                Some(fsck_dir)
            } else {
                None
            }
        })
        .collect()
}

/// Find all the directories under `redirection_path` that aren't present in
/// `existing_redirections`.
fn recursively_check_orphaned_mirrored_redirections(
    redirection_path: PathBuf,
    existing_redirections: &BTreeSet<PathBuf>,
) -> std::io::Result<Vec<PathBuf>> {
    let mut to_walk = VecDeque::new();
    to_walk.push_back(redirection_path);

    let mut orphaned = Vec::new();
    while let Some(current) = to_walk.pop_front() {
        // A range is required here to distinguish 3 cases:
        //  0) Is that path an existing redirection
        //  1) Is there an existing redirection in a subdirectory?
        //  2) Is this an orphaned redirection?
        let num_existing_redirections = existing_redirections
            // Logarithmically filter all the paths whose prefix is `current`
            .range(std::ops::RangeFrom {
                start: current.clone(),
            })
            // And then filter the remaining paths whose prefix do not start with `current`.
            .take_while(|p| p.starts_with(&current))
            .count();
        match num_existing_redirections {
            0 => orphaned.push(current),
            1 if existing_redirections.contains(&current) => continue,
            _ => {
                if current.is_dir() {
                    for current_subdir in fs::read_dir(current)? {
                        to_walk.push_back(current_subdir?.path());
                    }
                }
            }
        }
    }

    Ok(orphaned)
}

fn get_orphaned_redirections_impl(
    scratch_path: PathBuf,
    scratch_subdir: PathBuf,
    existing_redirections: &BTreeSet<PathBuf>,
) -> Result<Vec<PathBuf>> {
    // Scratch directories can either be flat, ie: a directory like foo/bar will be encoded as
    // fooZbar, or mirrored, where no encoding is performed. Let's test how mkscratch encoded the
    // directory and compare it against the EdenFS scratch namespace to test if mkscratch is
    // configured to be flat or mirrored.
    let is_scratch_mirrored = scratch_path.ends_with(&scratch_subdir);
    let (scratch_root, prefix) = if is_scratch_mirrored {
        (
            scratch_path
                .ancestors()
                .nth(scratch_subdir.components().count() + 1)
                .unwrap(),
            scratch_subdir,
        )
    } else {
        (
            // We want to get the root of the scratch directory, which is 2 level up from the path
            // mkscratch gave us: first the path in the repository, and second the repository path.
            scratch_path.parent().unwrap().parent().unwrap(),
            PathBuf::from(scratch_path.file_name().unwrap().to_os_string()),
        )
    };

    let mut orphaned_redirections = Vec::new();
    if is_scratch_mirrored {
        for dirent in fs::read_dir(scratch_root)? {
            let dirent_path = dirent?.path();
            let redirection_path = dirent_path.join(&prefix);
            if redirection_path.exists() {
                // The directory exist, now we need to check if there is an unknown redirection.
                orphaned_redirections.extend(recursively_check_orphaned_mirrored_redirections(
                    redirection_path,
                    existing_redirections,
                )?);
            }
        }
    } else {
        for dirent in fs::read_dir(scratch_root)? {
            let dirent_path = dirent?.path();
            if !dirent_path.is_dir() {
                continue;
            }

            for subdir in fs::read_dir(dirent_path)? {
                let path = subdir?.path();
                if !existing_redirections.contains(&path)
                    && path
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(&prefix.to_string_lossy().into_owned())
                {
                    orphaned_redirections.push(path);
                }
            }
        }
    }

    Ok(orphaned_redirections)
}

/// Try to find all the orphaned redirections.
///
/// When a repository is removed, its redirections aren't cleaned up and can take significant
/// amount of disk space. Finding them will allow `eden du --clean` to remove them.
fn get_orphaned_redirections(existing_redirections: &BTreeSet<PathBuf>) -> Result<Vec<PathBuf>> {
    let mkscratch = Redirection::mkscratch_bin();
    let scratch_subdir = Redirection::scratch_subdir();
    let scratch_subdir_str = scratch_subdir.to_string_lossy();
    let home_dir = match dirs::home_dir() {
        Some(dir) => dir,
        None => return Ok(vec![]),
    };
    let home_dir_str = home_dir.to_string_lossy();

    let mkscratch_args = vec![
        "--no-create",
        "path",
        &*home_dir_str,
        "--subdir",
        &*scratch_subdir_str,
    ];
    let mkscratch_res = Exec::cmd(mkscratch)
        .args(&mkscratch_args)
        .stdout(SubprocessRedirection::Pipe)
        .stderr(SubprocessRedirection::Pipe)
        .capture();

    let scratch_path = match mkscratch_res {
        Ok(output) if output.success() => PathBuf::from(output.stdout_str().trim()),
        _ => return Ok(vec![]),
    };

    get_orphaned_redirections_impl(scratch_path, scratch_subdir, existing_redirections)
}

/// Warn about backing repositories that are non-empty working copy.
fn warn_about_working_copy_for_backing_repo(backing_repos: &HashSet<PathBuf>) -> Result<()> {
    let mut warned = false;
    for backing_repo in backing_repos.iter() {
        // A non-empty working copy will contain more than just the .hg at the root.
        if backing_repo.join(".hg").is_dir() && fs::read_dir(backing_repo)?.count() > 1 {
            if !warned {
                println!(
                    "\nWorking copy detected in backing repo. This is not generally useful \
                    and just takes up space.  You can make this a bare repo to reclaim \
                    space by running:\n"
                );
                warned = true;
            }
            println!("hg -R {} checkout null", backing_repo.display());
        }
    }
    Ok(())
}

/// Run `buck clean` to reduce disk space usage of the buck-out directories.
fn clean_buck_redirections(buck_redirections: HashSet<PathBuf>) -> Result<()> {
    for redir in buck_redirections {
        println!(
            "\n{}",
            format!("Reclaiming space from redirection: {}", redir.display()).blue()
        );
        if let Some(basename) = redir.parent() {
            let output = Exec::cmd(get_buck_command())
                .arg("clean")
                .stderr(SubprocessRedirection::Pipe)
                .cwd(basename)
                .env_clear()
                .env_extend(&get_env_with_buck_version(basename)?)
                .capture()
                .with_context(|| format!("Failed to run buck {}", get_buck_command()))?;

            if output.success() {
                println!("{}", "Space reclaimed".blue());
            } else {
                return Err(anyhow!(
                    "Failed to execute buck clean from {}, stderr: {}, exit status: {:?}",
                    basename.display(),
                    output.stderr_str(),
                    output.exit_status,
                ));
            }
        } else {
            return Err(anyhow!("Found invalid redirection: {}", redir.display()));
        };
    }
    Ok(())
}

#[async_trait]
impl crate::Subcommand for DiskUsageCmd {
    async fn run(&self, instance: EdenFsInstance) -> Result<ExitCode> {
        let client = instance.connect(None).await?;

        let mounts = self
            .get_mounts(&instance)
            .context("Failed to get EdenFS mounts")?;
        let checkouts =
            get_checkouts(&mounts, &instance).context("Failed to get EdenFS checkouts")?;
        let backing_repos = get_backing_repos(&checkouts);
        let (redirections, buck_redirections) =
            get_redirections(&checkouts).context("Failed to get EdenFS redirections")?;
        let orphaned_redirections =
            get_orphaned_redirections(&redirections).unwrap_or_else(|_| Vec::new());
        let fsck_dirs = get_fsck_dirs(&checkouts);

        let mut aggregated_usage_counts = AggregatedUsageCounts::new();

        let mut backing_failed_file_checks = HashSet::new();
        for b in backing_repos.iter() {
            // GET SUMMARY INFO for backing counts
            let (usage_count, failed_file_checks) = usage_for_dir(b, None).with_context(|| {
                format!(
                    "Failed to measure disk space usage for backing repository {}",
                    b.display()
                )
            })?;
            aggregated_usage_counts.backing += usage_count;
            backing_failed_file_checks.extend(failed_file_checks);
        }

        let mut mount_failed_file_checks = HashSet::new();
        for checkout in checkouts.iter() {
            // GET SUMMARY INFO for materialized counts
            let overlay_dir = checkout.data_dir().join("local");
            let (usage_count, failed_file_checks) = usage_for_dir(&overlay_dir, None)
                .with_context(|| {
                    format!(
                        "Failed to measure disk space usage for overlay {}",
                        overlay_dir.display()
                    )
                })?;
            aggregated_usage_counts.materialized += usage_count;
            mount_failed_file_checks.extend(failed_file_checks);

            // GET SUMMARY INFO for ignored counts
            aggregated_usage_counts.ignored +=
                ignored_usage_counts_for_mount(&checkout, &client).await?;
        }

        for fsck_dir in fsck_dirs.iter() {
            let (usage_count, failed_file_checks) =
                usage_for_dir(fsck_dir, None).with_context(|| {
                    format!(
                        "Failed to measure disk space usage for fsck directory {}",
                        fsck_dir.display()
                    )
                })?;
            aggregated_usage_counts.fsck += usage_count;
            mount_failed_file_checks.extend(failed_file_checks);
        }

        let mut redirection_failed_file_checks = HashSet::new();
        for target in redirections {
            // GET SUMMARY INFO for redirections
            let (usage_count, failed_file_checks) =
                usage_for_dir(&target, None).with_context(|| {
                    format!(
                        "Failed to measure disk space usage for redirection {}",
                        target.display()
                    )
                })?;
            aggregated_usage_counts.redirection += usage_count;
            redirection_failed_file_checks.extend(failed_file_checks);
        }

        let mut orphaned_redirection_failed_file_checks = HashSet::new();
        for orphaned in orphaned_redirections.iter() {
            let (usage_count, failed_file_checks) =
                usage_for_dir(orphaned, None).with_context(|| {
                    format!(
                        "Failed to measure disk usage for orphaned redirection {}",
                        orphaned.display()
                    )
                })?;
            aggregated_usage_counts.orphaned_redirections += usage_count;
            orphaned_redirection_failed_file_checks.extend(failed_file_checks);
        }

        // Make immutable
        let backing_failed_file_checks = backing_failed_file_checks;
        let mount_failed_file_checks = mount_failed_file_checks;
        let redirection_failed_file_checks = redirection_failed_file_checks;
        let buck_redirections = buck_redirections;

        // GET SUMMARY INFO for shared usage
        let mut shared_failed_file_checks = HashSet::new();
        let (logs_dir_usage, failed_logs_dir_file_checks) =
            usage_for_dir(&instance.logs_dir(), None).with_context(|| {
                format!(
                    "Failed to measure disk space usage for EdenFS logs {}",
                    instance.logs_dir().display()
                )
            })?;
        aggregated_usage_counts.shared += logs_dir_usage;
        shared_failed_file_checks.extend(failed_logs_dir_file_checks);
        let (storage_dir_usage, failed_storage_dir_file_checks) =
            usage_for_dir(&instance.storage_dir(), None).with_context(|| {
                format!(
                    "Failed to measure disk space usage for EdenFS LocalStore {}",
                    instance.storage_dir().display()
                )
            })?;
        aggregated_usage_counts.shared += storage_dir_usage;
        shared_failed_file_checks.extend(failed_storage_dir_file_checks);

        // Make immutable
        let shared_failed_file_checks = shared_failed_file_checks;
        let aggregated_usage_counts = aggregated_usage_counts;

        // GET HGCACHE PATH
        let hg_cache_path = get_hg_cache_path().context("Failed to get hgcache path")?;

        // PRINT OUTPUT
        if self.json {
            println!(
                "{}",
                serde_json::to_string(&aggregated_usage_counts)
                    .context("Failed to serialize usage counts")?
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

            if self.should_clean() {
                self.clean_fsck_directories(fsck_dirs);
            }

            // PRINT REDIRECTIONS
            write_title("Buck redirections");
            if buck_redirections.is_empty() {
                println!("No Buck redirections");
            } else {
                for redir in &buck_redirections {
                    println!("{}", redir.display());
                }
            }
            write_failed_to_check_files_message(&redirection_failed_file_checks);

            if !buck_redirections.is_empty() {
                if self.should_clean() {
                    clean_buck_redirections(buck_redirections)
                        .context("Failed to clean Buck redirections")?;
                } else {
                    println!(
                        "\nTo reclaim space from buck-out directories, run `buck clean` from the \
                        parent of the buck-out directory."
                    );
                }
            }

            write_title("Orphaned redirections");
            if orphaned_redirections.is_empty() {
                println!("No orphaned redirections");
            } else {
                for redir in orphaned_redirections.iter() {
                    println!("{}", redir.display());
                }
            }
            write_failed_to_check_files_message(&orphaned_redirection_failed_file_checks);

            if !orphaned_redirections.is_empty() {
                if self.should_clean() {
                    for redir in orphaned_redirections.iter() {
                        fs::remove_dir_all(&redir).with_context(|| {
                            format!("Failed to recursively remove {}", redir.display())
                        })?;
                    }
                }
            }

            // PRINT BACKING REPOS
            if !backing_repos.is_empty() || !backing_failed_file_checks.is_empty() {
                write_title("Backing repos");
            }
            if !backing_repos.is_empty() {
                for backing in backing_repos.iter() {
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

            warn_about_working_copy_for_backing_repo(&backing_repos)
                .context("Failed to warn about working copy in backing repo")?;

            // PRINT SHARED SPACE
            write_title("Shared space");
            if self.should_clean() {
                println!(
                    "{}",
                    "Cleaning shared space used by the storage engine...".blue()
                );
                let output = Exec::cmd("eden")
                    .arg("gc")
                    .stdout(SubprocessRedirection::Pipe)
                    .stderr(SubprocessRedirection::Pipe)
                    .capture()?;

                if output.success() {
                    println!("{}", "Finished cleaning shared space.".blue())
                } else {
                    return Err(anyhow!(
                        "Failed to execute `eden gc`, stderr: {}, exit status: {:?}",
                        output.stderr_str(),
                        output.exit_status,
                    ));
                }
            } else {
                println!("Run `eden gc` to reduce the space used by the storage engine.");
            }
            write_failed_to_check_files_message(&shared_failed_file_checks);

            // PRINT SUMMARY
            write_title("Summary");
            if self.deep_clean {
                println!("{:+#}", aggregated_usage_counts);
            } else if self.clean {
                println!("{:#}", aggregated_usage_counts);
            } else {
                println!("{}", aggregated_usage_counts);
            }

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

#[cfg(test)]
mod tests {
    use std::fs::create_dir;
    use std::fs::create_dir_all;

    use tempfile::TempDir;

    use super::*;

    fn create_and_add(
        path: impl AsRef<Path>,
        existing_redirections: &mut BTreeSet<PathBuf>,
    ) -> Result<()> {
        create_dir_all(path.as_ref())?;
        existing_redirections.insert(path.as_ref().to_path_buf());
        Ok(())
    }

    #[test]
    fn test_recursive_check_orphaned_mirrored_redirections() -> Result<()> {
        let tempdir = TempDir::new()?;
        let path = tempdir.path();
        let mut existing_redirections = BTreeSet::new();

        // Single known directory
        create_and_add(path.join("A"), &mut existing_redirections)?;

        // Directory with an orphaned directory inside
        create_and_add(path.join("B/1"), &mut existing_redirections)?;
        create_and_add(path.join("B/2"), &mut existing_redirections)?;
        create_dir(path.join("B/3"))?;

        // Single orphaned directory
        create_dir(path.join("C"))?;

        // Single orphaned with several subdirectories
        create_dir_all(path.join("D/1"))?;
        create_dir_all(path.join("D/2"))?;
        create_dir_all(path.join("D/3"))?;

        // Orphaned redirection with an existing redirection as a sibling
        create_dir_all(path.join("E/1/2"))?;
        create_and_add(path.join("E/1/3"), &mut existing_redirections)?;

        let res = recursively_check_orphaned_mirrored_redirections(
            path.to_path_buf(),
            &existing_redirections,
        )?;
        assert!(!res.contains(&path.join("A")));

        assert!(!res.contains(&path.join("B")));
        assert!(!res.contains(&path.join("B/1")));
        assert!(!res.contains(&path.join("B/2")));
        assert!(res.contains(&path.join("B/3")));

        assert!(res.contains(&path.join("C")));

        assert!(res.contains(&path.join("D")));

        eprintln!("{:?}", res);
        assert!(res.contains(&path.join("E/1/2")));
        Ok(())
    }

    #[test]
    fn test_get_orphaned_redirections_mirrored() -> Result<()> {
        let tempdir = TempDir::new()?;
        let path = tempdir.path();
        let scratch_subdir = Path::new("foo/bar");
        let mut existing_redirections = BTreeSet::new();

        let scratch_path = path.join("repository").join(scratch_subdir);

        // Single known directory
        create_and_add(
            path.join("repo1").join(scratch_subdir).join("A"),
            &mut existing_redirections,
        )?;

        // Directory with an orphaned directory inside
        create_and_add(
            path.join("repo2").join(scratch_subdir).join("B/1"),
            &mut existing_redirections,
        )?;
        create_and_add(
            path.join("repo2").join(scratch_subdir).join("B/2"),
            &mut existing_redirections,
        )?;
        create_dir_all(path.join("repo2").join(scratch_subdir).join("B/3"))?;

        // Single orphaned directory
        create_dir_all(path.join("repo3").join(scratch_subdir).join("C"))?;

        // Single orphaned with several subdirectories
        create_dir_all(path.join("repo4").join(scratch_subdir).join("D/1"))?;
        create_dir_all(path.join("repo4").join(scratch_subdir).join("D/2"))?;
        create_dir_all(path.join("repo4").join(scratch_subdir).join("D/3"))?;

        let res = get_orphaned_redirections_impl(
            scratch_path,
            scratch_subdir.to_path_buf(),
            &existing_redirections,
        )?;
        assert!(!res.contains(&path.join("repo1").join(scratch_subdir).join("A")));

        assert!(!res.contains(&path.join("repo2").join(scratch_subdir).join("B")));
        assert!(!res.contains(&path.join("repo2").join(scratch_subdir).join("B/1")));
        assert!(!res.contains(&path.join("repo2").join(scratch_subdir).join("B/2")));
        assert!(res.contains(&path.join("repo2").join(scratch_subdir).join("B/3")));

        assert!(res.contains(&path.join("repo3").join(scratch_subdir)));

        assert!(res.contains(&path.join("repo4").join(scratch_subdir)));

        Ok(())
    }

    #[test]
    fn test_get_orphaned_redirections_flat() -> Result<()> {
        let tempdir = TempDir::new()?;
        let path = tempdir.path();
        let scratch_subdir = Path::new("fooZbar");
        let mut existing_redirections = BTreeSet::new();

        let scratch_path = path.join("repository").join(scratch_subdir);

        // Single known directory
        let repo1_a_path = path
            .join("repo1")
            .join(format!("{}Z{}", scratch_subdir.display(), "A"));
        create_and_add(&repo1_a_path, &mut existing_redirections)?;

        // Directory with an orphaned directory inside
        let repo2_b1_path =
            path.join("repo2")
                .join(format!("{}Z{}Z{}", scratch_subdir.display(), "B", "1"));
        let repo2_b2_path =
            path.join("repo2")
                .join(format!("{}Z{}Z{}", scratch_subdir.display(), "B", "2"));
        let repo2_b3_path =
            path.join("repo2")
                .join(format!("{}Z{}Z{}", scratch_subdir.display(), "B", "3"));
        create_and_add(&repo2_b1_path, &mut existing_redirections)?;
        create_and_add(&repo2_b2_path, &mut existing_redirections)?;
        create_dir_all(&repo2_b3_path)?;

        // Single orphaned directory
        let repo3_c_path = path
            .join("repo3")
            .join(format!("{}Z{}", scratch_subdir.display(), "C"));
        create_dir_all(&repo3_c_path)?;

        let res = get_orphaned_redirections_impl(
            scratch_path,
            Path::new("foo/bar").to_path_buf(),
            &existing_redirections,
        )?;
        assert!(!res.contains(&repo1_a_path));

        assert!(!res.contains(&repo2_b1_path));
        assert!(!res.contains(&repo2_b2_path));
        assert!(res.contains(&repo2_b3_path));

        assert!(res.contains(&repo3_c_path));

        Ok(())
    }
}
