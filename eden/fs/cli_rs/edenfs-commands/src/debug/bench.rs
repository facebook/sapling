/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl debug bench

use std::fs;
use std::fs::File;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use blake3::Hash;
use clap::Parser;
use lmdb::Txn;
use rand::RngCore;

use crate::ExitCode;

const BENCH_DIR_NAME: &str = "__fsiomicrobench__";
const ROCKSDB_FILE_NAME: &str = "__rocksdb__";
const LMDB_FILE_NAME: &str = "__lmdb__";
const SQLITE_FILE_NAME: &str = "__sqlite__";
const COMBINED_DATA_FILE_NAME: &str = "__combined_data__";
const DEFAULT_NUMBER_OF_FILES: usize = 64 * 1024;
const DEFAULT_CHUNK_SIZE: usize = 4 * 1024;
const NUMBER_OF_SUB_DIRS: usize = 256;
const BYTES_IN_KILOBYTE: usize = 1024;
const BYTES_IN_MEGABYTE: usize = 1024 * BYTES_IN_KILOBYTE;
const BYTES_IN_GIGABYTE: usize = 1024 * BYTES_IN_MEGABYTE;

/// Represents the result of a benchmark operation
#[derive(Debug, Clone)]
struct Benchmark {
    /// Name of the benchmark
    name: String,
    /// Various metrics
    metrics: Vec<Metric>,
}

/// Represents a measurement with a name, value, unit, and precision
#[derive(Debug, Clone)]
struct Metric {
    /// Name of the measurement (e.g., "write()", "write() latency")
    name: String,
    /// Value of the measurement
    value: f64,
    /// Unit of the measurement (e.g., "MiB/s", "ms")
    unit: String,
    /// Precision for display (number of decimal places)
    precision: u8,
}

impl Benchmark {
    /// Creates a new benchmark result with the given name
    fn new(name: &str) -> Self {
        Benchmark {
            name: name.to_string(),
            metrics: Vec::new(),
        }
    }

    /// Adds a measurement with optional precision (defaults to 2)
    fn add_measurement(&mut self, name: &str, value: f64, unit: &str, precision: Option<u8>) {
        self.metrics.push(Metric {
            name: name.to_string(),
            value,
            unit: unit.to_string(),
            precision: precision.unwrap_or(2),
        });
    }

    /// Displays the benchmark result
    fn display(&self) {
        let format_value_with_precision =
            |value: f64, precision: u8| -> String { format!("{:.1$}", value, precision as usize) };

        println!("{}", self.name);

        let max_value_len = self
            .metrics
            .iter()
            .map(|measurement| {
                format_value_with_precision(measurement.value, measurement.precision).len()
            })
            .max()
            .map_or(0, |len| if len < 10 { 10 } else { len });

        let max_unit_len = self
            .metrics
            .iter()
            .map(|measurement| measurement.unit.len())
            .max()
            .unwrap_or(0);

        for measurement in &self.metrics {
            let value_str = format_value_with_precision(measurement.value, measurement.precision);

            println!(
                "{:>width$} {:<unit_width$} - {}",
                value_str,
                measurement.unit,
                measurement.name,
                width = max_value_len,
                unit_width = max_unit_len
            );
        }
    }
}

#[derive(Parser, Debug)]
pub struct CommonOptions {
    /// Directory to use for testing
    #[clap(long, default_value_t = std::env::temp_dir().to_str().unwrap().to_string())]
    test_dir: String,

    /// Number of randomly generated files to use for benchmarking
    #[clap(long, default_value_t = DEFAULT_NUMBER_OF_FILES)]
    number_of_files: usize,

    /// Size of each chunk in bytes
    #[clap(long, default_value_t = DEFAULT_CHUNK_SIZE)]
    chunk_size: usize,
}

#[derive(Parser, Debug)]
#[clap(about = "Run benchmarks for EdenFS and OS-native file systems on Linux, macOS, and Windows")]
pub enum BenchCmd {
    #[clap(about = "Run filesystem I/O benchmarks")]
    FsIo {
        #[clap(flatten)]
        common: CommonOptions,
    },

