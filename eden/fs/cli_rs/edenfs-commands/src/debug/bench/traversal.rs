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
use async_recursion::async_recursion;
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

/// Build benchmark results with only traversal metrics (no file reading)
fn build_traversal_only_benchmark(ft: FinalizedTraversal) -> Result<Benchmark> {
    let avg_read_dir_latency = if ft.dir_count > 0 {
        ft.total_read_dir_time.as_secs_f64() * 1000.0 / ft.dir_count as f64
    } else {
        0.0
    };

    let avg_dir_size = if ft.dir_count > 0 {
        ft.total_dir_entries as f64 / ft.dir_count as f64
    } else {
        0.0
    };

    if ft.duration <= 0.0 {
        return Err(anyhow::anyhow!("Duration is less or equal to zero."));
    }

    let files_per_second = ft.file_count as f64 / ft.duration;

    let mut result = Benchmark::new(BenchmarkType::FsTraversal);

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
        ft.file_count as f64,
        types::Unit::Files,
        Some(0),
    );
    result.add_metric(
        "Total directories",
        ft.dir_count as f64,
        types::Unit::Dirs,
        Some(0),
    );
    result.add_metric(
        "Total symlinks skipped",
        ft.symlink_skipped_count as f64,
        types::Unit::Symlinks,
        Some(0),
    );
    result.add_metric(
        "Total symlinks traversed",
        ft.symlink_traversed_count as f64,
        types::Unit::Symlinks,
        Some(0),
    );

    Ok(result)
}

#[derive(Debug, PartialEq)]
enum TraversalResult {
    Continue,
    LimitReached,
    Interrupted,
}

struct InProgressTraversal {
    file_count: usize,
    dir_count: usize,
    symlink_skipped_count: usize,
    symlink_traversed_count: usize,
    start_time: Instant,
    progress_bar: Option<ProgressBar>,
    file_paths: Vec<PathBuf>,
    total_read_dir_time: std::time::Duration,
    total_dir_entries: usize,
    max_files: usize,
    follow_symlinks: bool,
    system: Option<System>,
    pid: Option<Pid>,
    cancellation_token: CancellationToken,
}

// Define a struct for the results from the finalize step
#[derive(Debug)]
pub struct FinalizedTraversal {
    file_count: usize,
    dir_count: usize,
    symlink_skipped_count: usize,
    symlink_traversed_count: usize,
    duration: f64,
    file_paths: Vec<PathBuf>,
    total_read_dir_time: std::time::Duration,
    total_dir_entries: usize,
}

impl InProgressTraversal {
    fn new(
        no_progress: bool,
        resource_usage: bool,
        max_files: usize,
        follow_symlinks: bool,
        cancellation_token: CancellationToken,
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
            // Set initial message with max files if limit is set
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

        // Only initialize system monitoring if resource usage monitoring is enabled
        let (system, pid) = if resource_usage {
            let mut sys = System::new_all();
            let process_id = sysinfo::get_current_pid().expect("Failed to get current process ID");
            sys.refresh_all();
            (Some(sys), Some(process_id))
        } else {
            (None, None)
        };

        Self {
            file_count: 0,
            dir_count: 0,
            symlink_skipped_count: 0,
            symlink_traversed_count: 0,
            start_time: Instant::now(),
            progress_bar,
            file_paths: Vec::with_capacity(
                max_files.min(types::DEFAULT_MAX_NUMBER_OF_FILES_FOR_TRAVERSAL),
            ),
            total_read_dir_time: std::time::Duration::new(0, 0),
            total_dir_entries: 0,
            max_files,
            follow_symlinks,
            system,
            pid,
            cancellation_token,
        }
    }

    fn add_file(&mut self, path: PathBuf) {
        self.file_count += 1;
        self.file_paths.push(path);
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
        self.total_read_dir_time += duration;
        self.total_dir_entries += entry_count;
    }

