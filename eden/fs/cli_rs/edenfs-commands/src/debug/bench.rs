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

struct RandomData {
    // Directory to use for testing.
    test_dir: PathBuf,

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
    fn new(test_dir: PathBuf, number_of_files: usize, chunk_size: usize) -> Self {
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
            test_dir,
            number_of_files,
            chunk_size,
            chunks,
            hashes,
        }
    }

    fn paths(&self) -> Vec<PathBuf> {
        self.hashes
            .iter()
            .map(|hash| hash_to_path(&self.test_dir, hash))
            .collect()
    }

    fn keys(&self) -> Vec<Vec<u8>> {
        self.hashes.iter().map(|h| h.as_bytes().to_vec()).collect()
    }

    fn total_size(&self) -> usize {
        self.number_of_files * self.chunk_size
    }

    fn combined_data_path(&self) -> PathBuf {
        self.test_dir.join(COMBINED_DATA_FILE_NAME)
    }

    fn rocksdb_path(&self) -> PathBuf {
        self.test_dir.join(ROCKSDB_FILE_NAME)
    }

    fn lmdb_path(&self) -> PathBuf {
        self.test_dir.join(LMDB_FILE_NAME)
    }

    fn sqlite_path(&self) -> PathBuf {
        self.test_dir.join(SQLITE_FILE_NAME)
    }
}

fn prepare_directories(root: &Path) -> Result<()> {
    for i in 0..NUMBER_OF_SUB_DIRS {
        let sub_dir = format!("{:02x}", i);
        let sub_dir_path = root.join(sub_dir);
        fs::create_dir_all(&sub_dir_path)?;
    }
    Ok(())
}

fn validate_test_dir(test_dir: &str) -> Result<PathBuf> {
    let test_dir_path = Path::new(test_dir);
    if !test_dir_path.exists() {
        return Err(anyhow!("The directory {} does not exist.", test_dir));
    }
    let bench_dir_path = test_dir_path.join(BENCH_DIR_NAME);
    if bench_dir_path.exists() {
        fs::remove_dir_all(&bench_dir_path)?;
    }
    fs::create_dir(&bench_dir_path)?;
    prepare_directories(&bench_dir_path)?;
    Ok(bench_dir_path)
}

fn remove_test_dir(test_dir: &PathBuf) -> Result<()> {
    if test_dir.exists() {
        fs::remove_dir_all(test_dir)?;
    }
    Ok(())
}

fn hash_to_path(root: &Path, hash: &Hash) -> PathBuf {
    let hash_str = hash.to_hex().to_string();
    let sub_dir = &hash_str[0..2];
    root.join(sub_dir).join(hash_str)
}

