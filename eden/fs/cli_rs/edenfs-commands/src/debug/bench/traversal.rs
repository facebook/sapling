/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Filesystem traversal benchmarking

use std::fs;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use edenfs_client::client::Client;
use edenfs_client::methods::EdenThriftMethod;
use edenfs_utils::bytes_from_path;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use sysinfo::Pid;
use sysinfo::System;
use thrift_types::edenfs::MountId;
use thrift_types::edenfs::ScmBlobOrError;
use thrift_types::edenfs::SyncBehavior;
use tokio_util::sync::CancellationToken;

use super::types;
use super::types::Benchmark;
use super::types::BenchmarkType;
use crate::get_edenfs_instance;

// ============================================================================
// Type Definitions
// ============================================================================

#[derive(Clone)]
enum ReadMode {
    Skip,
    Fs,
    Thrift {
        client: std::sync::Arc<edenfs_client::client::EdenFsClient>,
        repo_path: PathBuf,
    },
}

struct Traversal {
    // Counters
    file_count: usize,
    dir_count: usize,
    symlink_skipped_count: usize,
    symlink_traversed_count: usize,
    successful_reads: usize,
    total_dir_entries: usize,
    total_bytes_read: u64,

    // Durations
    start_time: Instant,
    read_dir_duration: std::time::Duration,
    read_duration: std::time::Duration,
    open_duration: std::time::Duration,

    // Configuration
    max_files: usize,
    follow_symlinks: bool,
    read_mode: ReadMode,
    cancellation_token: CancellationToken,

    // UI and monitoring
    progress_bar: Option<ProgressBar>,
    system: Option<System>,
    pid: Option<Pid>,

    // Working buffer
    buffer: Vec<u8>,
}

// ============================================================================
// Type Implementations
// ============================================================================

impl Traversal {
    fn new(
        no_progress: bool,
        resource_usage: bool,
        max_files: usize,
        follow_symlinks: bool,
        cancellation_token: CancellationToken,
        read_mode: ReadMode,
    ) -> Self {
        let progress_bar = if no_progress {
            None
        } else {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("[{elapsed_precise}] {msg}")
                    .unwrap(),
            );
            let initial_files_display = if max_files == usize::MAX {
                "0".to_string()
            } else {
                format!("0/{}", max_files)
            };
            pb.set_message(format!(
                "{} files | 0 dirs | 0 files/s",
                initial_files_display
            ));
            Some(pb)
        };

        let (system, pid) = if resource_usage {
            let mut sys = System::new_all();
            let process_id = sysinfo::get_current_pid().expect("Failed to get current process ID");
            sys.refresh_all();
            (Some(sys), Some(process_id))
        } else {
            (None, None)
        };