    fn update_progress(&mut self) {
        if let Some(pb) = &self.progress_bar {
            let elapsed = self.start_time.elapsed().as_secs_f64();
            if elapsed <= 0.0 {
                return;
            }

            let files_per_second = self.file_count as f64 / elapsed;

            // Format file count with max files if limit is set
            let files_display = if self.max_files == usize::MAX {
                self.file_count.to_string()
            } else {
                format!("{}/{}", self.file_count, self.max_files)
            };

            let message = if let (Some(system), Some(pid)) = (&mut self.system, &self.pid) {
                system.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[*pid]), false);
                match system.process(*pid) {
                    Some(process) => {
                        let memory_mb = process.memory() as f64 / types::BYTES_IN_MEGABYTE as f64;
                        let cpu_usage = process.cpu_usage();
                        format!(
                            "{} files | {} dirs | {:.0} files/s | {:.2} MiB memory usage | {:.2}% CPU usage",
                            files_display, self.dir_count, files_per_second, memory_mb, cpu_usage
                        )
                    }
                    None => {
                        format!(
                            "{} files | {} dirs | {:.0} files/s",
                            files_display, self.dir_count, files_per_second
                        )
                    }
                }
            } else {
                format!(
                    "{} files | {} dirs | {:.0} files/s",
                    files_display, self.dir_count, files_per_second
                )
            };

            pb.set_message(message);
        }
    }

    fn finalize(self) -> FinalizedTraversal {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if let Some(pb) = &self.progress_bar {
            pb.finish_and_clear();
        }

        FinalizedTraversal {
            file_count: self.file_count,
            dir_count: self.dir_count,
            symlink_skipped_count: self.symlink_skipped_count,
            symlink_traversed_count: self.symlink_traversed_count,
            duration: elapsed,
            file_paths: self.file_paths,
            total_read_dir_time: self.total_read_dir_time,
            total_dir_entries: self.total_dir_entries,
        }
    }

    /// Traverses and returns whether we completed successfully (Some) or were interrupted (None)
    pub async fn traverse_path_with_result(
        &mut self,
        path: &Path,
    ) -> Result<Option<TraversalResult>> {
        if path.is_dir() {
            let result = self.traverse_directory(path).await?;
            match result {
                TraversalResult::Interrupted => Ok(None),
                other => Ok(Some(other)),
            }
        } else {
            Ok(Some(TraversalResult::Continue))
        }
    }

    #[async_recursion]
    async fn traverse_directory(&mut self, path: &Path) -> Result<TraversalResult> {
        // Check for cancellation at the start of each directory traversal
        if self.cancellation_token.is_cancelled() {
            eprintln!(
                "Directory traversal cancelled at dir_count={}, file_count={}",
                self.dir_count, self.file_count
            );
            return Ok(TraversalResult::Interrupted);
        }

        self.add_dir();

        let start_time = Instant::now();
        let read_dir_result = fs::read_dir(path);
        let read_dir_duration = start_time.elapsed();

        let entries = read_dir_result?;
        let mut entry_count = 0;

        for entry_result in entries {
            // Check for cancellation while iterating on director entries
            if self.cancellation_token.is_cancelled() {
                self.add_read_dir_stats(read_dir_duration, entry_count);
                return Ok(TraversalResult::Interrupted);
            }

            let entry = entry_result?;
            let path = entry.path();
            let file_type = entry.file_type()?;
            entry_count += 1;

            if file_type.is_dir() {
                if file_type.is_symlink() {
                    if self.follow_symlinks {
                        self.add_traversed_symlink();
                        match self.traverse_directory(&path).await? {
                            TraversalResult::LimitReached => {
                                self.add_read_dir_stats(read_dir_duration, entry_count);
                                return Ok(TraversalResult::LimitReached);
                            }
                            TraversalResult::Interrupted => {
                                self.add_read_dir_stats(read_dir_duration, entry_count);
                                return Ok(TraversalResult::Interrupted);
                            }
                            TraversalResult::Continue => {}
                        }
                    } else {
                        self.add_skipped_symlink();
                    }
                } else {
                    match self.traverse_directory(&path).await? {
                        TraversalResult::LimitReached => {
                            self.add_read_dir_stats(read_dir_duration, entry_count);
                            return Ok(TraversalResult::LimitReached);
                        }
                        TraversalResult::Interrupted => {
                            self.add_read_dir_stats(read_dir_duration, entry_count);
                            return Ok(TraversalResult::Interrupted);
                        }
                        TraversalResult::Continue => {}
                    }
                }
            } else if file_type.is_file() {
                if self.file_count < self.max_files {
                    self.add_file(path);
                } else {
                    self.add_read_dir_stats(read_dir_duration, entry_count);
                    return Ok(TraversalResult::LimitReached);
                }
            }
        }
        self.add_read_dir_stats(read_dir_duration, entry_count);
        Ok(TraversalResult::Continue)
    }
}

