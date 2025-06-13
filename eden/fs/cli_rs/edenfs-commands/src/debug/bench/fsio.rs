/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Filesystem I/O benchmarking

use std::fs::File;
use std::io::Read;
use std::io::Write;
#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;
#[cfg(target_os = "linux")]
use std::process::Command;
use std::time::Instant;

use anyhow::Result;
#[cfg(target_os = "linux")]
use anyhow::anyhow;

use super::r#gen::RandomData;
use super::r#gen::TestDir;
use super::types;
use super::types::Benchmark;
use super::types::BenchmarkType;

/// Runs the MFMD write benchmark and returns the benchmark results
pub fn bench_write_mfmd(
    test_dir: &TestDir,
    random_data: &RandomData,
    no_caches: bool,
) -> Result<Benchmark> {
    let mut agg_create_dur = std::time::Duration::new(0, 0);
    let mut agg_write_dur = std::time::Duration::new(0, 0);
    for (chunk, path) in random_data
        .chunks
        .iter()
        .zip(random_data.paths(test_dir).iter())
    {
        let start = Instant::now();
        let mut file = File::create(path)?;
        agg_create_dur += start.elapsed();

        let start = Instant::now();
        file.write_all(chunk)?;
        agg_write_dur += start.elapsed();
    }

    let mut agg_sync_dur = std::time::Duration::new(0, 0);
    for path in random_data.paths(test_dir) {
        let start = Instant::now();
        let file = File::options().write(true).open(path)?;
        file.sync_all()?;
        agg_sync_dur += start.elapsed();
    }

    let avg_create_dur = agg_create_dur.as_secs_f64() / random_data.number_of_files as f64;
    let avg_write_dur = agg_write_dur.as_secs_f64() / random_data.number_of_files as f64;
    let avg_sync_dur = agg_sync_dur.as_secs_f64() / random_data.number_of_files as f64;
    let avg_e2e_dur = avg_create_dur + avg_write_dur + avg_sync_dur;
    let avg_create_write_dur = avg_create_dur + avg_write_dur;
    let mb_per_second_e2e =
        random_data.chunk_size as f64 / avg_e2e_dur / types::BYTES_IN_MEGABYTE as f64;
    let mb_per_second_create_write =
        random_data.chunk_size as f64 / avg_create_write_dur / types::BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new(BenchmarkType::FsWriteMultipleFiles);

    // Add throughput metrics
    result.add_metric(
        "create() + write() + sync() throughput",
        mb_per_second_e2e,
        types::Unit::MiBps,
        None,
    );
    result.add_metric(
        "create() + write() throughput",
        mb_per_second_create_write,
        types::Unit::MiBps,
        None,
    );

    // Add latency metrics
    result.add_metric(
        "create() latency",
        avg_create_dur * 1000.0,
        types::Unit::Ms,
        Some(4),
    );

    let chunk_size_kb = random_data.chunk_size as f64 / types::BYTES_IN_KILOBYTE as f64;
    result.add_metric(
        &format!("write() {:.0} KiB latency", chunk_size_kb),
        avg_write_dur * 1000.0,
        types::Unit::Ms,
        Some(4),
    );
    result.add_metric(
        &format!("sync() {:.0} KiB latency", chunk_size_kb),
        avg_sync_dur * 1000.0,
        types::Unit::Ms,
        Some(4),
    );

    #[cfg(target_os = "linux")]
    {
        if no_caches {
            if let Err(e) = drop_kernel_caches() {
                eprintln!("\nFailed to drop caches: {}\n", e);
            } else {
                println!("\nCaches dropped successfully after writes\n");
            }
        }
    }

    Ok(result)
}