    #[clap(about = "Run database I/O benchmarks")]
    DbIo {
        #[clap(flatten)]
        common: CommonOptions,
    },
}

/// TestDir represents a directory used for testing.
/// It handles creation, validation, and removal of test directories,
/// as well as generating paths for test files and databases.
struct TestDir {
    // Path to the test directory
    path: PathBuf,
}

impl TestDir {
    /// Validates and prepares a test directory.
    /// Returns a TestDir instance if successful.
    fn validate(test_dir: &str) -> Result<Self> {
        let test_dir_path = Path::new(test_dir);
        if !test_dir_path.exists() {
            return Err(anyhow!("The directory {} does not exist.", test_dir));
        }
        let bench_dir_path = test_dir_path.join(BENCH_DIR_NAME);
        if bench_dir_path.exists() {
            fs::remove_dir_all(&bench_dir_path)?;
        }
        fs::create_dir(&bench_dir_path)?;
        Self::prepare_directories(&bench_dir_path)?;
        Ok(TestDir {
            path: bench_dir_path,
        })
    }

    /// Prepares subdirectories for the test directory.
    fn prepare_directories(root: &Path) -> Result<()> {
        for i in 0..NUMBER_OF_SUB_DIRS {
            let sub_dir = format!("{:02x}", i);
            let sub_dir_path = root.join(sub_dir);
            fs::create_dir_all(&sub_dir_path)?;
        }
        Ok(())
    }

    /// Removes the test directory.
    fn remove(&self) -> Result<()> {
        if self.path.exists() {
            fs::remove_dir_all(&self.path)?;
        }
        Ok(())
    }

    /// Converts a hash to a file path within the test directory.
    fn hash_to_path(&self, hash: &Hash) -> PathBuf {
        let hash_str = hash.to_hex().to_string();
        let sub_dir = &hash_str[0..2];
        self.path.join(sub_dir).join(hash_str)
    }

    /// Returns the path to the combined data file.
    fn combined_data_path(&self) -> PathBuf {
        self.path.join(COMBINED_DATA_FILE_NAME)
    }

    /// Returns the path to the RocksDB file.
    fn rocksdb_path(&self) -> PathBuf {
        self.path.join(ROCKSDB_FILE_NAME)
    }

    /// Returns the path to the LMDB file.
    fn lmdb_path(&self) -> PathBuf {
        self.path.join(LMDB_FILE_NAME)
    }

    /// Returns the path to the SQLite file.
    fn sqlite_path(&self) -> PathBuf {
        self.path.join(SQLITE_FILE_NAME)
    }
}

struct RandomData {
    // Number of randomly generated files.
    number_of_files: usize,

    // Size of each chunk in bytes.
    chunk_size: usize,

    // Random content that will be written to files.
    chunks: Vec<Vec<u8>>,

    // Hashes to verify the data written to files.
    // Also used for generate file paths contents will be written to.
    hashes: Vec<Hash>,
}

impl RandomData {
    fn new(number_of_files: usize, chunk_size: usize) -> Self {
        let mut rng = rand::thread_rng();
        let mut chunks = Vec::with_capacity(number_of_files);
        let mut hashes = Vec::with_capacity(number_of_files);
        for _ in 0..number_of_files {
            let mut chunk = vec![0u8; chunk_size];
            rng.fill_bytes(&mut chunk);
            let hash = blake3::hash(&chunk);
            chunks.push(chunk);
            hashes.push(hash);
        }
        RandomData {
            number_of_files,
            chunk_size,
            chunks,
            hashes,
        }
    }

    fn paths(&self, test_dir: &TestDir) -> Vec<PathBuf> {
        self.hashes
            .iter()
            .map(|hash| test_dir.hash_to_path(hash))
            .collect()
    }

    fn keys(&self) -> Vec<Vec<u8>> {
        self.hashes.iter().map(|h| h.as_bytes().to_vec()).collect()
    }

    fn total_size(&self) -> usize {
        self.number_of_files * self.chunk_size
    }
}

