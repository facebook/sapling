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
#[clap(
    about = "Run benchmarks for EdenFS and OS-native file systems on Linux, macOS, and Windows",
    long_about = "Benchmark filesystem operations including traversal, I/O, and database performance"
)]
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

    #[clap(
        about = "Run filesystem traversal benchmark",
        long_about = "Benchmark filesystem traversal performance including file reading, directory scanning, and I/O throughput. Supports multiple directories and various reading modes."
    )]
    Traversal {
        /// Directories to traverse (can be specified multiple times: --dir=/path1 --dir=/path2)
        #[clap(
            long,
            required = true,
            help = "Directory to traverse",
            long_help = "Directories to traverse. Can be specified multiple times to benchmark across multiple directory trees. Each directory will be traversed sequentially and results will be combined."
        )]
        dir: Vec<String>,

        /// Path to fbsource directory, required for thrift IO mode
        #[clap(
            long,
            help = "Use thrift I/O instead of filesystem calls",
            long_help = "Path to fbsource directory for thrift I/O mode. When specified, files will be read using EdenFS thrift calls instead of direct filesystem operations."
        )]
        thrift_io: Option<String>,

        /// Maximum number of files to process (default: unlimited)
        #[clap(
            long,
            help = "Limit number of files to process",
            long_help = "Maximum number of files to process during traversal. If not specified, all files in the directory tree will be processed."
        )]
        max_files: Option<usize>,

        /// Follow symbolic links during directory traversal
        #[clap(long, help = "Follow symbolic links")]
        follow_symlinks: bool,

        /// Disable progress bars and real-time updates
        #[clap(long, help = "Disable progress display")]
        no_progress: bool,

        /// Monitor CPU and memory usage during the benchmark
        #[clap(
            long,
            help = "Enable resource monitoring",
            long_help = "Enable CPU and memory usage monitoring during traversal. Shows additional metrics including memory consumption and CPU utilization."
        )]
        resource_usage: bool,

        /// Output results in JSON format for programmatic processing
        #[clap(long, help = "Output JSON format")]
        json: bool,

        /// Skip file reading and only measure directory traversal performance
        #[clap(
            long,
            help = "Skip file I/O, only traverse",
            long_help = "Skip the file reading benchmark and only measure directory traversal performance. Useful for testing pure filesystem traversal speed without I/O overhead."
        )]
        skip_read: bool,

        /// Show detailed read statistics including file size distribution and per-directory analysis
        #[clap(
            long,
            help = "Show detailed read performance statistics",
            long_help = "Enable detailed read statistics showing file size distribution, per-directory I/O performance breakdown, depth analysis, and file category overhead metrics. Only available when file reading is enabled (not compatible with --skip-read)."
        )]
        detailed_read_stats: bool,

        /// Show detailed directory listing statistics
        #[clap(
            long,
            help = "Show detailed directory listing statistics",
            long_help = "Enable detailed statistics about directory traversal including readdir() latency distribution, directory size analysis, scan rate variance, and slowest directories. Compatible with --skip-read for analyzing pure traversal performance."
        )]
        detailed_list_stats: bool,

        /// Include per-directory statistics (causes ~20% throughput reduction)
        #[clap(
            long,
            help = "Include per-directory stats (slower)",
            long_help = "Enable per-directory I/O statistics tracking. This feature causes significant overhead (~20% throughput reduction) due to CPU cache effects from HashMap operations. Only effective when used with --detailed-read-stats."
        )]
        include_dir_stats: bool,
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
                resource_usage,
                json,
                skip_read,
                detailed_read_stats,
                detailed_list_stats,
                include_dir_stats,
            } => {
                // Validate flag compatibility
                if *skip_read && *detailed_read_stats {
                    return Err(anyhow::anyhow!(
                        "--skip-read and --detailed-read-stats are mutually exclusive.\n\
                        Detailed read statistics focus on file I/O performance metrics which are not \
                        collected when skipping file reads. Use --detailed-read-stats without --skip-read \
                        to see I/O performance analysis."
                    ));
                }

                if !*json {
                    if dir.len() == 1 {
                        println!(
                            "Running filesystem traversal benchmark on directory: {}",
                            dir[0]
                        );
                    } else {
                        println!(
                            "Running filesystem traversal benchmark on {} directories:",
                            dir.len()
                        );
                        for directory in dir {
                            println!("  - {}", directory);
                        }
                    }
                }

                let effective_max_files = max_files.unwrap_or(usize::MAX);
                let benchmark_result = traversal::bench_traversal(
                    dir,
                    effective_max_files,
                    *follow_symlinks,
                    *no_progress,
                    *resource_usage,
                    *skip_read,
                    thrift_io.as_deref(),
                    *detailed_read_stats,
                    *detailed_list_stats,
                    *include_dir_stats,
                )
                .await?;

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
