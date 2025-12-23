/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Filesystem traversal benchmarking

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Instant;

use anyhow::Result;
use edenfs_client::client::Client;
use edenfs_client::methods::EdenThriftMethod;
use edenfs_utils::bytes_from_path;
use indicatif::ProgressBar;
use indicatif::ProgressStyle;
use num_format::Locale;
use num_format::ToFormattedString;
use sysinfo::System;
use thrift_types::edenfs::MountId;
use thrift_types::edenfs::ScmBlobOrError;
use thrift_types::edenfs::SyncBehavior;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::types;
use super::types::Benchmark;
use super::types::BenchmarkType;
use crate::get_edenfs_instance;

// File size category boundaries for histo output
const SMALL_FILE_THRESHOLD: u64 = 10 * 1024; // 10 KB
const MEDIUM_FILE_THRESHOLD: u64 = 1024 * 1024; // 1 MB

// Maximum number of directory entries to track for detailed statistics
// This prevents unbounded memory growth when traversing millions of directories
// Each entry uses ~200-300 bytes, so 10M entries ≈ 2-3GB memory
const MAX_DIR_STATS_ENTRIES: usize = 10_000_000;

// ============================================================================
// Type Definitions
// ============================================================================

/// File size categories for performance analysis
#[derive(Debug, Clone, Copy)]
enum FileSizeCategory {
    Small = 0,
    Medium = 1,
    Large = 2,
}

impl FileSizeCategory {
    fn from_size(size: u64) -> Self {
        if size < SMALL_FILE_THRESHOLD {
            Self::Small
        } else if size < MEDIUM_FILE_THRESHOLD {
            Self::Medium
        } else {
            Self::Large
        }
    }

    fn name(&self) -> &'static str {
        match self {
            Self::Small => "Small",
            Self::Medium => "Medium",
            Self::Large => "Large",
        }
    }

    fn description(&self) -> String {
        match self {
            Self::Small => format!("<{} KB", SMALL_FILE_THRESHOLD / 1024),
            Self::Medium => format!(
                "{} KB - {} MB",
                SMALL_FILE_THRESHOLD / 1024,
                MEDIUM_FILE_THRESHOLD / 1024 / 1024
            ),
            Self::Large => format!(">{} MB", MEDIUM_FILE_THRESHOLD / 1024 / 1024),
        }
    }

    const COUNT: usize = 3;
}

/// Statistics for a single file size category (under 1k, under 1MB, etc)
#[derive(Default)]
struct CategoryStats {
    count: AtomicUsize,
    bytes: AtomicU64,
    open_nanos: AtomicU64,
    read_nanos: AtomicU64,
}

#[derive(Clone)]
enum ReadMode {
    Skip,
    Fs,
    Thrift {
        client: Arc<edenfs_client::client::EdenFsClient>,
        repo_path: PathBuf,
    },
}

/// Detailed statistics for more comprehensive read performance analysis
struct AdvancedStats {
    // File size histogram (buckets: <1KB, 1-10KB, 10-100KB, 100KB-1MB,
    // 1MB-10MB, 10MB-100MB, >100MB)
    size_histogram: [AtomicUsize; 7],
    size_histogram_bytes: [AtomicU64; 7],

    // Per-directory statistics (path -> (file_count, total_bytes, total_duration_nanos))
    dir_stats: std::sync::Mutex<std::collections::HashMap<String, (usize, u64, u64)>>,

    // Count of directories that couldn't be tracked due to MAX_DIR_STATS_ENTRIES limit
    dirs_dropped: AtomicUsize,

    // Directory depth statistics (relative to base paths)
    depth_histogram: [AtomicUsize; 20], // Track depths 0-19+
    base_paths: Vec<PathBuf>,           // Base paths for relative depth calculation

    // Per-category statistics (indexed by FileSizeCategory)
    category_stats: [CategoryStats; FileSizeCategory::COUNT],

    // Peak memory usage in bytes
    peak_memory_bytes: AtomicU64,
}

impl AdvancedStats {
    fn new(base_paths: Vec<PathBuf>) -> Arc<Self> {
        Arc::new(Self {
            size_histogram: Default::default(),
            size_histogram_bytes: Default::default(),
            dir_stats: std::sync::Mutex::new(std::collections::HashMap::new()),
            dirs_dropped: AtomicUsize::new(0),
            depth_histogram: Default::default(),
            base_paths,
            category_stats: Default::default(),
            peak_memory_bytes: AtomicU64::new(0),
        })
    }

    fn get_size_bucket(size: u64) -> usize {
        const KB: u64 = 1024;
        const MB: u64 = 1024 * KB;

        match size {
            s if s < KB => 0,       // <1KB
            s if s < 10 * KB => 1,  // 1-10KB
            s if s < 100 * KB => 2, // 10-100KB
            s if s < MB => 3,       // 100KB-1MB
            s if s < 10 * MB => 4,  // 1-10MB
            s if s < 100 * MB => 5, // 10-100MB
            _ => 6,                 // >100MB
        }
    }

