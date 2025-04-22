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

use super::types::Benchmark;
use super::types::BenchmarkType;

/// Recursively traverses a directory and counts files
fn traverse_directory(path: &Path) -> Result<usize> {
    let mut count = 0;
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let path = entry.path();
            if path.is_dir() {
                count += traverse_directory(&path)?;
            } else {
                count += 1;
            }
        }
    }
    Ok(count)
}

/// Runs the filesystem traversal benchmark and returns the benchmark results
pub fn bench_fs_traversal(dir_path: &str) -> Result<Benchmark> {
    let path = Path::new(dir_path);
    if !path.exists() || !path.is_dir() {
        return Err(anyhow::anyhow!("Invalid directory path: {}", dir_path));
    }

    let start = Instant::now();
    let file_count = traverse_directory(path)?;
    let duration = start.elapsed().as_secs_f64();

    let files_per_second = if duration > 0.0 {
        file_count as f64 / duration
    } else {
        return Err(anyhow::anyhow!("Duration is less or requal to zero."));
    };

    let mut result = Benchmark::new(BenchmarkType::FsTraversal);

    result.add_measurement("Traversal throughput", files_per_second, "files/s", Some(0));
    result.add_measurement("Total files", file_count as f64, "files", Some(0));
    result.add_measurement("Total time", duration, "seconds", Some(2));

    Ok(result)
}