        Self {
            // Counters
            file_count: 0,
            dir_count: 0,
            symlink_skipped_count: 0,
            symlink_traversed_count: 0,
            successful_reads: 0,
            total_dir_entries: 0,
            total_bytes_read: 0,

            // Durations
            start_time: Instant::now(),
            read_dir_duration: std::time::Duration::new(0, 0),
            read_duration: std::time::Duration::new(0, 0),
            open_duration: std::time::Duration::new(0, 0),

            // Configuration
            max_files,
            follow_symlinks,
            read_mode,
            cancellation_token,

            // UI and monitoring
            progress_bar,
            system,
            pid,

            // Working buffer
            buffer: Vec::with_capacity(types::BYTES_IN_MEGABYTE),
        }
    }

    fn add_file(&mut self) {
        self.file_count += 1;
        if (self.file_count + self.dir_count)
            .is_multiple_of(types::TRAVERSAL_PROGRESS_UPDATE_INTERVAL)
        {
            self.update_progress();
        }
    }

    fn add_dir(&mut self) {
        self.dir_count += 1;
        if (self.file_count + self.dir_count)
            .is_multiple_of(types::TRAVERSAL_PROGRESS_UPDATE_INTERVAL)
        {
            self.update_progress();
        }
    }

    fn add_traversed_symlink(&mut self) {
        self.symlink_traversed_count += 1;
    }

    fn add_skipped_symlink(&mut self) {
        self.symlink_skipped_count += 1;
    }

    fn add_read_dir_stats(&mut self, duration: std::time::Duration, entry_count: usize) {
        self.read_dir_duration += duration;
        self.total_dir_entries += entry_count;
    }

    fn update_progress(&mut self) {
        if let Some(pb) = &self.progress_bar {
            let elapsed = self.start_time.elapsed().as_secs_f64();
            if elapsed <= 0.0 {
                return;
            }

            let files_per_second = self.file_count as f64 / elapsed;

            let files_display = if self.max_files == usize::MAX {
                self.file_count.to_string()
            } else {
                format!("{}/{}", self.file_count, self.max_files)
            };

            let show_throughput = !matches!(self.read_mode, ReadMode::Skip);

            let message = if let (Some(system), Some(pid)) = (&mut self.system, &self.pid) {
                system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[*pid]), false);
                match system.process(*pid) {
                    Some(process) => {
                        let memory_mb = process.memory() as f64 / types::BYTES_IN_MEGABYTE as f64;
                        let cpu_usage = process.cpu_usage();
                        if show_throughput {
                            let mb_per_second = self.total_bytes_read as f64
                                / types::BYTES_IN_MEGABYTE as f64
                                / elapsed;
                            format!(
                                "{} files | {} dirs | {:.0} files/s | {:.2} MiB/s | {:.2} MiB memory | {:.2}% CPU",
                                files_display,
                                self.dir_count,
                                files_per_second,
                                mb_per_second,
                                memory_mb,
                                cpu_usage
                            )
                        } else {
                            format!(
                                "{} files | {} dirs | {:.0} files/s | {:.2} MiB memory | {:.2}% CPU",
                                files_display,
                                self.dir_count,
                                files_per_second,
                                memory_mb,
                                cpu_usage
                            )
                        }
                    }
                    None => {
                        if show_throughput {
                            let mb_per_second = self.total_bytes_read as f64
                                / types::BYTES_IN_MEGABYTE as f64
                                / elapsed;
                            format!(
                                "{} files | {} dirs | {:.0} files/s | {:.2} MiB/s",
                                files_display, self.dir_count, files_per_second, mb_per_second
                            )
                        } else {
                            format!(
                                "{} files | {} dirs | {:.0} files/s",
                                files_display, self.dir_count, files_per_second
                            )
                        }
                    }
                }
            } else if show_throughput {
                let mb_per_second =
                    self.total_bytes_read as f64 / types::BYTES_IN_MEGABYTE as f64 / elapsed;
                format!(
                    "{} files | {} dirs | {:.0} files/s | {:.2} MiB/s",
                    files_display, self.dir_count, files_per_second, mb_per_second
                )
            } else {
                format!(
                    "{} files | {} dirs | {:.0} files/s",
                    files_display, self.dir_count, files_per_second
                )
            };

            pb.set_message(message);
        }
    }

    async fn traverse_file(&mut self, path: PathBuf) -> Result<()> {
        match &self.read_mode {
            ReadMode::Skip => Ok(()),
            ReadMode::Fs => {
                if self.cancellation_token.is_cancelled() {
                    return Ok(());
                }

                let start = Instant::now();
                let mut file = match File::open(&path) {
                    Ok(f) => f,
                    Err(_) => return Ok(()),
                };
                self.open_duration += start.elapsed();

                let start = Instant::now();
                if let Ok(bytes_read) = file.read_to_end(&mut self.buffer) {
                    self.total_bytes_read += bytes_read as u64;
                    self.successful_reads += 1;
                }
                self.read_duration += start.elapsed();
                self.buffer.clear();

                Ok(())
            }
            ReadMode::Thrift { client, repo_path } => {
                if self.cancellation_token.is_cancelled() {
                    return Ok(());
                }

                let abs_path = path.canonicalize()?;
                let rel_file_path = abs_path
                    .strip_prefix(repo_path)
                    .map_err(|_| {
                        anyhow::anyhow!(
                            "File path does not start with repo path (after canonicalization)"
                        )
                    })?
                    .to_path_buf();

                let start = Instant::now();
                let request = get_thrift_request(repo_path.clone(), rel_file_path)?;
                let response = client
                    .with_thrift(|thrift| {
                        (
                            thrift.getFileContent(&request),
                            EdenThriftMethod::GetFileContent,
                        )
                    })
                    .await?;
                self.read_duration += start.elapsed();

                match response.blob {
                    ScmBlobOrError::blob(blob) => {
                        self.total_bytes_read += blob.len() as u64;
                        self.successful_reads += 1;
                    }
                    _ => {}
                }

                Ok(())
            }
        }
    }

    fn total_elapsed_time(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    fn finish_progress_bar(&self) {
        if let Some(pb) = &self.progress_bar {
            pb.finish_and_clear();
        }
    }

    pub async fn traverse_path(&mut self, path: &Path) -> Result<()> {
        if path.is_dir() {
            self.traverse_directory(path).await?;
        }
        Ok(())
    }

    async fn traverse_directory(&mut self, path: &Path) -> Result<()> {
        let mut stack = vec![path.to_path_buf()];

        while let Some(current_path) = stack.pop() {
            if self.cancellation_token.is_cancelled() {
                return Ok(());
            }

            self.add_dir();

            let start_time = Instant::now();
            let entries = fs::read_dir(&current_path)?;
            let read_dir_duration = start_time.elapsed();

            let mut entry_count = 0;

            for entry_result in entries {
                let entry = entry_result?;
                let entry_path = entry.path();
                let file_type = entry.file_type()?;
                entry_count += 1;

                if file_type.is_dir() {
                    if file_type.is_symlink() {
                        if self.follow_symlinks {
                            self.add_traversed_symlink();
                            stack.push(entry_path);
                        } else {
                            self.add_skipped_symlink();
                        }
                    } else {
                        stack.push(entry_path);
                    }
                } else if file_type.is_file() {
                    if self.file_count < self.max_files {
                        self.add_file();
                        self.traverse_file(entry_path).await?;
                    } else {
                        self.add_read_dir_stats(read_dir_duration, entry_count);
                        return Ok(());
                    }
                }
            }

            self.add_read_dir_stats(read_dir_duration, entry_count);
        }

        Ok(())
    }
}