    fn calculate_relative_depth(&self, path: &Path) -> Option<usize> {
        // Try to find which base path this file is under
        for base in &self.base_paths {
            if let Ok(rel_path) = path.strip_prefix(base) {
                // Count the number of components in the relative path
                // Subtract 1 because the file itself shouldn't count toward directory depth
                return Some(rel_path.components().count().saturating_sub(1));
            }
        }
        None
    }

    /// Convert an absolute path to relative to the traversal base paths
    fn make_path_relative(&self, path: &str) -> String {
        let path_buf = PathBuf::from(path);

        // Try to strip each base path and return the first match
        for base in &self.base_paths {
            if let Ok(rel_path) = path_buf.strip_prefix(base) {
                return rel_path.to_string_lossy().to_string();
            }
        }

        // If no base path matches, return the original path
        // (shouldn't happen, but handle gracefully)
        path.to_string()
    }

    fn record_file(&self, path: &Path, size: u64, open_nanos: u64, read_nanos: u64) {
        // Update size histogram
        let bucket = Self::get_size_bucket(size);
        self.size_histogram[bucket].fetch_add(1, Ordering::Relaxed);
        self.size_histogram_bytes[bucket].fetch_add(size, Ordering::Relaxed);

        // Track file category performance using enum indexing
        let category = FileSizeCategory::from_size(size);
        let category_idx = category as usize;

        let stats = &self.category_stats[category_idx];
        stats.count.fetch_add(1, Ordering::Relaxed);
        stats.bytes.fetch_add(size, Ordering::Relaxed);
        stats.open_nanos.fetch_add(open_nanos, Ordering::Relaxed);
        stats.read_nanos.fetch_add(read_nanos, Ordering::Relaxed);

        // Update per-directory stats (with bounded capacity)
        if let Some(parent) = path.parent() {
            let parent_str = parent.to_string_lossy().to_string();
            let total_nanos = open_nanos + read_nanos;

            if let Ok(mut stats) = self.dir_stats.lock() {
                // Only track up to MAX_DIR_STATS_ENTRIES directories to prevent unbounded growth
                if stats.len() < MAX_DIR_STATS_ENTRIES || stats.contains_key(&parent_str) {
                    let entry = stats.entry(parent_str).or_insert((0, 0, 0));
                    entry.0 += 1;
                    entry.1 += size;
                    entry.2 += total_nanos;
                } else {
                    // Directory limit reached, count this as dropped
                    self.dirs_dropped.fetch_add(1, Ordering::Relaxed);
                }
            }

            // Track depth relative to base paths
            if let Some(depth) = self.calculate_relative_depth(path) {
                let capped_depth = depth.min(19);
                self.depth_histogram[capped_depth].fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Update peak memory if the current value is higher
    fn update_peak_memory(&self, current_bytes: u64) {
        let mut current_peak = self.peak_memory_bytes.load(Ordering::Relaxed);
        while current_bytes > current_peak {
            match self.peak_memory_bytes.compare_exchange_weak(
                current_peak,
                current_bytes,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_peak = actual,
            }
        }
    }
}

/// Counters for tracking traversal progress.
/// All atomic counters are wrapped in Arc for efficient sharing across tasks.
/// Cloning this struct is cheap - it only clones the Arc references, not the underlying data.
#[derive(Clone)]
struct TraversalCounters {
    files: Arc<AtomicUsize>,
    dirs: Arc<AtomicUsize>,
    symlinks_skipped: Arc<AtomicUsize>,
    symlinks_traversed: Arc<AtomicUsize>,
    files_read: Arc<AtomicUsize>,
    total_dir_entries: Arc<AtomicUsize>,
    total_bytes_read: Arc<AtomicU64>,
    dir_read_duration_nanos: Arc<AtomicU64>,
    file_read_duration_nanos: Arc<AtomicU64>,
    file_open_duration_nanos: Arc<AtomicU64>,
    queue_size: Arc<AtomicUsize>,
    start_time: Instant,
    advanced_stats: Option<Arc<AdvancedStats>>,
}

struct Traversal {
    // Counters
    counters: TraversalCounters,

    // Configuration
    max_files: usize,
    follow_symlinks: bool,
    read_mode: ReadMode,
    cancellation_token: CancellationToken,

    // Task handles
    reader_handle: Option<JoinHandle<()>>,
    traversal_handle: Option<JoinHandle<Result<()>>>,
    progress_handle: Option<JoinHandle<()>>,
}

// ============================================================================
// Type Implementations
// ============================================================================

impl TraversalCounters {
    fn new(detailed_stats: bool, base_paths: Vec<PathBuf>) -> Self {
        let advanced_stats = if detailed_stats {
            Some(AdvancedStats::new(base_paths))
        } else {
            None
        };

        Self {
            files: Arc::new(AtomicUsize::new(0)),
            dirs: Arc::new(AtomicUsize::new(0)),
            symlinks_skipped: Arc::new(AtomicUsize::new(0)),
            symlinks_traversed: Arc::new(AtomicUsize::new(0)),
            files_read: Arc::new(AtomicUsize::new(0)),
            total_dir_entries: Arc::new(AtomicUsize::new(0)),
            total_bytes_read: Arc::new(AtomicU64::new(0)),
            dir_read_duration_nanos: Arc::new(AtomicU64::new(0)),
            file_read_duration_nanos: Arc::new(AtomicU64::new(0)),
            file_open_duration_nanos: Arc::new(AtomicU64::new(0)),
            queue_size: Arc::new(AtomicUsize::new(0)),
            start_time: Instant::now(),
            advanced_stats,
        }
    }

    fn total_elapsed_time(&self) -> f64 {
        self.start_time.elapsed().as_secs_f64()
    }

    fn report_progress(
        &self,
        max_files: usize,
        read_mode: &ReadMode,
        system_info: Option<(f64, f64)>, // (memory_mb, cpu_usage)
    ) -> String {
        let elapsed = self.start_time.elapsed().as_secs_f64();
        let files_val = self.files.load(Ordering::Relaxed);
        let dirs_val = self.dirs.load(Ordering::Relaxed);
        let total_bytes_read_val = self.total_bytes_read.load(Ordering::Relaxed);
        let queue_size_val = self.queue_size.load(Ordering::Relaxed);

        let files_per_second = files_val as f64 / elapsed;

        let files_display = if max_files == usize::MAX {
            files_val.to_formatted_string(&Locale::en)
        } else {
            format!(
                "{}/{}",
                files_val.to_formatted_string(&Locale::en),
                max_files.to_formatted_string(&Locale::en)
            )
        };

        let show_throughput = !match read_mode {
            ReadMode::Skip => true,
            _ => false,
        };

        if let Some((memory_mb, cpu_usage)) = system_info {
            if show_throughput {
                let mb_per_second =
                    total_bytes_read_val as f64 / types::BYTES_IN_MEGABYTE as f64 / elapsed;
                format!(
                    "{} files | {} dirs | {} files/s | {:.2} MiB/s | queue: {} | {:.2} MiB memory | {:.2}% CPU",
                    files_display,
                    dirs_val.to_formatted_string(&Locale::en),
                    (files_per_second as u64).to_formatted_string(&Locale::en),
                    mb_per_second,
                    queue_size_val.to_formatted_string(&Locale::en),
                    memory_mb,
                    cpu_usage
                )
            } else {
                format!(
                    "{} files | {} dirs | {} files/s | queue: {} | {:.2} MiB memory | {:.2}% CPU",
                    files_display,
                    dirs_val.to_formatted_string(&Locale::en),
                    (files_per_second as u64).to_formatted_string(&Locale::en),
                    queue_size_val.to_formatted_string(&Locale::en),
                    memory_mb,
                    cpu_usage
                )
            }
        } else if show_throughput {
            let mb_per_second =
                total_bytes_read_val as f64 / types::BYTES_IN_MEGABYTE as f64 / elapsed;
            format!(
                "{} files | {} dirs | {} files/s | {:.2} MiB/s | queue: {}",
                files_display,
                dirs_val.to_formatted_string(&Locale::en),
                (files_per_second as u64).to_formatted_string(&Locale::en),
                mb_per_second,
                queue_size_val.to_formatted_string(&Locale::en)
            )
        } else {
            format!(
                "{} files | {} dirs | {} files/s | queue: {}",
                files_display,
                dirs_val.to_formatted_string(&Locale::en),
                (files_per_second as u64).to_formatted_string(&Locale::en),
                queue_size_val.to_formatted_string(&Locale::en)
            )
        }
    }

    fn report_benchmark(&self, read_mode: &ReadMode) -> Result<Benchmark> {
        let mut result = Benchmark::new(BenchmarkType::FsTraversal);

        let total_elapsed_time = self.total_elapsed_time();

        if total_elapsed_time <= 0.0 {
            return Err(anyhow::anyhow!(
                "Total elapsed time is less or equal to zero."
            ));
        }

        let files = self.files.load(Ordering::Relaxed);
        let dirs = self.dirs.load(Ordering::Relaxed);
        let total_dir_entries = self.total_dir_entries.load(Ordering::Relaxed);
        let dir_read_duration_nanos = self.dir_read_duration_nanos.load(Ordering::Relaxed);

        let files_per_second = files as f64 / total_elapsed_time;

        let avg_read_dir_latency = if dirs > 0 {
            (dir_read_duration_nanos as f64 / 1_000_000.0) / dirs as f64
        } else {
            0.0
        };

        let avg_dir_size = if dirs > 0 {
            total_dir_entries as f64 / dirs as f64
        } else {
            0.0
        };

        result.add_metric(
            "Traversal throughput",
            files_per_second,
            types::Unit::FilesPerSecond,
            Some(0),
        );
        result.add_metric(
            "Average read_dir() latency",
            avg_read_dir_latency,
            types::Unit::Ms,
            Some(4),
        );
        result.add_metric(
            "Average directory size",
            avg_dir_size,
            types::Unit::Files,
            Some(2),
        );
        result.add_metric(
            "Total files scanned",
            files as f64,
            types::Unit::Files,
            Some(0),
        );
        result.add_metric("Total directories", dirs as f64, types::Unit::Dirs, Some(0));
        result.add_metric(
            "Total symlinks skipped",
            self.symlinks_skipped.load(Ordering::Relaxed) as f64,
            types::Unit::Symlinks,
            Some(0),
        );
        result.add_metric(
            "Total symlinks traversed",
            self.symlinks_traversed.load(Ordering::Relaxed) as f64,
            types::Unit::Symlinks,
            Some(0),
        );

        let files_read = self.files_read.load(Ordering::Relaxed);
        let total_bytes_read = self.total_bytes_read.load(Ordering::Relaxed);

        if files_read > 0 {
            let avg_file_size = total_bytes_read as f64 / files_read as f64;
            let avg_file_size_kb = avg_file_size / types::BYTES_IN_KILOBYTE as f64;

            result.add_metric(
                "Total files read",
                files_read as f64,
                types::Unit::Files,
                Some(0),
            );
            result.add_metric(
                "Average file size",
                avg_file_size_kb,
                types::Unit::KiB,
                Some(2),
            );
            result.add_metric(
                "Total data read",
                total_bytes_read as f64 / types::BYTES_IN_MEGABYTE as f64,
                types::Unit::MiB,
                Some(2),
            );

            match read_mode {
                ReadMode::Thrift { .. } => {
                    let file_read_duration_nanos =
                        self.file_read_duration_nanos.load(Ordering::Relaxed);
                    let total_read_duration_secs =
                        file_read_duration_nanos as f64 / 1_000_000_000.0;
                    let avg_read_dur_ms =
                        (file_read_duration_nanos as f64 / 1_000_000.0) / files_read as f64;
                    let mb_per_second = total_bytes_read as f64
                        / types::BYTES_IN_MEGABYTE as f64
                        / total_read_duration_secs;

                    result.add_metric("Throughput", mb_per_second, types::Unit::MiBps, Some(2));
                    result.add_metric(
                        "Average thrift read latency",
                        avg_read_dur_ms,
                        types::Unit::Ms,
                        Some(4),
                    );
                }
                ReadMode::Fs => {
                    let file_open_duration_nanos =
                        self.file_open_duration_nanos.load(Ordering::Relaxed);
                    let file_read_duration_nanos =
                        self.file_read_duration_nanos.load(Ordering::Relaxed);
                    let total_duration_nanos = file_open_duration_nanos + file_read_duration_nanos;
                    let total_duration_secs = total_duration_nanos as f64 / 1_000_000_000.0;

                    let avg_open_dur_ms =
                        (file_open_duration_nanos as f64 / 1_000_000.0) / files_read as f64;
                    let avg_read_dur_ms =
                        (file_read_duration_nanos as f64 / 1_000_000.0) / files_read as f64;
                    let mb_per_second = total_bytes_read as f64
                        / types::BYTES_IN_MEGABYTE as f64
                        / total_duration_secs;

                    result.add_metric(
                        "open() + read() throughput",
                        mb_per_second,
                        types::Unit::MiBps,
                        Some(2),
                    );
                    result.add_metric("open() latency", avg_open_dur_ms, types::Unit::Ms, Some(4));
                    result.add_metric(
                        "Average read() latency",
                        avg_read_dur_ms,
                        types::Unit::Ms,
                        Some(4),
                    );
                }
                ReadMode::Skip => {}
            }
        }

        Ok(result)
    }
}

impl Traversal {
    fn new(
        max_files: usize,
        follow_symlinks: bool,
        read_mode: ReadMode,
        detailed_stats: bool,
        base_paths: Vec<PathBuf>,
    ) -> Self {
        // Create internal components
        let cancellation_token = setup_cancellation();

        Self {
            counters: TraversalCounters::new(detailed_stats, base_paths),
            max_files,
            follow_symlinks,
            read_mode,
            cancellation_token,
            reader_handle: None,
            traversal_handle: None,
            progress_handle: None,
        }
    }

    async fn read_file(
        path: PathBuf,
        read_mode: &ReadMode,
        buffer: &mut Vec<u8>,
        counters: &TraversalCounters,
    ) -> Result<()> {
        match read_mode {
            ReadMode::Skip => Ok(()),
            ReadMode::Fs => {
                // Use tokio::fs for async file I/O
                buffer.clear();

                let start = Instant::now();
                let file_result = tokio::fs::File::open(&path).await;
                let open_elapsed = start.elapsed();

                match file_result {
                    Ok(mut file) => {
                        let start = Instant::now();

                        match tokio::io::AsyncReadExt::read_to_end(&mut file, buffer).await {
                            Ok(bytes_read) => {
                                let read_elapsed = start.elapsed();

                                counters
                                    .file_open_duration_nanos
                                    .fetch_add(open_elapsed.as_nanos() as u64, Ordering::Relaxed);
                                counters
                                    .file_read_duration_nanos
                                    .fetch_add(read_elapsed.as_nanos() as u64, Ordering::Relaxed);
                                counters
                                    .total_bytes_read
                                    .fetch_add(bytes_read as u64, Ordering::Relaxed);
                                counters.files_read.fetch_add(1, Ordering::Relaxed);

                                // Record detailed statistics
                                if let Some(stats) = &counters.advanced_stats {
                                    stats.record_file(
                                        &path,
                                        bytes_read as u64,
                                        open_elapsed.as_nanos() as u64,
                                        read_elapsed.as_nanos() as u64,
                                    );
                                }
                            }
                            Err(_) => {
                                // Silently ignore read errors
                            }
                        }
                    }
                    Err(_) => {
                        // Silently ignore open errors (file might have been deleted, permission issues, etc.)
                    }
                }

                Ok(())
            }
            ReadMode::Thrift { client, repo_path } => {
                let abs_path = path.canonicalize()?;
                let rel_file_path = abs_path
                    .strip_prefix(repo_path)
                    .map_err(|_| {
                        anyhow::anyhow!(
                            "File path does not start with repo path (after canonicalization)"
                        )
                    })?
                    .to_path_buf();

                let start = Instant::now();
                let request = get_thrift_request(repo_path.clone(), rel_file_path)?;
                let response = client
                    .with_thrift(|thrift| {
                        (
                            thrift.getFileContent(&request),
                            EdenThriftMethod::GetFileContent,
                        )
                    })
                    .await?;
                let read_elapsed = start.elapsed();
                counters
                    .file_read_duration_nanos
                    .fetch_add(read_elapsed.as_nanos() as u64, Ordering::Relaxed);

                match response.blob {
                    ScmBlobOrError::blob(blob) => {
                        let bytes_read = blob.len() as u64;
                        counters
                            .total_bytes_read
                            .fetch_add(bytes_read, Ordering::Relaxed);
                        counters.files_read.fetch_add(1, Ordering::Relaxed);

                        // Record detailed statistics (no open duration for thrift)
                        if let Some(stats) = &counters.advanced_stats {
                            stats.record_file(&path, bytes_read, 0, read_elapsed.as_nanos() as u64);
                        }
                    }
                    _ => {}
                }

                Ok(())
            }
        }
    }

    fn start_reader_task(&mut self) -> mpsc::UnboundedSender<PathBuf> {
        let (file_sender, mut file_receiver) = mpsc::unbounded_channel::<PathBuf>();

        // Clone the fields needed for the reader task
        let read_mode = self.read_mode.clone();
        let cancellation_token = self.cancellation_token.clone();
        let counters = self.counters.clone();

        let handle = tokio::spawn(async move {
            let mut buffer = Vec::with_capacity(types::BYTES_IN_MEGABYTE);

            loop {
                if cancellation_token.is_cancelled() {
                    break;
                }

                match file_receiver.recv().await {
                    Some(path) => {
                        counters.queue_size.fetch_sub(1, Ordering::Relaxed);
                        let _ = Self::read_file(path, &read_mode, &mut buffer, &counters).await;
                    }
                    None => {
                        // Channel closed, exit reader
                        break;
                    }
                }
            }
        });

        self.reader_handle = Some(handle);
        file_sender
    }

    fn start_traversal_task(
        &mut self,
        paths: Vec<PathBuf>,
        file_sender: mpsc::UnboundedSender<PathBuf>,
    ) {
        let max_files = self.max_files;
        let follow_symlinks = self.follow_symlinks;
        let cancellation_token = self.cancellation_token.clone();
        let counters = self.counters.clone();

        let handle = tokio::task::spawn_blocking(move || {
            Self::traverse_directories_blocking(
                paths,
                max_files,
                follow_symlinks,
                file_sender,
                &counters,
                cancellation_token,
            )
        });

        self.traversal_handle = Some(handle);
    }

    fn traverse_directories_blocking(
        paths: Vec<PathBuf>,
        max_files: usize,
        follow_symlinks: bool,
        file_sender: mpsc::UnboundedSender<PathBuf>,
        counters: &TraversalCounters,
        cancellation_token: CancellationToken,
    ) -> Result<()> {
        let mut stack = paths;

        while let Some(current_path) = stack.pop() {
            if cancellation_token.is_cancelled() {
                return Ok(());
            }

            counters.dirs.fetch_add(1, Ordering::Relaxed);

            let start_time = Instant::now();
            let entries = fs::read_dir(&current_path)?;
            let duration = start_time.elapsed();
            counters
                .dir_read_duration_nanos
                .fetch_add(duration.as_nanos() as u64, Ordering::Relaxed);

            let mut entry_count = 0;

            for entry_result in entries {
                let entry = entry_result?;
                let entry_path = entry.path();
                let file_type = entry.file_type()?;
                entry_count += 1;

                if file_type.is_dir() {
                    if file_type.is_symlink() {
                        if follow_symlinks {
                            counters.symlinks_traversed.fetch_add(1, Ordering::Relaxed);
                            stack.push(entry_path);
                        } else {
                            counters.symlinks_skipped.fetch_add(1, Ordering::Relaxed);
                        }
                    } else {
                        stack.push(entry_path);
                    }
                } else if file_type.is_file() {
                    let current_file_count = counters.files.fetch_add(1, Ordering::Relaxed);
                    if current_file_count < max_files {
                        if file_sender.send(entry_path).is_err() {
                            // Channel closed, stop traversal
                            counters
                                .total_dir_entries
                                .fetch_add(entry_count, Ordering::Relaxed);
                            return Ok(());
                        }
                        counters.queue_size.fetch_add(1, Ordering::Relaxed);
                    } else {
                        // Reached max files
                        counters
                            .total_dir_entries
                            .fetch_add(entry_count, Ordering::Relaxed);
                        return Ok(());
                    }
                }
            }

            counters
                .total_dir_entries
                .fetch_add(entry_count, Ordering::Relaxed);
        }

        Ok(())
    }

    fn start_progress_task(
        &mut self,
        no_progress: bool,
        resource_usage: bool,
        traversal_handle: JoinHandle<Result<()>>,
        reader_handle: JoinHandle<()>,
    ) {
        let max_files = self.max_files;
        let read_mode = self.read_mode.clone();
        let counters = self.counters.clone();

        let handle = tokio::spawn(async move {
            tokio::pin!(traversal_handle);
            tokio::pin!(reader_handle);

            if no_progress {
                // Just wait for completion, no progress bar
                let _ = traversal_handle.await;
                let _ = reader_handle.await;
            } else {
                // Display progress bar while waiting
                let pb = ProgressBar::new_spinner();
                pb.set_style(
                    ProgressStyle::default_spinner()
                        .template("[{elapsed_precise}] {msg}")
                        .unwrap(),
                );

                let initial_files_display = if max_files == usize::MAX {
                    "0"
                } else {
                    &format!("0/{}", max_files.to_formatted_string(&Locale::en))
                };
                pb.set_message(format!(
                    "{} files | 0 dirs | 0 files/s",
                    initial_files_display
                ));

                let (mut system, pid) = if resource_usage || counters.advanced_stats.is_some() {
                    let mut sys = System::new_all();
                    let process_id = sysinfo::get_current_pid().ok();
                    sys.refresh_all();
                    (Some(sys), process_id)
                } else {
                    (None, None)
                };

                let mut interval = tokio::time::interval(std::time::Duration::from_secs(
                    types::PROGRESS_BAR_UPDATE_INTERVAL_SECS,
                ));

                let mut traversal_done = false;
                let mut reader_done = false;

                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let elapsed = counters.start_time.elapsed().as_secs_f64();
                            if elapsed > 0.0 {
                                let system_info = if let (Some(sys), Some(pid_val)) = (&mut system, pid) {
                                    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid_val]), false);
                                    sys.process(pid_val).map(|process| {
                                        let memory_bytes = process.memory();
                                        let memory_mb = memory_bytes as f64 / types::BYTES_IN_MEGABYTE as f64;
                                        let cpu_usage = process.cpu_usage() as f64;

                                        // Update peak memory if detailed stats are enabled
                                        if let Some(stats) = &counters.advanced_stats {
                                            stats.update_peak_memory(memory_bytes);
                                        }

                                        (memory_mb, cpu_usage)
                                    })
                                } else {
                                    None
                                };

                                let message = counters.report_progress(max_files, &read_mode, system_info);
                                pb.set_message(message);
                            }

                            if traversal_done && reader_done {
                                pb.finish_and_clear();
                                break;
                            }
                        }
                        _ = &mut traversal_handle, if !traversal_done => {
                            traversal_done = true;
                        }
                        _ = &mut reader_handle, if !reader_done => {
                            reader_done = true;
                        }
                    }
                }
            }
        });

        self.progress_handle = Some(handle);
    }

    async fn wait_for_completion(&mut self) {
        // Progress task waits for everything, so just wait for it
        if let Some(handle) = self.progress_handle.take() {
            let _ = handle.await;
        }
    }
}

