/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Database I/O benchmarking

use std::time::Instant;

use anyhow::Result;

use super::r#gen::RandomData;
use super::r#gen::TestDir;
use super::types;
use super::types::Benchmark;
use super::types::BenchmarkType;

/// Runs the SQLite write benchmark and returns the benchmark results
pub fn bench_sqlite_write_mfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
    let conn = rusqlite::Connection::open(test_dir.sqlite_path())?;
    conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA synchronous = OFF;")?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS data (key BLOB PRIMARY KEY, value BLOB NOT NULL)",
        [],
    )?;
    let mut stmt = conn.prepare("INSERT INTO data (key, value) VALUES (?, ?)")?;
    let keys = random_data.keys();
    let mut agg_write_dur = std::time::Duration::new(0, 0);
    for (chunk, key) in random_data.chunks.iter().zip(keys.iter()) {
        let start = Instant::now();
        stmt.execute(rusqlite::params![key, chunk])?;
        agg_write_dur += start.elapsed();
    }
    let avg_write_dur = agg_write_dur.as_secs_f64() / random_data.number_of_files as f64;
    let mb_per_second =
        random_data.chunk_size as f64 / avg_write_dur / types::BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new(BenchmarkType::SqliteWriteMultipleFiles);

    // Add throughput metrics
    result.add_metric("write()", mb_per_second, types::Unit::MiBps, None);

    // Add latency metrics
    let chunk_size_kb = random_data.chunk_size as f64 / types::BYTES_IN_KILOBYTE as f64;
    result.add_metric(
        &format!("write() {chunk_size_kb:.0} KiB latency"),
        avg_write_dur * 1000.0,
        types::Unit::Ms,
        Some(4),
    );

    Ok(result)
}

/// Runs the SQLite read benchmark and returns the benchmark results
pub fn bench_sqlite_read_mfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
    let conn = rusqlite::Connection::open(test_dir.sqlite_path())?;
    let mut stmt = conn.prepare("SELECT value FROM data WHERE key = ?")?;
    let keys = random_data.keys();
    let mut read_data = vec![0u8; random_data.chunk_size];
    let mut agg_read_dur = std::time::Duration::new(0, 0);
    for key in keys {
        let start = Instant::now();
        stmt.query_row(rusqlite::params![key], |row| {
            let value: Vec<u8> = row.get(0)?;
            read_data.copy_from_slice(&value);
            Ok(())
        })?;
        agg_read_dur += start.elapsed();
    }
    let avg_read_dur = agg_read_dur.as_secs_f64() / random_data.number_of_files as f64;
    let mb_per_second =
        random_data.chunk_size as f64 / avg_read_dur / types::BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new(BenchmarkType::SqliteReadMultipleFiles);

    // Add throughput metrics
    result.add_metric("read()", mb_per_second, types::Unit::MiBps, None);

    // Add latency metrics
    let chunk_size_kb = random_data.chunk_size as f64 / types::BYTES_IN_KILOBYTE as f64;
    result.add_metric(
        &format!("read() {chunk_size_kb:.0} KiB latency"),
        avg_read_dur * 1000.0,
        types::Unit::Ms,
        Some(4),
    );

    Ok(result)
}
