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
use serde_json;

use super::dbio;
use super::fsio;
use super::r#gen;
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

        /// Whether to drop memory caches after writes.
        /// Only supported on linux and needs root privilege to run.
        #[clap(long)]
        drop_kernel_caches: bool,
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

    #[clap(about = "Run traversal benchmark")]
    Traversal {
        /// Directory to traverse
        #[clap(long)]
        dir: String,

        /// Path to fbsource directory, required for thrift IO
        #[clap(long)]
        thrift_io: Option<String>,

        /// Max number of files to read when traversing the file system
        #[clap(long, default_value_t = types::DEFAULT_MAX_NUMBER_OF_FILES_FOR_TRAVERSAL)]
        max_files: usize,

        /// Whether to follow symbolic links during traversal
        #[clap(long)]
        follow_symlinks: bool,

        /// Disable progress bars in benchmarks
        #[clap(long)]
        no_progress: bool,

        /// Output results in JSON format
        #[clap(long)]
        json: bool,
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
                drop_kernel_caches,
            } => match r#gen::TestDir::validate(test_dir) {
                Ok(test_dir) => {
                    let random_data = r#gen::RandomData::new(*number_of_files, *chunk_size);
                    println!(
                        "The random data generated with {} chunks with {:.0} KiB each, with the total size of {:.2} GiB.",
                        random_data.number_of_files,
                        random_data.chunk_size as f64 / types::BYTES_IN_KILOBYTE as f64,
                        random_data.total_size() as f64 / types::BYTES_IN_GIGABYTE as f64
                    );
                    println!(
                        "{}",
                        fsio::bench_write_mfmd(&test_dir, &random_data, *drop_kernel_caches)?
                    );
                    println!("{}", fsio::bench_fs_read_mfmd(&test_dir, &random_data)?);

                    println!(
                        "{}",
                        fsio::bench_write_sfmd(&test_dir, &random_data, *drop_kernel_caches)?
                    );

                    println!("{}", fsio::bench_fs_read_sfmd(&test_dir, &random_data)?);

                    test_dir.remove()?;
                }
                Err(e) => return Err(e),
            },
            Self::DbIo {
                test_dir,
                number_of_files,
                chunk_size,
            } => match r#gen::TestDir::validate(test_dir) {
                Ok(test_dir) => {
                    let random_data = r#gen::RandomData::new(*number_of_files, *chunk_size);
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
            Self::Traversal {
                dir,
                thrift_io,
                max_files,
                follow_symlinks,
                no_progress,
                json,
            } => {
                if !*json {
                    println!(
                        "Running filesystem traversal benchmark on directory: {}",
                        dir
                    );
                }

                let benchmark_result = if thrift_io.is_some() {
                    traversal::bench_traversal_thrift_read(
                        dir,
                        *max_files,
                        *follow_symlinks,
                        *no_progress,
                        thrift_io.as_deref(),
                    )
                    .await?
                } else {
                    traversal::bench_traversal_fs_read(
                        dir,
                        *max_files,
                        *follow_symlinks,
                        *no_progress,
                    )?
                };

                if *json {
                    println!("{}", serde_json::to_string_pretty(&benchmark_result)?);
                } else {
                    println!("{}", benchmark_result);
                }
            }
        }

        Ok(0)
    }
}
