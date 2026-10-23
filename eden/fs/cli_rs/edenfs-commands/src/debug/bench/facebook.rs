/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! The benchmarks that need internal crates. oss.rs provides the same
//! interface for open-source builds.

use std::time::Instant;

use anyhow::Result;
use anyhow::anyhow;
use lmdb::Txn;

use super::r#gen::RandomData;
use super::r#gen::TestDir;
use super::types;
use super::types::Benchmark;
use super::types::BenchmarkType;

pub fn run_lmdb_benchmarks(test_dir: &TestDir, random_data: &RandomData) -> Result<()> {
    println!("{}", bench_lmdb_write_mfmd(test_dir, random_data)?);
    println!("{}", bench_lmdb_read_mfmd(test_dir, random_data)?);
    Ok(())
}

/// Runs the LMDB write benchmark and returns the benchmark results
fn bench_lmdb_write_mfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
    let mut agg_write_dur = std::time::Duration::new(0, 0);
    let env = lmdb::Env::options()?
        .set_mapsize(4 * random_data.total_size())?
        .set_nosync(true)
        .set_nordahead(true)
        .create_file(test_dir.lmdb_path(), 0o644)?;
    let db: lmdb::TypedDb<lmdb::VecU8> = lmdb::TypedDb::create(&env, None)?;
    let keys = random_data.keys();
    for (chunk, key) in random_data.chunks.iter().zip(keys.iter()) {
        let start = Instant::now();
        let mut txn = env.rw_begin()?;
        db.put(&mut txn, key, chunk)?;
        txn.commit()?;
        agg_write_dur += start.elapsed();
    }
    let start = Instant::now();
    env.sync(true)?;
    let agg_sync_dur = start.elapsed();
    let avg_write_dur = agg_write_dur.as_secs_f64() / random_data.number_of_files as f64;
    let avg_sync_dur = agg_sync_dur.as_secs_f64() / random_data.number_of_files as f64;
    let avg_dur = avg_write_dur + avg_sync_dur;
    let mb_per_second_e2e =
        random_data.chunk_size as f64 / avg_dur / types::BYTES_IN_MEGABYTE as f64;
    let mb_per_second_write =
        random_data.chunk_size as f64 / avg_write_dur / types::BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new(BenchmarkType::LmdbWriteMultipleFiles);

    // Add throughput metrics
    result.add_metric(
        "write() + sync()",
        mb_per_second_e2e,
        types::Unit::MiBps,
        None,
    );
    result.add_metric("write()", mb_per_second_write, types::Unit::MiBps, None);

    // Add latency metrics
    let chunk_size_kb = random_data.chunk_size as f64 / types::BYTES_IN_KILOBYTE as f64;
    result.add_metric(
        &format!("write() {chunk_size_kb:.0} KiB latency"),
        avg_write_dur * 1000.0,
        types::Unit::Ms,
        Some(4),
    );
    result.add_metric(
        "sync() latency",
        avg_sync_dur * 1000.0,
        types::Unit::Ms,
        Some(4),
    );

    Ok(result)
}

/// Runs the LMDB read benchmark and returns the benchmark results
fn bench_lmdb_read_mfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
    let mut agg_read_dur = std::time::Duration::new(0, 0);
    let mut read_data = vec![0u8; random_data.chunk_size];
    let env = lmdb::Env::options()?
        .set_nordahead(true)
        .open_file(test_dir.lmdb_path(), 0o644)?;
    let db: lmdb::TypedDb<lmdb::VecU8> = lmdb::TypedDb::open(&env, None)?;
    let keys = random_data.keys();
    for key in keys {
        let start = Instant::now();
        let txn = env.ro_begin()?;
        let dbres = db.get(&txn, &key)?;
        match dbres {
            Some(value) => read_data.copy_from_slice(value),
            None => return Err(anyhow!("Data not found for key {key:?}")),
        }
        agg_read_dur += start.elapsed();
    }
    let avg_read_dur = agg_read_dur.as_secs_f64() / random_data.number_of_files as f64;
    let mb_per_second =
        random_data.chunk_size as f64 / avg_read_dur / types::BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new(BenchmarkType::LmdbReadMultipleFiles);

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
