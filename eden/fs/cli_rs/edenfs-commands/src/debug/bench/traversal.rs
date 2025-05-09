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
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use thrift_types::edenfs::ScmBlobOrError;

use super::fsio::get_thrift_request;
use super::fsio::split_fbsource_file_path;
use super::types;
use super::types::Benchmark;
use super::types::BenchmarkType;
use crate::get_edenfs_instance;

struct TraversalProgress {
    file_count: usize,
    start_time: Instant,
    progress_bar: Option<ProgressBar>,
    file_paths: Vec<PathBuf>,
}

impl TraversalProgress {
    fn new(no_progress: bool) -> Self {
        let progress_bar = if no_progress {
            None
        } else {
            let pb = ProgressBar::new_spinner();
            pb.set_style(
                ProgressStyle::default_spinner()
                    .template("[{elapsed_precise}] {msg}")
                    .unwrap(),
            );
            pb.set_message("0 files | 0 files/s");
            Some(pb)
        };

        Self {
            file_count: 0,
            start_time: Instant::now(),
            progress_bar,
            file_paths: Vec::new(),
        }
    }

    fn add_file(&mut self, path: PathBuf) {
        self.file_count += 1;
        self.file_paths.push(path);
        if self.file_count % 100 == 0 {
            self.update_progress();
        }
    }

    fn update_progress(&mut self) {
        if let Some(pb) = &self.progress_bar {
            let elapsed = self.start_time.elapsed().as_secs_f64();
            if elapsed <= 0.0 {
                return;
            }
            let files_per_second = self.file_count as f64 / elapsed;
            pb.set_message(format!(
                "{} files | {:.0} files/s",
                self.file_count, files_per_second
            ));
        }
    }

    fn finalize(&self) -> (usize, f64, &Vec<PathBuf>) {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if let Some(pb) = &self.progress_bar {
            pb.finish_and_clear();
        }
        (self.file_count, elapsed, &self.file_paths)
    }
}

/// Recursively traverses a directory and collects file paths, displaying progress
///
/// If follow_symlinks is true, symbolic links will be followed during traversal.
/// Otherwise, symbolic links will be ignored.
fn traverse_directory(
    path: &Path,
    metrics: &mut TraversalProgress,
    follow_symlinks: bool,
) -> Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();

            if path.is_dir() {
                if !path.is_symlink() || follow_symlinks {
                    traverse_directory(&path, metrics, follow_symlinks)?;
                }
            } else if path.is_file() {
                metrics.add_file(path);
            }
        }
    }
    Ok(())
}

pub async fn bench_traversal_thrift_read(
    dir_path: &str,
    follow_symlinks: bool,
    no_progress: bool,
) -> Result<Benchmark> {
    let path = Path::new(dir_path);
    if !path.exists() || !path.is_dir() {
        return Err(anyhow::anyhow!("Invalid directory path: {}", dir_path));
    }

    let mut traverse_progress = TraversalProgress::new(no_progress);
    traverse_directory(path, &mut traverse_progress, follow_symlinks)?;
    let (file_count, duration, file_paths) = traverse_progress.finalize();

    if duration <= 0.0 {
        return Err(anyhow::anyhow!("Duration is less or equal to zero."));
    }

    let files_per_second = file_count as f64 / duration;

    let mut result = Benchmark::new(BenchmarkType::FsTraversal);

    result.add_metric(
        "Traversal throughput",
        files_per_second,
        types::Unit::FilesPerSecond,
        Some(0),
    );
    result.add_metric(
        "Total files",
        file_count as f64,
        types::Unit::Files,
        Some(0),
    );

    let read_progress = if no_progress {
        None
    } else {
        let pb = ProgressBar::new(file_count as u64);
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
    for path in file_paths {
        if !path.is_file() {
            if let Some(pb) = &read_progress {
                pb.inc(1);
            }
            continue;
        }

        let start = Instant::now();
        let (repo_path, rel_file_path) = split_fbsource_file_path(path);
        let request = get_thrift_request(repo_path, rel_file_path)?;
        let response = client
            .with_thrift(|thrift| thrift.getFileContent(&request))
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
        return Err(anyhow::anyhow!("No files were successfully read."));
    }

    let avg_read_dur = agg_read_dur.as_secs_f64() / successful_reads as f64;
    let avg_file_size = total_bytes_read as f64 / successful_reads as f64;
    let avg_file_size_kb = avg_file_size / types::BYTES_IN_KILOBYTE as f64;

    let mb_per_second =
        total_bytes_read as f64 / types::BYTES_IN_MEGABYTE as f64 / agg_read_dur.as_secs_f64();

    result.add_metric("Throughput ", mb_per_second, types::Unit::MiBps, Some(2));
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
pub fn bench_traversal_fs_read(
    dir_path: &str,
    follow_symlinks: bool,
    no_progress: bool,
) -> Result<Benchmark> {
    let path = Path::new(dir_path);
    if !path.exists() || !path.is_dir() {
        return Err(anyhow::anyhow!("Invalid directory path: {}", dir_path));
    }

    let mut traverse_progress = TraversalProgress::new(no_progress);

    traverse_directory(path, &mut traverse_progress, follow_symlinks)?;

    let (file_count, duration, file_paths) = traverse_progress.finalize();

    if duration <= 0.0 {
        return Err(anyhow::anyhow!("Duration is less or equal to zero."));
    }

    let files_per_second = file_count as f64 / duration;

    let mut result = Benchmark::new(BenchmarkType::FsTraversal);

    result.add_metric(
        "Traversal throughput",
        files_per_second,
        types::Unit::FilesPerSecond,
        Some(0),
    );
    result.add_metric(
        "Total files",
        file_count as f64,
        types::Unit::Files,
        Some(0),
    );

    let read_progress = if no_progress {
        None
    } else {
        let pb = ProgressBar::new(file_count as u64);
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
    let mut buffer = Vec::new();

    for path in file_paths {
        if !path.is_file() {
            if let Some(pb) = &read_progress {
                pb.inc(1);
            }
            continue;
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
        return Err(anyhow::anyhow!("No files were successfully read."));
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