fn bench_write_mfmd(random_data: &RandomData) -> Result<()> {
    let mut agg_create_dur = std::time::Duration::new(0, 0);
    let mut agg_write_dur = std::time::Duration::new(0, 0);
    for (chunk, path) in random_data.chunks.iter().zip(random_data.paths().iter()) {
        let start = Instant::now();
        let mut file = File::create(path)?;
        agg_create_dur += start.elapsed();

        let start = Instant::now();
        file.write_all(chunk)?;
        agg_write_dur += start.elapsed();
    }

    let mut agg_sync_dur = std::time::Duration::new(0, 0);
    for path in random_data.paths() {
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
    println!("MFMD Write");
    println!(
        "\t- {:.2} MiB/s create() + write() + sync()",
        mb_per_second_e2e
    );
    println!(
        "\t- {:.2} MiB/s create() + write()",
        mb_per_second_create_write
    );
    println!("\t- {:.4} ms create() latency", avg_create_dur * 1000.0);
    println!(
        "\t- {:.4} ms write() {:.0} KiB bytes latency",
        avg_write_dur * 1000.0,
        random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64
    );
    println!(
        "\t- {:.4} ms sync() {:.0} KiB latency",
        avg_sync_dur * 1000.0,
        random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64
    );
    Ok(())
}

fn bench_read_mfmd(random_data: &RandomData) -> Result<()> {
    let mut agg_open_dur = std::time::Duration::new(0, 0);
    let mut agg_read_dur = std::time::Duration::new(0, 0);
    let mut read_data = vec![0u8; random_data.chunk_size];
    for path in random_data.paths() {
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
    println!("MFMD Read");
    println!("\t- {:.2} MiB/s open() + read()", mb_per_second);
    println!("\t- {:.4} ms open() latency", avg_open_dur * 1000.0);
    println!(
        "\t- {:.4} ms read() {:.0} KiB latency",
        avg_read_dur * 1000.0,
        random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64
    );
    Ok(())
}

fn bench_rocksdb_write_mfmd(random_data: &RandomData) -> Result<()> {
    let mut agg_write_dur = std::time::Duration::new(0, 0);
    let db_opts = rocksdb::Options::new().create_if_missing(true);
    let flush_opts = rocksdb::FlushOptions::new();
    let write_opts = rocksdb::WriteOptions::new();
    let db = rocksdb::Db::open(random_data.rocksdb_path(), db_opts)?;
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
    println!("MFMD Write with RocksDB");
    println!("\t- {:.2} MiB/s write() + flush()", mb_per_second_e2e);
    println!("\t- {:.2} MiB/s write()", mb_per_second_write);
    println!(
        "\t- {:.4} ms write() {:.0} KiB latency",
        avg_write_dur * 1000.0,
        random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64
    );

    Ok(())
}

fn bench_rocksdb_read_mfmd(random_data: &RandomData) -> Result<()> {
    let mut agg_read_dur = std::time::Duration::new(0, 0);
    let mut read_data = vec![0u8; random_data.chunk_size];
    let db_opts = rocksdb::Options::new();
    let read_opts = rocksdb::ReadOptions::new();
    let db = rocksdb::Db::open(random_data.rocksdb_path(), db_opts)?;
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
    println!("MFMD Read with RocksDB");
    println!("\t- {:.2} MiB/s read()", mb_per_second);
    println!(
        "\t- {:.4} ms read() {:.0} KiB latency",
        avg_read_dur * 1000.0,
        random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64
    );
    Ok(())
}

fn bench_lmdb_write_mfmd(random_data: &RandomData) -> Result<()> {
    let mut agg_write_dur = std::time::Duration::new(0, 0);
    let env = lmdb::Env::options()?
        .set_mapsize(4 * random_data.total_size())?
        .set_nosync(true)
        .set_nordahead(true)
        .create_file(random_data.lmdb_path(), 0o644)?;
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
    println!("MFMD Write with LMDB");
    println!("\t- {:.2} MiB/s write() + sync()", mb_per_second_e2e);
    println!("\t- {:.2} MiB/s write()", mb_per_second_write);
    println!(
        "\t- {:.4} ms write() {:.0} KiB latency",
        avg_write_dur * 1000.0,
        random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64
    );

    Ok(())
}

fn bench_lmdb_read_mfmd(random_data: &RandomData) -> Result<()> {
    let mut agg_read_dur = std::time::Duration::new(0, 0);
    let mut read_data = vec![0u8; random_data.chunk_size];
    let env = lmdb::Env::options()?
        .set_nordahead(true)
        .open_file(random_data.lmdb_path(), 0o644)?;
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
    println!("MFMD Read with LMDB");
    println!("\t- {:.2} MiB/s read()", mb_per_second);
    println!(
        "\t- {:.4} ms read() {:.0} KiB latency",
        avg_read_dur * 1000.0,
        random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64
    );
    Ok(())
}

fn bench_sqlite_write_mfmd(random_data: &RandomData) -> Result<()> {
    let conn = rusqlite::Connection::open(random_data.sqlite_path())?;
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
    println!("MFMD Write with SQLite");
    println!("\t- {:.2} MiB/s write()", mb_per_second);
    println!(
        "\t- {:.4} ms write() {:.0} KiB latency",
        avg_write_dur * 1000.0,
        random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64
    );
    Ok(())
}

fn bench_sqlite_read_mfmd(random_data: &RandomData) -> Result<()> {
    let conn = rusqlite::Connection::open(random_data.sqlite_path())?;
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
    println!("MFMD Read with SQLite");
    println!("\t- {:.2} MiB/s read()", mb_per_second);
    println!(
        "\t- {:.4} ms read() {:.0} KiB latency",
        avg_read_dur * 1000.0,
        random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64
    );
    Ok(())
}

fn bench_write_sfmd(random_data: &RandomData) -> Result<()> {
    let start = Instant::now();
    let mut file = File::create(random_data.combined_data_path())?;
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
    println!("SFMD Write");
    println!(
        "\t- {:.2} MiB/s create() + write() + sync()",
        mb_per_second_e2e
    );
    println!("\t- {:.2} MiB/s create() + write()", mb_per_second_write);
    Ok(())
}

fn bench_read_sfmd(random_data: &RandomData) -> Result<()> {
    let file_path = random_data.combined_data_path();
    let mut read_data = Vec::with_capacity(random_data.total_size());
    let start = Instant::now();
    let mut file = File::open(&file_path)?;
    file.read_to_end(&mut read_data)?;
    let agg_dur = start.elapsed().as_secs_f64();
    let mb_per_second = read_data.len() as f64 / BYTES_IN_MEGABYTE as f64 / agg_dur;
    println!("SFMD Read");
    println!("\t- {:.2} MiB/s open() + read()", mb_per_second);
    Ok(())
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
            Self::FsIo { common } => match validate_test_dir(&common.test_dir) {
                Ok(path) => {
                    print_section_divider();
                    println!("Prepared the directory at {:?}", path);
                    println!("Generating in-memory random data ...");
                    let random_data =
                        RandomData::new(path, common.number_of_files, common.chunk_size);
                    println!(
                        "The random data generated with {} chunks with {:.0} KiB each, with the total size of {:.2} GiB.",
                        random_data.number_of_files,
                        random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64,
                        random_data.total_size() as f64 / BYTES_IN_GIGABYTE as f64
                    );
                    print_section_divider();
                    bench_write_mfmd(&random_data)?;
                    bench_read_mfmd(&random_data)?;
                    bench_write_sfmd(&random_data)?;
                    bench_read_sfmd(&random_data)?;
                    print_section_divider();
                    println!("Removing the directory at {:?}", random_data.test_dir);
                    remove_test_dir(&random_data.test_dir)?;
                }
                Err(e) => return Err(e),
            },
            Self::DbIo { common } => match validate_test_dir(&common.test_dir) {
                Ok(path) => {
                    print_section_divider();
                    println!("Prepared the directory at {:?}", path);
                    println!("Generating in-memory random data ...");
                    let random_data =
                        RandomData::new(path, common.number_of_files, common.chunk_size);
                    println!(
                        "The random data generated with {} chunks with {:.0} KiB each, with the total size of {:.2} GiB.",
                        random_data.number_of_files,
                        random_data.chunk_size as f64 / BYTES_IN_KILOBYTE as f64,
                        random_data.total_size() as f64 / BYTES_IN_GIGABYTE as f64
                    );
                    print_section_divider();
                    bench_rocksdb_write_mfmd(&random_data)?;
                    bench_rocksdb_read_mfmd(&random_data)?;
                    bench_lmdb_write_mfmd(&random_data)?;
                    bench_lmdb_read_mfmd(&random_data)?;
                    bench_sqlite_write_mfmd(&random_data)?;
                    bench_sqlite_read_mfmd(&random_data)?;
                    print_section_divider();
                    println!("Removing the directory at {:?}", random_data.test_dir);
                    remove_test_dir(&random_data.test_dir)?;
                }
                Err(e) => return Err(e),
            },
        }

        Ok(0)
    }
}
