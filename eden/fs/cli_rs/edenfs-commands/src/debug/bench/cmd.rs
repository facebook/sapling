/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Command-line interface for benchmarking

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;

use super::dbio;
use super::fsio;
use super::gen;
use super::traversal;
use super::types;
use crate::ExitCode;

#[derive(Parser, Debug)]
pub struct CommonOptions {
    /// Directory to use for testing
    #[clap(long, default_value_t = std::env::temp_dir().to_str().unwrap().to_string())]
    pub test_dir: String,

    /// Number of randomly generated files to use for benchmarking
    #[clap(long, default_value_t = types::DEFAULT_NUMBER_OF_FILES)]
    pub number_of_files: usize,

    /// Size of each chunk in bytes
    #[clap(long, default_value_t = types::DEFAULT_CHUNK_SIZE)]
    pub chunk_size: usize,
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

    #[clap(about = "Run filesystem traversal benchmark")]
    FsTraversal {
        /// Directory to traverse
        #[clap(long)]
        dir: String,
    },
}

#[async_trait]
impl crate::Subcommand for BenchCmd {
    async fn run(&self) -> Result<ExitCode> {
        match self {
            Self::FsIo { common } => match gen::TestDir::validate(&common.test_dir) {
                Ok(test_dir) => {
                    let random_data =
                        gen::RandomData::new(common.number_of_files, common.chunk_size);
                    println!(
                        "The random data generated with {} chunks with {:.0} KiB each, with the total size of {:.2} GiB.",
                        random_data.number_of_files,
                        random_data.chunk_size as f64 / types::BYTES_IN_KILOBYTE as f64,
                        random_data.total_size() as f64 / types::BYTES_IN_GIGABYTE as f64
                    );
                    println!("{}", fsio::bench_write_mfmd(&test_dir, &random_data)?);
                    println!("{}", fsio::bench_read_mfmd(&test_dir, &random_data)?);
                    println!("{}", fsio::bench_write_sfmd(&test_dir, &random_data)?);
                    println!("{}", fsio::bench_read_sfmd(&test_dir, &random_data)?);
                    test_dir.remove()?;
                }
                Err(e) => return Err(e),
            },
            Self::DbIo { common } => match gen::TestDir::validate(&common.test_dir) {
                Ok(test_dir) => {
                    let random_data =
                        gen::RandomData::new(common.number_of_files, common.chunk_size);
                    println!(
                        "The random data generated with {} chunks with {:.0} KiB each, with the total size of {:.2} GiB.",
                        random_data.number_of_files,
                        random_data.chunk_size as f64 / types::BYTES_IN_KILOBYTE as f64,
                        random_data.total_size() as f64 / types::BYTES_IN_GIGABYTE as f64
                    );
                    println!(
                        "{}",
                        dbio::bench_rocksdb_write_mfmd(&test_dir, &random_data)?
                    );
                    println!(
                        "{}",
                        dbio::bench_rocksdb_read_mfmd(&test_dir, &random_data)?
                    );
                    println!("{}", dbio::bench_lmdb_write_mfmd(&test_dir, &random_data)?);
                    println!("{}", dbio::bench_lmdb_read_mfmd(&test_dir, &random_data)?);
                    println!(
                        "{}",
                        dbio::bench_sqlite_write_mfmd(&test_dir, &random_data)?
                    );
                    println!("{}", dbio::bench_sqlite_read_mfmd(&test_dir, &random_data)?);
                    test_dir.remove()?;
                }
                Err(e) => return Err(e),
            },
            Self::FsTraversal { dir } => {
                println!(
                    "Running filesystem traversal benchmark on directory: {}",
                    dir
                );
                println!("{}", traversal::bench_fs_traversal(dir)?);
            }
        }

        Ok(0)
    }
}