/// Runs the MFMD read benchmark and returns the benchmark results
pub fn bench_fs_read_mfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
    let mut agg_open_dur = std::time::Duration::new(0, 0);
    let mut agg_read_dur = std::time::Duration::new(0, 0);
    let mut read_data = vec![0u8; random_data.chunk_size];
    for path in random_data.paths(test_dir) {
        let start = Instant::now();
        let mut file = File::open(path)?;
        agg_open_dur += start.elapsed();

        let start = Instant::now();
        file.read_exact(&mut read_data)?;
        agg_read_dur += start.elapsed();
    }
    let avg_open_dur = agg_open_dur.as_secs_f64() / random_data.number_of_files as f64;
    let avg_read_dur = agg_read_dur.as_secs_f64() / random_data.number_of_files as f64;
    let avg_dur = avg_open_dur + avg_read_dur;
    let mb_per_second = random_data.chunk_size as f64 / avg_dur / types::BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new(BenchmarkType::FsReadMultipleFiles);

    // Add throughput metrics
    result.add_metric(
        "open() + read() throughput",
        mb_per_second,
        types::Unit::MiBps,
        None,
    );

    // Add latency metrics
    result.add_metric(
        "open() latency",
        avg_open_dur * 1000.0,
        types::Unit::Ms,
        Some(4),
    );

    let chunk_size_kb = random_data.chunk_size as f64 / types::BYTES_IN_KILOBYTE as f64;
    result.add_metric(
        &format!("read() {:.0} KiB latency", chunk_size_kb),
        avg_read_dur * 1000.0,
        types::Unit::Ms,
        Some(4),
    );

    result.add_metric(
        &format!("total {:.0} KiB latency", chunk_size_kb),
        (avg_read_dur + avg_open_dur) * 1000.0,
        types::Unit::Ms,
        Some(4),
    );

    Ok(result)
}

/// Runs the SFMD write benchmark and returns the benchmark results
pub fn bench_write_sfmd(
    test_dir: &TestDir,
    random_data: &RandomData,
    no_caches: bool,
) -> Result<Benchmark> {
    let start = Instant::now();
    let mut file = File::create(test_dir.combined_data_path())?;
    for chunk in &random_data.chunks {
        file.write_all(chunk)?;
    }
    let write_dur = start.elapsed().as_secs_f64();
    let start = Instant::now();
    file.sync_all()?;
    let sync_dur = start.elapsed().as_secs_f64();
    let agg_dur = write_dur + sync_dur;
    let mb_per_second_e2e =
        random_data.total_size() as f64 / types::BYTES_IN_MEGABYTE as f64 / agg_dur;
    let mb_per_second_write =
        random_data.total_size() as f64 / types::BYTES_IN_MEGABYTE as f64 / write_dur;

    let mut result = Benchmark::new(BenchmarkType::FsWriteSingleFile);

    // Add throughput metrics
    result.add_metric(
        "create() + write() + sync() throughput",
        mb_per_second_e2e,
        types::Unit::MiBps,
        None,
    );
    result.add_metric(
        "create() + write()",
        mb_per_second_write,
        types::Unit::MiBps,
        None,
    );

    #[cfg(target_os = "linux")]
    {
        if no_caches {
            if let Err(e) = drop_kernel_caches() {
                eprintln!("\nFailed to drop caches: {}\n", e);
            } else {
                println!("\nCaches dropped successfully after writes\n");
            }
        }
    }

    Ok(result)
}

/// Runs the SFMD read benchmark and returns the benchmark results
pub fn bench_fs_read_sfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
    let file_path = test_dir.combined_data_path();
    let mut read_data = Vec::with_capacity(random_data.total_size());
    let start = Instant::now();
    let mut file = File::open(&file_path)?;
    file.read_to_end(&mut read_data)?;
    let agg_dur = start.elapsed().as_secs_f64();
    let mb_per_second = read_data.len() as f64 / types::BYTES_IN_MEGABYTE as f64 / agg_dur;

    let mut result = Benchmark::new(BenchmarkType::FsReadSingleFile);

    // Add throughput metrics
    result.add_metric(
        "open() + read() throughput",
        mb_per_second,
        types::Unit::MiBps,
        None,
    );

    Ok(result)
}

#[cfg(target_os = "linux")]
fn drop_kernel_caches() -> Result<()> {
    let status = Command::new("sh")
        .arg("-c")
        .arg("echo 3 > /proc/sys/vm/drop_caches")
        .uid(0)
        .gid(0)
        .status()
        .map_err(|e| anyhow!("Failed to execute shell: {}", e))?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow!("Failed to drop caches: {}", status))
    }
}