/// Runs the MFMD write benchmark and returns the benchmark results
fn bench_write_mfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
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
    let mb_per_second_e2e = random_data.chunk_size as f64 / avg_e2e_dur / BYTES_IN_MEGABYTE as f64;
    let mb_per_second_create_write =
        random_data.chunk_size as f64 / avg_create_write_dur / BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new("MFMD Write");

    // Add throughput measurements
    result.add_measurement(
        "create() + write() + sync() throughput",
        mb_per_second_e2e,
        "MiB/s",
        None,
    );
    result.add_measurement(
        "create() + write() throughput",
        mb_per_second_create_write,
        "MiB/s",
        None,
    );

    // Add latency measurements
    result.add_measurement("create() latency", avg_create_dur * 1000.0, "ms", Some(4));

    let chunk_size_kb = random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64;
    result.add_measurement(
        &format!("write() {:.0} KiB latency", chunk_size_kb),
        avg_write_dur * 1000.0,
        "ms",
        Some(4),
    );
    result.add_measurement(
        &format!("sync() {:.0} KiB latency", chunk_size_kb),
        avg_sync_dur * 1000.0,
        "ms",
        Some(4),
    );

    Ok(result)
}

/// Runs the MFMD read benchmark and returns the benchmark results
fn bench_read_mfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
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
    let mb_per_second = random_data.chunk_size as f64 / avg_dur / BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new("MFMD Read");

    // Add throughput measurements
    result.add_measurement("open() + read() throughput", mb_per_second, "MiB/s", None);

    // Add latency measurements
    result.add_measurement("open() latency", avg_open_dur * 1000.0, "ms", Some(4));

    let chunk_size_kb = random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64;
    result.add_measurement(
        &format!("read() {:.0} KiB latency", chunk_size_kb),
        avg_read_dur * 1000.0,
        "ms",
        Some(4),
    );

    Ok(result)
}

/// Runs the RocksDB write benchmark and returns the benchmark results
fn bench_rocksdb_write_mfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
    let mut agg_write_dur = std::time::Duration::new(0, 0);
    let db_opts = rocksdb::Options::new().create_if_missing(true);
    let flush_opts = rocksdb::FlushOptions::new();
    let write_opts = rocksdb::WriteOptions::new();
    let db = rocksdb::Db::open(test_dir.rocksdb_path(), db_opts)?;
    let keys = random_data.keys();
    for (chunk, key) in random_data.chunks.iter().zip(keys.iter()) {
        let start = Instant::now();
        db.put(key, chunk, &write_opts)?;
        agg_write_dur += start.elapsed();
    }
    let start = Instant::now();
    db.flush(flush_opts, None)?;
    let flush_dur = start.elapsed().as_secs_f64();
    let avg_write_dur = agg_write_dur.as_secs_f64() / random_data.number_of_files as f64;
    let avg_flush_dur = flush_dur / random_data.number_of_files as f64;
    let avg_dur = avg_write_dur + avg_flush_dur;
    let mb_per_second_e2e = random_data.chunk_size as f64 / avg_dur / BYTES_IN_MEGABYTE as f64;
    let mb_per_second_write =
        random_data.chunk_size as f64 / avg_write_dur / BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new("MFMD Write with RocksDB");

    // Add throughput measurements
    result.add_measurement("write() + flush()", mb_per_second_e2e, "MiB/s", None);
    result.add_measurement("write()", mb_per_second_write, "MiB/s", None);

    // Add latency measurements
    let chunk_size_kb = random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64;
    result.add_measurement(
        &format!("write() {:.0} KiB latency", chunk_size_kb),
        avg_write_dur * 1000.0,
        "ms",
        Some(4),
    );
    result.add_measurement("flush() latency", avg_flush_dur * 1000.0, "ms", Some(4));

    Ok(result)
}