// ============================================================================
// Public Functions
// ============================================================================

pub async fn bench_traversal(
    dir_path: &str,
    max_files: usize,
    follow_symlinks: bool,
    no_progress: bool,
    resource_usage: bool,
    skip_read: bool,
    thrift_io: Option<&str>,
) -> Result<Benchmark> {
    let path = Path::new(dir_path);
    if !path.exists() || !path.is_dir() {
        return Err(anyhow::anyhow!("Invalid directory path: {}", dir_path));
    }

    let cancellation_token = setup_cancellation();

    let read_mode = if skip_read {
        ReadMode::Skip
    } else if let Some(fbsource_path) = thrift_io {
        let client = get_edenfs_instance().get_client();
        let repo_path = PathBuf::from(fbsource_path).canonicalize()?;
        ReadMode::Thrift { client, repo_path }
    } else {
        ReadMode::Fs
    };

    let mut traversal = Traversal::new(
        no_progress,
        resource_usage,
        max_files,
        follow_symlinks,
        cancellation_token.clone(),
        read_mode.clone(),
    );

    traversal.traverse_path(path).await?;
    traversal.finish_progress_bar();

    let mut result = Benchmark::new(BenchmarkType::FsTraversal);
    add_traversal_metrics(&mut result, &traversal, &read_mode)?;

    Ok(result)
}

// ============================================================================
// Private Helper Functions
// ============================================================================

fn get_thrift_request(
    repo_path: PathBuf,
    rel_file_path: PathBuf,
) -> Result<thrift_types::edenfs::GetFileContentRequest> {
    let req = thrift_types::edenfs::GetFileContentRequest {
        mount: MountId {
            mountPoint: bytes_from_path(repo_path)?,
            ..Default::default()
        },
        filePath: bytes_from_path(rel_file_path)?,
        sync: SyncBehavior {
            ..Default::default()
        },
        ..Default::default()
    };
    Ok(req)
}

fn setup_cancellation() -> CancellationToken {
    let token = CancellationToken::new();

    let token_clone = token.clone();
    if let Err(err) = ctrlc::set_handler(move || {
        token_clone.cancel();
    }) {
        eprintln!("Failed to set Ctrl+C handler: {}", err);
    }

    token
}

