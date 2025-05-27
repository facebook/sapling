/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Types for benchmarking

// Constants
pub const BENCH_DIR_NAME: &str = "__fsiomicrobench__";
pub const ROCKSDB_FILE_NAME: &str = "__rocksdb__";
pub const LMDB_FILE_NAME: &str = "__lmdb__";
pub const SQLITE_FILE_NAME: &str = "__sqlite__";
pub const COMBINED_DATA_FILE_NAME: &str = "__combined_data__";
pub const DEFAULT_NUMBER_OF_FILES: usize = 64 * 1024;
pub const DEFAULT_CHUNK_SIZE: usize = 4 * 1024;
pub const NUMBER_OF_SUB_DIRS: usize = 256;
pub const BYTES_IN_KILOBYTE: usize = 1024;
pub const BYTES_IN_MEGABYTE: usize = 1024 * BYTES_IN_KILOBYTE;
pub const BYTES_IN_GIGABYTE: usize = 1024 * BYTES_IN_MEGABYTE;

/// Represents the type of benchmark being performed
#[derive(Debug, Clone)]
pub enum BenchmarkType {
    FsWriteMultipleFiles,
    FsReadMultipleFiles,
    FsWriteSingleFile,
    FsReadSingleFile,
    FsTraversal,
    ThriftReadMultipleFiles,
    ThriftReadSingleFile,
    RocksDbWriteMultipleFiles,
    RocksDbReadMultipleFiles,
    LmdbWriteMultipleFiles,
    LmdbReadMultipleFiles,
    SqliteWriteMultipleFiles,
    SqliteReadMultipleFiles,
}

impl std::fmt::Display for BenchmarkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BenchmarkType::FsWriteMultipleFiles => write!(f, "Filesystem Write Multiple Files"),
            BenchmarkType::FsReadMultipleFiles => write!(f, "Filesystem Read Multiple Files"),
            BenchmarkType::FsWriteSingleFile => write!(f, "Filesystem Write Single File"),
            BenchmarkType::FsReadSingleFile => write!(f, "Filesystem Read Single File"),
            BenchmarkType::ThriftReadMultipleFiles => write!(f, "Thrift Read Multiple File"),
            BenchmarkType::ThriftReadSingleFile => write!(f, "Thrift Read Single File"),
            BenchmarkType::FsTraversal => write!(f, "Filesystem Traversal"),
            BenchmarkType::RocksDbWriteMultipleFiles => write!(f, "RocksDB Write Multiple Files"),
            BenchmarkType::RocksDbReadMultipleFiles => write!(f, "RocksDB Read Multiple Files"),
            BenchmarkType::LmdbWriteMultipleFiles => write!(f, "LMDB Write Multiple Files"),
            BenchmarkType::LmdbReadMultipleFiles => write!(f, "LMDB Read Multiple Files"),
            BenchmarkType::SqliteWriteMultipleFiles => write!(f, "SQLite Write Multiple Files"),
            BenchmarkType::SqliteReadMultipleFiles => write!(f, "SQLite Read Multiple Files"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, clap::ValueEnum)]
pub enum ReadFileMethod {
    Fs,
    Thrift,
}

/// Represents the result of a benchmark operation
#[derive(Debug, Clone)]
pub struct Benchmark {
    /// Type of the benchmark
    pub benchmark_type: BenchmarkType,
    /// Various metrics
    pub metrics: Vec<Metric>,
}

/// Represents the unit of measurement for a metric
#[derive(Debug, Clone, PartialEq)]
pub enum Unit {
    /// Megabytes per second (throughput)
    MiBps,
    /// Milliseconds (latency)
    Ms,
    /// Files per second (traversal throughput)
    FilesPerSecond,
    /// Count of files
    Files,
    /// Count of directories
    Dirs,
    /// Kilobytes (file size)
    KiB,
    /// Megabytes (total data)
    MiB,
}

impl std::fmt::Display for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unit::MiBps => write!(f, "MiB/s"),
            Unit::Ms => write!(f, "ms"),
            Unit::FilesPerSecond => write!(f, "files/s"),
            Unit::Files => write!(f, "files"),
            Unit::Dirs => write!(f, "dirs"),
            Unit::KiB => write!(f, "KiB"),
            Unit::MiB => write!(f, "MiB"),
        }
    }
}

/// Represents a metric with a name, value, unit, and precision
#[derive(Debug, Clone)]
pub struct Metric {
    /// Name of the metric (e.g., "write()", "write() latency")
    pub name: String,
    /// Value of the metric
    pub value: f64,
    /// Unit of the metric (e.g., MiBps, Ms)
    pub unit: Unit,
    /// Precision for display (number of decimal places)
    pub precision: u8,
}

impl Benchmark {
    /// Creates a new benchmark result with the given benchmark type
    pub fn new(benchmark_type: BenchmarkType) -> Self {
        Benchmark {
            benchmark_type,
            metrics: Vec::new(),
        }
    }

    /// Adds a metric with optional precision (defaults to 2)
    pub fn add_metric(&mut self, name: &str, value: f64, unit: Unit, precision: Option<u8>) {
        self.metrics.push(Metric {
            name: name.to_string(),
            value,
            unit,
            precision: precision.unwrap_or(2),
        });
    }
}

impl std::fmt::Display for Benchmark {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let format_value_with_precision =
            |value: f64, precision: u8| -> String { format!("{:.1$}", value, precision as usize) };

        writeln!(f, "{}", self.benchmark_type)?;

        let max_value_len = self
            .metrics
            .iter()
            .map(|metric| format_value_with_precision(metric.value, metric.precision).len())
            .max()
            .map_or(0, |len| if len < 10 { 10 } else { len });

        let max_unit_len = self
            .metrics
            .iter()
            .map(|metric| format!("{}", metric.unit).len())
            .max()
            .unwrap_or(0);

        for metric in &self.metrics {
            let value_str = format_value_with_precision(metric.value, metric.precision);

            writeln!(
                f,
                "{:>width$} {:<unit_width$} - {}",
                value_str,
                metric.unit,
                metric.name,
                width = max_value_len,
                unit_width = max_unit_len
            )?;
        }

        Ok(())
    }
}