/// Runs the RocksDB read benchmark and returns the benchmark results
fn bench_rocksdb_read_mfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
    let mut agg_read_dur = std::time::Duration::new(0, 0);
    let mut read_data = vec![0u8; random_data.chunk_size];
    let db_opts = rocksdb::Options::new();
    let read_opts = rocksdb::ReadOptions::new();
    let db = rocksdb::Db::open(test_dir.rocksdb_path(), db_opts)?;
    let keys = random_data.keys();
    for key in keys {
        let start = Instant::now();
        let dbres = db.get(&key, &read_opts)?;
        match dbres {
            Some(value) => read_data.copy_from_slice(&value),
            None => return Err(anyhow!("Data not found for path {:?}", key)),
        }
        agg_read_dur += start.elapsed();
    }
    let avg_read_dur = agg_read_dur.as_secs_f64() / random_data.number_of_files as f64;
    let mb_per_second = random_data.chunk_size as f64 / avg_read_dur / BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new("MFMD Read with RocksDB");

    // Add throughput measurements
    result.add_measurement("read()", mb_per_second, "MiB/s", None);

    // Add latency measurements
    let chunk_size_kb = random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64;
    result.add_measurement(
        &format!("read() {:.0} KiB latency", chunk_size_kb),
        avg_read_dur * 1000.0,
        "ms",
        Some(4),
    );

    Ok(result)
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
    let mb_per_second_e2e = random_data.chunk_size as f64 / avg_dur / BYTES_IN_MEGABYTE as f64;
    let mb_per_second_write =
        random_data.chunk_size as f64 / avg_write_dur / BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new("MFMD Write with LMDB");

    // Add throughput measurements
    result.add_measurement("write() + sync()", mb_per_second_e2e, "MiB/s", None);
    result.add_measurement("write()", mb_per_second_write, "MiB/s", None);

    // Add latency measurements
    let chunk_size_kb = random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64;
    result.add_measurement(
        &format!("write() {:.0} KiB latency", chunk_size_kb),
        avg_write_dur * 1000.0,
        "ms",
        Some(4),
    );
    result.add_measurement("sync() latency", avg_sync_dur * 1000.0, "ms", Some(4));

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
            None => return Err(anyhow!("Data not found for key {:?}", key)),
        }
        agg_read_dur += start.elapsed();
    }
    let avg_read_dur = agg_read_dur.as_secs_f64() / random_data.number_of_files as f64;
    let mb_per_second = random_data.chunk_size as f64 / avg_read_dur / BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new("MFMD Read with LMDB");

    // Add throughput measurements
    result.add_measurement("read()", mb_per_second, "MiB/s", None);

    // Add latency measurements
    let chunk_size_kb = random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64;
    result.add_measurement(
        &format!("read() {:.0} KiB latency", chunk_size_kb),
        avg_read_dur * 1000.0,
        "ms",
        Some(4),
    );

    Ok(result)
}
/// Runs the SQLite write benchmark and returns the benchmark results
fn bench_sqlite_write_mfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
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
    let mb_per_second = random_data.chunk_size as f64 / avg_write_dur / BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new("MFMD Write with SQLite");

    // Add throughput measurements
    result.add_measurement("write()", mb_per_second, "MiB/s", None);

    // Add latency measurements
    let chunk_size_kb = random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64;
    result.add_measurement(
        &format!("write() {:.0} KiB latency", chunk_size_kb),
        avg_write_dur * 1000.0,
        "ms",
        Some(4),
    );

    Ok(result)
}

/// Runs the SQLite read benchmark and returns the benchmark results
fn bench_sqlite_read_mfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
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
    let mb_per_second = random_data.chunk_size as f64 / avg_read_dur / BYTES_IN_MEGABYTE as f64;

    let mut result = Benchmark::new("MFMD Read with SQLite");

    // Add throughput measurements
    result.add_measurement("read()", mb_per_second, "MiB/s", None);

    // Add latency measurements
    let chunk_size_kb = random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64;
    result.add_measurement(
        &format!("read() {:.0} KiB latency", chunk_size_kb),
        avg_read_dur * 1000.0,
        "ms",
        Some(4),
    );

    Ok(result)
}