fn add_traversal_metrics(
    result: &mut Benchmark,
    traversal: &Traversal,
    read_mode: &ReadMode,
) -> Result<()> {
    let total_elapsed_time = traversal.total_elapsed_time();

    if total_elapsed_time <= 0.0 {
        return Err(anyhow::anyhow!(
            "Total elapsed time is less or equal to zero."
        ));
    }

    let files_per_second = traversal.file_count as f64 / total_elapsed_time;

    let avg_read_dir_latency = if traversal.dir_count > 0 {
        traversal.read_dir_duration.as_secs_f64() * 1000.0 / traversal.dir_count as f64
    } else {
        0.0
    };

    let avg_dir_size = if traversal.dir_count > 0 {
        traversal.total_dir_entries as f64 / traversal.dir_count as f64
    } else {
        0.0
    };

    result.add_metric(
        "Traversal throughput",
        files_per_second,
        types::Unit::FilesPerSecond,
        Some(0),
    );
    result.add_metric(
        "Average read_dir() latency",
        avg_read_dir_latency,
        types::Unit::Ms,
        Some(4),
    );
    result.add_metric(
        "Average directory size",
        avg_dir_size,
        types::Unit::Files,
        Some(2),
    );
    result.add_metric(
        "Total files scanned",
        traversal.file_count as f64,
        types::Unit::Files,
        Some(0),
    );
    result.add_metric(
        "Total directories",
        traversal.dir_count as f64,
        types::Unit::Dirs,
        Some(0),
    );
    result.add_metric(
        "Total symlinks skipped",
        traversal.symlink_skipped_count as f64,
        types::Unit::Symlinks,
        Some(0),
    );
    result.add_metric(
        "Total symlinks traversed",
        traversal.symlink_traversed_count as f64,
        types::Unit::Symlinks,
        Some(0),
    );

    if traversal.successful_reads > 0 {
        let avg_file_size = traversal.total_bytes_read as f64 / traversal.successful_reads as f64;
        let avg_file_size_kb = avg_file_size / types::BYTES_IN_KILOBYTE as f64;

        result.add_metric(
            "Total files read",
            traversal.successful_reads as f64,
            types::Unit::Files,
            Some(0),
        );
        result.add_metric(
            "Average file size",
            avg_file_size_kb,
            types::Unit::KiB,
            Some(2),
        );
        result.add_metric(
            "Total data read",
            traversal.total_bytes_read as f64 / types::BYTES_IN_MEGABYTE as f64,
            types::Unit::MiB,
            Some(2),
        );

        match read_mode {
            ReadMode::Thrift { .. } => {
                let avg_read_dur =
                    traversal.read_duration.as_secs_f64() / traversal.successful_reads as f64;
                let mb_per_second = traversal.total_bytes_read as f64
                    / types::BYTES_IN_MEGABYTE as f64
                    / traversal.read_duration.as_secs_f64();

                result.add_metric("Throughput", mb_per_second, types::Unit::MiBps, Some(2));
                result.add_metric(
                    "Average thrift read latency",
                    avg_read_dur * 1000.0,
                    types::Unit::Ms,
                    Some(4),
                );
            }
            ReadMode::Fs => {
                let avg_open_dur =
                    traversal.open_duration.as_secs_f64() / traversal.successful_reads as f64;
                let avg_read_dur =
                    traversal.read_duration.as_secs_f64() / traversal.successful_reads as f64;
                let total_duration =
                    (traversal.open_duration + traversal.read_duration).as_secs_f64();
                let mb_per_second = traversal.total_bytes_read as f64
                    / types::BYTES_IN_MEGABYTE as f64
                    / total_duration;

                result.add_metric(
                    "open() + read() throughput",
                    mb_per_second,
                    types::Unit::MiBps,
                    Some(2),
                );
                result.add_metric(
                    "open() latency",
                    avg_open_dur * 1000.0,
                    types::Unit::Ms,
                    Some(4),
                );
                result.add_metric(
                    "Average read() latency",
                    avg_read_dur * 1000.0,
                    types::Unit::Ms,
                    Some(4),
                );
            }
            ReadMode::Skip => {}
        }
    }

    Ok(())
}
