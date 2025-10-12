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

// ============================================================================
// Type Definitions
// ============================================================================

#[derive(Clone)]
enum ReadMode {
    Skip,
    Fs,
    Thrift {
        client: Arc<edenfs_client::client::EdenFsClient>,
        repo_path: PathBuf,
    },
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
    fn new() -> Self {
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
    fn new(max_files: usize, follow_symlinks: bool, read_mode: ReadMode) -> Self {
        // Create internal components
        let cancellation_token = setup_cancellation();

        Self {
            counters: TraversalCounters::new(),
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
                        counters
                            .total_bytes_read
                            .fetch_add(blob.len() as u64, Ordering::Relaxed);
                        counters.files_read.fetch_add(1, Ordering::Relaxed);
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

                let (mut system, pid) = if resource_usage {
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
                                        let memory_mb = process.memory() as f64 / types::BYTES_IN_MEGABYTE as f64;
                                        let cpu_usage = process.cpu_usage() as f64;
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

    let mut traversal = Traversal::new(max_files, follow_symlinks, read_mode.clone());

    // Start reader task, get sender
    let file_sender = traversal.start_reader_task();

    // Start traversal task, passing sender
    let paths: Vec<PathBuf> = dir_paths.iter().map(PathBuf::from).collect();
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

    // Start progress task with handles
    traversal.start_progress_task(no_progress, resource_usage, traversal_handle, reader_handle);

    // Wait for everything to complete
    traversal.wait_for_completion().await;

    traversal.counters.report_benchmark(&read_mode)
}

// ============================================================================
// Private Helper Functions
// ============================================================================

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