// ============================================================================
// Public Functions
// ============================================================================

pub async fn bench_traversal(
    dir_paths: &[String],
    max_files: usize,
    follow_symlinks: bool,
    no_progress: bool,
    resource_usage: bool,
    skip_read: bool,
    thrift_io: Option<&str>,
    detailed_read_stats: bool,
) -> Result<Benchmark> {
    // Validate all directories first
    for dir_path in dir_paths {
        let path = Path::new(dir_path);
        if !path.exists() || !path.is_dir() {
            return Err(anyhow::anyhow!("Invalid directory path: {}", dir_path));
        }
    }

    let read_mode = if skip_read {
        ReadMode::Skip
    } else if let Some(fbsource_path) = thrift_io {
        let client = get_edenfs_instance().get_client();
        let repo_path = PathBuf::from(fbsource_path).canonicalize()?;
        ReadMode::Thrift { client, repo_path }
    } else {
        ReadMode::Fs
    };

    // Convert to PathBuf and prepare base paths for detailed stats
    let paths: Vec<PathBuf> = dir_paths.iter().map(PathBuf::from).collect();

    let mut traversal = Traversal::new(
        max_files,
        follow_symlinks,
        read_mode.clone(),
        detailed_read_stats,
        paths.clone(),
    );

    // Start reader task, get sender
    let file_sender = traversal.start_reader_task();

    // Start traversal task, passing sender
    traversal.start_traversal_task(paths, file_sender);

    // Extract handles
    let reader_handle = traversal
        .reader_handle
        .take()
        .expect("reader_handle must exist");
    let traversal_handle = traversal
        .traversal_handle
        .take()
        .expect("traversal_handle must exist");

    traversal.start_progress_task(no_progress, resource_usage, traversal_handle, reader_handle);

    traversal.wait_for_completion().await;

    let result = traversal.counters.report_benchmark(&read_mode)?;

    if detailed_read_stats {
        if let Some(stats) = &traversal.counters.advanced_stats {
            print_detailed_read_statistics(stats);
        }
    }

    Ok(result)
}