/// Runs the SFMD write benchmark and returns the benchmark results
fn bench_write_sfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
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
    let mb_per_second_e2e = random_data.total_size() as f64 / BYTES_IN_MEGABYTE as f64 / agg_dur;
    let mb_per_second_write =
        random_data.total_size() as f64 / BYTES_IN_MEGABYTE as f64 / write_dur;

    let mut result = Benchmark::new("SFMD Write");

    // Add throughput measurements
    result.add_measurement(
        "create() + write() + sync() throughput",
        mb_per_second_e2e,
        "MiB/s",
        None,
    );
    result.add_measurement("create() + write()", mb_per_second_write, "MiB/s", None);

    Ok(result)
}

/// Runs the SFMD read benchmark and returns the benchmark results
fn bench_read_sfmd(test_dir: &TestDir, random_data: &RandomData) -> Result<Benchmark> {
    let file_path = test_dir.combined_data_path();
    let mut read_data = Vec::with_capacity(random_data.total_size());
    let start = Instant::now();
    let mut file = File::open(&file_path)?;
    file.read_to_end(&mut read_data)?;
    let agg_dur = start.elapsed().as_secs_f64();
    let mb_per_second = read_data.len() as f64 / BYTES_IN_MEGABYTE as f64 / agg_dur;

    let mut result = Benchmark::new("SFMD Read");

    // Add throughput measurements
    result.add_measurement("open() + read() throughput", mb_per_second, "MiB/s", None);

    Ok(result)
}

fn print_section_divider() {
    println!("-----------------------------------");
}

fn print_glossary() {
    println!("Glossary:");
    println!(
        "MFMD - Multiple Files Multiple Data - Writing and reading multiple files, each containing different data chunks."
    );
    println!(
        "SFMD - Single File Multiple Data - Writing and reading a single file containing multiple data chunks."
    );
}

#[async_trait]
impl crate::Subcommand for BenchCmd {
    async fn run(&self) -> Result<ExitCode> {
        print_section_divider();
        print_glossary();

        match self {
            Self::FsIo { common } => match TestDir::validate(&common.test_dir) {
                Ok(test_dir) => {
                    print_section_divider();
                    println!("Prepared the directory at {:?}", test_dir.path);
                    println!("Generating in-memory random data ...");
                    let random_data = RandomData::new(common.number_of_files, common.chunk_size);
                    println!(
                        "The random data generated with {} chunks with {:.0} KiB each, with the total size of {:.2} GiB.",
                        random_data.number_of_files,
                        random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64,
                        random_data.total_size() as f64 / BYTES_IN_GIGABYTE as f64
                    );
                    print_section_divider();
                    bench_write_mfmd(&test_dir, &random_data)?.display();
                    bench_read_mfmd(&test_dir, &random_data)?.display();
                    bench_write_sfmd(&test_dir, &random_data)?.display();
                    bench_read_sfmd(&test_dir, &random_data)?.display();
                    print_section_divider();
                    println!("Removing the directory at {:?}", test_dir.path);
                    test_dir.remove()?;
                }
                Err(e) => return Err(e),
            },
            Self::DbIo { common } => match TestDir::validate(&common.test_dir) {
                Ok(test_dir) => {
                    print_section_divider();
                    println!("Prepared the directory at {:?}", test_dir.path);
                    println!("Generating in-memory random data ...");
                    let random_data = RandomData::new(common.number_of_files, common.chunk_size);
                    println!(
                        "The random data generated with {} chunks with {:.0} KiB each, with the total size of {:.2} GiB.",
                        random_data.number_of_files,
                        random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64,
                        random_data.total_size() as f64 / BYTES_IN_GIGABYTE as f64
                    );
                    print_section_divider();
                    bench_rocksdb_write_mfmd(&test_dir, &random_data)?.display();
                    bench_rocksdb_read_mfmd(&test_dir, &random_data)?.display();
                    bench_lmdb_write_mfmd(&test_dir, &random_data)?.display();
                    bench_lmdb_read_mfmd(&test_dir, &random_data)?.display();
                    bench_sqlite_write_mfmd(&test_dir, &random_data)?.display();
                    bench_sqlite_read_mfmd(&test_dir, &random_data)?.display();
                    print_section_divider();
                    println!("Removing the directory at {:?}", test_dir.path);
                    test_dir.remove()?;
                }
                Err(e) => return Err(e),
            },
        }

        Ok(0)
    }
}