pub async fn bench_traversal_thrift_read(
    dir_path: &str,
    max_files: usize,
    follow_symlinks: bool,
    no_progress: bool,
    resource_usage: bool,
    skip_read: bool,
    fbsource_path: Option<&str>,
) -> Result<Benchmark> {
    let path = Path::new(dir_path);
    if !path.exists() || !path.is_dir() {
        return Err(anyhow::anyhow!("Invalid directory path: {}", dir_path));
    }

    // Set up signal handling for graceful interruption
    let cancellation_token = setup_cancellation();

    let mut in_progress_traversal = InProgressTraversal::new(
        no_progress,
        resource_usage,
        max_files,
        follow_symlinks,
        cancellation_token.clone(),
    );

    let _traversal_result = if let Some(result) = in_progress_traversal
        .traverse_path_with_result(path)
        .await?
    {
        result
    } else {
        // Interrupted during traversal - return early with just traversal metrics
        let ft = in_progress_traversal.finalize();
        return build_traversal_only_benchmark(ft);
    };

    let ft = in_progress_traversal.finalize();

    let avg_read_dir_latency = if ft.dir_count > 0 {
        ft.total_read_dir_time.as_secs_f64() * 1000.0 / ft.dir_count as f64
    } else {
        0.0
    };

    let avg_dir_size = if ft.dir_count > 0 {
        ft.total_dir_entries as f64 / ft.dir_count as f64
    } else {
        0.0
    };

    if ft.duration <= 0.0 {
        return Err(anyhow::anyhow!("Duration is less or equal to zero."));
    }

    let files_per_second = ft.file_count as f64 / ft.duration;

    let mut result = Benchmark::new(BenchmarkType::FsTraversal);

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
        ft.file_count as f64,
        types::Unit::Files,
        Some(0),
    );
    result.add_metric(
        "Total directories",
        ft.dir_count as f64,
        types::Unit::Dirs,
        Some(0),
    );
    result.add_metric(
        "Total symlinks skipped",
        ft.symlink_skipped_count as f64,
        types::Unit::Symlinks,
        Some(0),
    );
    result.add_metric(
        "Total symlinks traversed",
        ft.symlink_traversed_count as f64,
        types::Unit::Symlinks,
        Some(0),
    );

    // Return early if skip_read is true
    if skip_read {
        return Ok(result);
    }

    let read_progress = if no_progress {
        None
    } else {
        let pb = ProgressBar::new(ft.file_count as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {pos}/{len} files | {msg}")
                .unwrap(),
        );
        Some(pb)
    };

    let mut agg_read_dur = std::time::Duration::new(0, 0);
    let mut total_bytes_read: u64 = 0;
    let mut successful_reads = 0;

    let client = get_edenfs_instance().get_client();
    for path in ft.file_paths {
        // Check for cancellation during file reading
        if cancellation_token.is_cancelled() {
            if let Some(pb) = &read_progress {
                pb.finish_and_clear(); // Immediate cleanup
            }
            break;
        }

        if !path.is_file() {
            if let Some(pb) = &read_progress {
                pb.inc(1);
            }
            continue;
        }

        // Yield control occasionally to allow signal processing
        if successful_reads % 1000 == 0 {
            tokio::task::yield_now().await;
        }

        let start = Instant::now();
        let fbsource_path = fbsource_path
            .ok_or_else(|| anyhow::anyhow!("fbsource path is required for thrift IO"))?;

        // Convert both paths to absolute paths
        let repo_path = PathBuf::from(fbsource_path)
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("Failed to canonicalize fbsource path: {}", e))?;
        let abs_path = path
            .canonicalize()
            .map_err(|e| anyhow::anyhow!("Failed to canonicalize file path: {}", e))?;

        // Now strip the prefix using absolute paths
        let rel_file_path = abs_path
            .strip_prefix(&repo_path)
            .map_err(|_| {
                anyhow::anyhow!(
                    "File path does not start with fbsource path (after canonicalization)"
                )
            })?
            .to_path_buf();

        let request = get_thrift_request(repo_path, rel_file_path)?;
        let response = client
            .with_thrift(|thrift| {
                (
                    thrift.getFileContent(&request),
                    EdenThriftMethod::GetFileContent,
                )
            })
            .await?;
        agg_read_dur += start.elapsed();

        match response.blob {
            ScmBlobOrError::blob(blob) => {
                total_bytes_read += blob.len() as u64;
                if let Some(pb) = &read_progress {
                    pb.inc(1);
                }
                successful_reads += 1;
            }
            ScmBlobOrError::error(_) => {
                if let Some(pb) = &read_progress {
                    pb.inc(1);
                }
            }
            ScmBlobOrError::UnknownField(_) => {}
        }

        if agg_read_dur.as_secs_f64() > 0.0 && read_progress.is_some() {
            if let Some(pb) = &read_progress {
                pb.set_message(format!(
                    "{:.2} MiB/s",
                    total_bytes_read as f64
                        / types::BYTES_IN_MEGABYTE as f64
                        / agg_read_dur.as_secs_f64()
                ));
            }
        }
    }
    if let Some(pb) = read_progress {
        pb.finish_and_clear();
    }

    if successful_reads == 0 {
        if cancellation_token.is_cancelled() {
            eprintln!("No files were read before interruption - showing traversal-only results");
            return Ok(result);
        } else {
            return Err(anyhow::anyhow!("No files were successfully read."));
        }
    }

    let avg_read_dur = agg_read_dur.as_secs_f64() / successful_reads as f64;
    let avg_file_size = total_bytes_read as f64 / successful_reads as f64;
    let avg_file_size_kb = avg_file_size / types::BYTES_IN_KILOBYTE as f64;

    let mb_per_second =
        total_bytes_read as f64 / types::BYTES_IN_MEGABYTE as f64 / agg_read_dur.as_secs_f64();

    result.add_metric("Throughput ", mb_per_second, types::Unit::MiBps, Some(2));
    result.add_metric(
        "Total files read",
        successful_reads as f64,
        types::Unit::Files,
        Some(0),
    );
    result.add_metric(
        "Average thrift read latency",
        avg_read_dur * 1000.0,
        types::Unit::Ms,
        Some(4),
    );
    result.add_metric(
        "Average file size",
        avg_file_size_kb,
        types::Unit::KiB,
        Some(2),
    );
    result.add_metric(
        "Total data read",
        total_bytes_read as f64 / types::BYTES_IN_MEGABYTE as f64,
        types::Unit::MiB,
        Some(2),
    );

    Ok(result)
}

