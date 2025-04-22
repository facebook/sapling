/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Filesystem traversal benchmarking

use std::fs;
use std::path::Path;
use std::time::Instant;

use anyhow::Result;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;

use super::types::Benchmark;
use super::types::BenchmarkType;

struct TraversalProgress {
    file_count: usize,
    start_time: Instant,
    progress_bar: ProgressBar,
}

impl TraversalProgress {
    fn new() -> Self {
        let progress_bar = ProgressBar::new_spinner();
        progress_bar.set_style(
            ProgressStyle::default_spinner()
                .template("[{elapsed_precise}] {msg}")
                .unwrap(),
        );
        progress_bar.set_message("0 files | 0 files/s");

        Self {
            file_count: 0,
            start_time: Instant::now(),
            progress_bar,
        }
    }

    fn increment_count(&mut self) {
        self.file_count += 1;
        self.update_progress();
    }

    fn update_progress(&mut self) {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        if elapsed <= 0.0 {
            return;
        }
        let files_per_second = self.file_count as f64 / elapsed;
        self.progress_bar.set_message(format!(
            "{} files | {:.0} files/s",
            self.file_count, files_per_second
        ));
    }

    fn finalize(&self) -> (usize, f64) {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        self.progress_bar.finish_and_clear();
        (self.file_count, elapsed)
    }
}

/// Recursively traverses a directory and counts files, displaying progress
fn traverse_directory(path: &Path, metrics: &mut TraversalProgress) -> Result<()> {
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                traverse_directory(&path, metrics)?;
            } else {
                metrics.increment_count();
            }
        }
    }
    Ok(())
}

/// Runs the filesystem traversal benchmark and returns the benchmark results
pub fn bench_fs_traversal(dir_path: &str) -> Result<Benchmark> {
    let path = Path::new(dir_path);
    if !path.exists() || !path.is_dir() {
        return Err(anyhow::anyhow!("Invalid directory path: {}", dir_path));
    }

    let mut progress = TraversalProgress::new();

    traverse_directory(path, &mut progress)?;

    let (file_count, duration) = progress.finalize();

    if duration <= 0.0 {
        return Err(anyhow::anyhow!("Duration is less or equal to zero."));
    }

    let files_per_second = file_count as f64 / duration;

    let mut result = Benchmark::new(BenchmarkType::FsTraversal);

    result.add_metric("Traversal throughput", files_per_second, "files/s", Some(0));
    result.add_metric("Total files", file_count as f64, "files", Some(0));
    result.add_metric("Total time", duration, "seconds", Some(2));

    Ok(result)
}
