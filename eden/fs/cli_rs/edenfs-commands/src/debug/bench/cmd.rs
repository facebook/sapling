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
#[clap(about = "Run benchmarks for EdenFS and OS-native file systems on Linux, macOS, and Windows")]
pub enum BenchCmd {
    #[clap(about = "Run filesystem/thrift I/O benchmarks")]
    FsIo {
        /// Directory to use for testing
        #[clap(long, default_value_t = std::env::temp_dir().to_str().unwrap().to_string())]
        test_dir: String,

        /// Number of randomly generated files to use for benchmarking
        #[clap(long, default_value_t = types::DEFAULT_NUMBER_OF_FILES)]
        number_of_files: usize,

        /// Size of each chunk in bytes
        #[clap(long, default_value_t = types::DEFAULT_CHUNK_SIZE)]
        chunk_size: usize,

        /// Read file content through file system or via thrift.
        #[clap(long, value_enum, default_value_t = types::ReadFileMethod::Fs)]
        read_file_via: types::ReadFileMethod,
    },

    #[clap(about = "Run database I/O benchmarks")]
    DbIo {
        /// Directory to use for testing
        #[clap(long, default_value_t = std::env::temp_dir().to_str().unwrap().to_string())]
        test_dir: String,

        /// Number of randomly generated files to use for benchmarking
        #[clap(long, default_value_t = types::DEFAULT_NUMBER_OF_FILES)]
        number_of_files: usize,

        /// Size of each chunk in bytes
        #[clap(long, default_value_t = types::DEFAULT_CHUNK_SIZE)]
        chunk_size: usize,
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
            Self::FsIo {
                test_dir,
                number_of_files,
                chunk_size,
                read_file_via,
            } => match gen::TestDir::validate(
                test_dir,
                *read_file_via == types::ReadFileMethod::Thrift,
            ) {
                Ok(test_dir) => {
                    let random_data = gen::RandomData::new(*number_of_files, *chunk_size);
                    println!(
                        "The random data generated with {} chunks with {:.0} KiB each, with the total size of {:.2} GiB.",
                        random_data.number_of_files,
                        random_data.chunk_size as f64 / types::BYTES_IN_KILOBYTE as f64,
                        random_data.total_size() as f64 / types::BYTES_IN_GIGABYTE as f64
                    );
                    println!("{}", fsio::bench_write_mfmd(&test_dir, &random_data)?);
                    match read_file_via {
                        types::ReadFileMethod::Fs => {
                            println!("{}", fsio::bench_fs_read_mfmd(&test_dir, &random_data)?);
                        }
                        types::ReadFileMethod::Thrift => {
                            println!(
                                "{}",
                                fsio::bench_thrift_read_mfmd(&test_dir, &random_data).await?
                            );
                        }
                    }
                    println!("{}", fsio::bench_write_sfmd(&test_dir, &random_data)?);
                    match read_file_via {
                        types::ReadFileMethod::Fs => {
                            println!("{}", fsio::bench_fs_read_sfmd(&test_dir, &random_data)?);
                        }
                        types::ReadFileMethod::Thrift => {
                            println!("{}", fsio::bench_thrift_read_sfmd(&test_dir).await?);
                        }
                    }
                    test_dir.remove()?;
                }
                Err(e) => return Err(e),
            },
            Self::DbIo {
                test_dir,
                number_of_files,
                chunk_size,
            } => match gen::TestDir::validate(test_dir, false) {
                Ok(test_dir) => {
                    let random_data = gen::RandomData::new(*number_of_files, *chunk_size);
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