// ============================================================================
// Private Helper Functions
// ============================================================================

fn print_detailed_read_statistics(stats: &Arc<AdvancedStats>) {
    println!("\n=== DETAILED READ STATISTICS ===\n");

    println!("File Size Distribution:");
    let bucket_names = [
        "<1 KB",
        "1-10 KB",
        "10-100 KB",
        "100 KB-1 MB",
        "1-10 MB",
        "10-100 MB",
        ">100 MB",
    ];

    // Calculate totals first to get correct percentages
    let (total_files, total_bytes): (usize, u64) = (0..7)
        .map(|i| {
            (
                stats.size_histogram[i].load(Ordering::Relaxed),
                stats.size_histogram_bytes[i].load(Ordering::Relaxed),
            )
        })
        .fold((0, 0), |(tf, tb), (c, b)| (tf + c, tb + b));

    for (bucket_name, (histogram, histogram_bytes)) in bucket_names.iter().zip(
        stats
            .size_histogram
            .iter()
            .zip(stats.size_histogram_bytes.iter()),
    ) {
        let count = histogram.load(Ordering::Relaxed);
        let bytes = histogram_bytes.load(Ordering::Relaxed);

        if count > 0 {
            let avg_size = bytes as f64 / count as f64;
            println!(
                "  {:>12}: {:>10} files ({:>6.2}%) | {:>10.2} MB total | {:>8.2} KB avg",
                bucket_name,
                count.to_formatted_string(&Locale::en),
                (count as f64 / total_files.max(1) as f64) * 100.0,
                bytes as f64 / types::BYTES_IN_MEGABYTE as f64,
                avg_size / 1024.0
            );
        }
    }

    for category in [
        FileSizeCategory::Small,
        FileSizeCategory::Medium,
        FileSizeCategory::Large,
    ] {
        let category_stats = &stats.category_stats[category as usize];
        let count = category_stats.count.load(Ordering::Relaxed);
        let bytes = category_stats.bytes.load(Ordering::Relaxed);
        let open_nanos = category_stats.open_nanos.load(Ordering::Relaxed);
        let read_nanos = category_stats.read_nanos.load(Ordering::Relaxed);

        println!(
            "\n{} File Performance ({} files):",
            category.name(),
            category.description()
        );

        if count > 0 {
            let avg_open_us = (open_nanos as f64 / count as f64) / 1000.0;
            let avg_read_us = (read_nanos as f64 / count as f64) / 1000.0;
            let avg_read_ms = (read_nanos as f64 / count as f64) / 1_000_000.0;
            let mb = bytes as f64 / types::BYTES_IN_MEGABYTE as f64;
            let total_time_s = (open_nanos + read_nanos) as f64 / 1_000_000_000.0;
            let throughput = if total_time_s > 0.0 {
                mb / total_time_s
            } else {
                0.0
            };

            println!(
                "  Files:           {}",
                count.to_formatted_string(&Locale::en)
            );
            println!("  Total Size:      {:.2} MB", mb);
            println!("  Avg open():      {:.1} µs", avg_open_us);

            // Use microseconds for small/medium, milliseconds for large files
            match category {
                FileSizeCategory::Large => println!("  Avg read():      {:.1} ms", avg_read_ms),
                _ => println!("  Avg read():      {:.1} µs", avg_read_us),
            }

            println!("  Throughput:      {:.2} MB/s", throughput);
            println!(
                "  Overhead:        {:.1}% of time in open()",
                (open_nanos as f64 / (open_nanos + read_nanos).max(1) as f64) * 100.0
            );
        } else {
            println!("  No {} files found", category.name().to_lowercase());
        }
    }

    println!("\nTop 20 Slowest Directories for Reading:");
    let dirs_tracked = if let Ok(dir_stats) = stats.dir_stats.lock() {
        let mut dir_vec: Vec<_> = dir_stats.iter().collect();
        dir_vec.sort_by(|a, b| b.1.2.cmp(&a.1.2)); // Sort by total duration descending

        for (i, (dir, (file_count, bytes, nanos))) in dir_vec.iter().take(20).enumerate() {
            let time_ms = *nanos as f64 / 1_000_000.0;
            let mb = *bytes as f64 / types::BYTES_IN_MEGABYTE as f64;
            let throughput = if time_ms > 0.0 {
                mb / (time_ms / 1000.0)
            } else {
                0.0
            };

            let relative_dir = stats.make_path_relative(dir);

            println!(
                "  {:>2}. {} files | {:.2} MB | {:.1} ms ({:.2} MB/s) | {}",
                i + 1,
                file_count.to_formatted_string(&Locale::en),
                mb,
                time_ms,
                throughput,
                relative_dir
            );
        }
        dir_stats.len()
    } else {
        0
    };

    // Show directory tracking info
    let dirs_dropped = stats.dirs_dropped.load(Ordering::Relaxed);
    println!(
        "\n  Directories tracked: {}",
        dirs_tracked.to_formatted_string(&Locale::en)
    );
    if dirs_dropped > 0 {
        println!(
            "  WARNING: {} file records from additional directories were not tracked (limit: {})",
            dirs_dropped.to_formatted_string(&Locale::en),
            MAX_DIR_STATS_ENTRIES.to_formatted_string(&Locale::en)
        );
    }

    // Directory Depth Analysis
    println!("\nDirectory Depth Distribution (relative to base paths):");
    let depth_data: Vec<(usize, usize)> = stats
        .depth_histogram
        .iter()
        .enumerate()
        .map(|(depth, count)| (depth, count.load(Ordering::Relaxed)))
        .filter(|(_, count)| *count > 0)
        .collect();

    if !depth_data.is_empty() {
        for (depth, count) in depth_data.iter() {
            println!(
                "  Depth {:>2}: {:>10} files",
                depth,
                count.to_formatted_string(&Locale::en)
            );
        }
    }

    // Summary
    println!("\nSummary:");
    println!(
        "  Total Files:       {}",
        total_files.to_formatted_string(&Locale::en)
    );
    println!(
        "  Total Data Read:   {:.2} MB",
        total_bytes as f64 / types::BYTES_IN_MEGABYTE as f64
    );

    let peak_memory = stats.peak_memory_bytes.load(Ordering::Relaxed);
    if peak_memory > 0 {
        println!(
            "  Peak Memory:       {:.2} MB",
            peak_memory as f64 / types::BYTES_IN_MEGABYTE as f64
        );
    }

    println!("\n===========================\n");
}

fn get_thrift_request(
    repo_path: PathBuf,
    rel_file_path: PathBuf,
) -> Result<thrift_types::edenfs::GetFileContentRequest> {
    let req = thrift_types::edenfs::GetFileContentRequest {
        mount: MountId {
            mountPoint: bytes_from_path(repo_path)?,
            ..Default::default()
        },
        filePath: bytes_from_path(rel_file_path)?,
        sync: SyncBehavior {
            ..Default::default()
        },
        ..Default::default()
    };
    Ok(req)
}

fn setup_cancellation() -> CancellationToken {
    let token = CancellationToken::new();

    let token_clone = token.clone();
    if let Err(err) = ctrlc::set_handler(move || {
        token_clone.cancel();
    }) {
        eprintln!("Failed to set Ctrl+C handler: {}", err);
    }

    token
}