/// Runs the filesystem traversal benchmark and returns the benchmark results
pub async fn bench_traversal_fs_read(
    dir_path: &str,
    max_files: usize,
    follow_symlinks: bool,
    no_progress: bool,
    resource_usage: bool,
    skip_read: bool,
) -> Result<Benchmark> {
    let path = Path::new(dir_path);
    if !path.exists() || !path.is_dir() {
        return Err(anyhow::anyhow!("Invalid directory path: {}", dir_path));
    }

    // Set up signal handling for graceful interruption
    let cancellation_token = setup_cancellation();

    let mut in_progress_traversal = InProgressTraversal::new(
        no_progress,
        resource_usage,
        max_files,
        follow_symlinks,
        cancellation_token.clone(),
    );

    let _traversal_result = if let Some(result) = in_progress_traversal
        .traverse_path_with_result(path)
        .await?
    {
        result
    } else {
        // Interrupted during traversal - return early with just traversal metrics
        let ft = in_progress_traversal.finalize();
        return build_traversal_only_benchmark(ft);
    };

    let ft = in_progress_traversal.finalize();

    let avg_read_dir_latency = if ft.dir_count > 0 {
        ft.total_read_dir_time.as_secs_f64() * 1000.0 / ft.dir_count as f64
    } else {
        0.0
    };

    let avg_dir_size = if ft.dir_count > 0 {
        ft.total_dir_entries as f64 / ft.dir_count as f64
    } else {
        0.0
    };

    if ft.duration <= 0.0 {
        return Err(anyhow::anyhow!("Duration is less or equal to zero."));
    }

    let files_per_second = ft.file_count as f64 / ft.duration;

    let mut result = Benchmark::new(BenchmarkType::FsTraversal);

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
        ft.file_count as f64,
        types::Unit::Files,
        Some(0),
    );
    result.add_metric(
        "Total symlinks skipped",
        ft.symlink_skipped_count as f64,
        types::Unit::Symlinks,
        Some(0),
    );
    result.add_metric(
        "Total symlinks traversed",
        ft.symlink_traversed_count as f64,
        types::Unit::Symlinks,
        Some(0),
    );
    result.add_metric(
        "Total directories",
        ft.dir_count as f64,
        types::Unit::Dirs,
        Some(0),
    );

    // Return early if skip_read is true
    if skip_read {
        return Ok(result);
    }

    let read_progress = if no_progress {
        None
    } else {
        let pb = ProgressBar::new(ft.file_count as u64);
        pb.set_style(
            ProgressStyle::default_bar()
                .template("[{elapsed_precise}] {pos}/{len} files | {msg}")
                .unwrap(),
        );
        Some(pb)
    };

    let mut agg_open_dur = std::time::Duration::new(0, 0);
    let mut agg_read_dur = std::time::Duration::new(0, 0);
    let mut total_bytes_read: u64 = 0;
    let mut successful_reads = 0;
    let mut buffer = Vec::with_capacity(types::BYTES_IN_MEGABYTE);

    for path in ft.file_paths {
        // Check for cancellation during file reading
        if cancellation_token.is_cancelled() {
            if let Some(pb) = &read_progress {
                pb.finish_and_clear(); // Immediate cleanup
            }
            break;
        }

        if !path.is_file() {
            if let Some(pb) = &read_progress {
                pb.inc(1);
            }
            continue;
        }

        // Yield control occasionally to allow signal processing
        if successful_reads % 1000 == 0 {
            tokio::task::yield_now().await;
        }

        let start = Instant::now();
        let mut file = match File::open(path) {
            Ok(f) => f,
            Err(_) => {
                if let Some(pb) = &read_progress {
                    pb.inc(1);
                }
                continue;
            }
        };
        agg_open_dur += start.elapsed();

        let start = Instant::now();
        if let Ok(bytes_read) = file.read_to_end(&mut buffer) {
            total_bytes_read += bytes_read as u64;
            successful_reads += 1;
        }
        agg_read_dur += start.elapsed();
        buffer.clear();
        if let Some(pb) = &read_progress {
            pb.inc(1);
        }

        if agg_read_dur.as_secs_f64() > 0.0 && read_progress.is_some() {
            if let Some(pb) = &read_progress {
                pb.set_message(format!(
                    "{:.2} MiB/s",
                    total_bytes_read as f64
                        / types::BYTES_IN_MEGABYTE as f64
                        / agg_read_dur.as_secs_f64()
                ));
            }
        }
    }

    if let Some(pb) = read_progress {
        pb.finish_and_clear();
    }

    if successful_reads == 0 {
        if cancellation_token.is_cancelled() {
            eprintln!("No files were read before interruption - showing traversal-only results");
            return Ok(result);
        } else {
            return Err(anyhow::anyhow!("No files were successfully read."));
        }
    }

    let avg_open_dur = agg_open_dur.as_secs_f64() / successful_reads as f64;
    let avg_read_dur = agg_read_dur.as_secs_f64() / successful_reads as f64;
    let avg_file_size = total_bytes_read as f64 / successful_reads as f64;
    let avg_file_size_kb = avg_file_size / types::BYTES_IN_KILOBYTE as f64;

    let total_duration = (agg_open_dur + agg_read_dur).as_secs_f64();
    let mb_per_second = total_bytes_read as f64 / types::BYTES_IN_MEGABYTE as f64 / total_duration;

    result.add_metric(
        "open() + read() throughput ",
        mb_per_second,
        types::Unit::MiBps,
        Some(2),
    );
    result.add_metric(
        "Total files read",
        successful_reads as f64,
        types::Unit::Files,
        Some(0),
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
    result.add_metric(
        "Average file size",
        avg_file_size_kb,
        types::Unit::KiB,
        Some(2),
    );
    result.add_metric(
        "Total data read",
        total_bytes_read as f64 / types::BYTES_IN_MEGABYTE as f64,
        types::Unit::MiB,
        Some(2),
    );

    Ok(result)
}

pub fn get_thrift_request(
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
