/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Rotation support for a set of [`Log`]s.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::atomic::Ordering::SeqCst;

use minibytes::Bytes;
use once_cell::sync::OnceCell;
use tracing::debug;
use tracing::debug_span;
use tracing::trace;

use crate::change_detect::SharedChangeDetector;
use crate::errors::IoResultExt;
use crate::errors::ResultExt;
use crate::lock::READER_LOCK_OPTS;
use crate::lock::ScopedDirLock;
use crate::log;
use crate::log::ExtendWrite;
use crate::log::FlushFilterContext;
use crate::log::FlushFilterFunc;
use crate::log::FlushFilterOutput;
use crate::log::IndexDef;
use crate::log::Log;
use crate::repair::OpenOptionsOutput;
use crate::repair::OpenOptionsRepair;
use crate::repair::RepairMessage;
use crate::utils;

pub static ROTATE_COUNT: AtomicU64 = AtomicU64::new(0);

/// A collection of [`Log`]s that get rotated or deleted automatically when they
/// exceed size or count limits.
///
/// Writes go to the active [`Log`]. Reads scan through all [`Log`]s.
pub struct RotateLog {
    dir: Option<PathBuf>,
    open_options: OpenOptions,
    logs: Vec<OnceCell<Log>>,
    // Logical length of `logs`. It can be smaller than `logs.len()` if some Log
    // fails to load.
    logs_len: AtomicUsize,
    // Rotated logs we hold on to in "consistent read" mode to provide temporary read-after-write
    // consistency.
    pinned_logs: Vec<Log>,
    latest: u8,
    // Indicate an active reader. Destrictive writes (repair) are unsafe.
    reader_lock: Option<ScopedDirLock>,
    change_detector: Option<SharedChangeDetector>,

    // At what apparent file size we should check the btrfs "physical" size when checking if we
    // should rotate. This is to minimize checks of the btrfs size.
    next_btrfs_size_check: Option<u64>,

    // Run after log.sync(). For testing purpose only.
    #[cfg(test)]
    hook_after_log_sync: Option<Box<dyn Fn()>>,

    // Count of active "with_consistent_reads()" calls.
    consistent_reads: Arc<AtomicU64>,
}

// On disk, a RotateLog is a directory containing:
// - 0/, 1/, 2/, 3/, ...: one Log per directory.
// - latest: a file, the name of the directory that is considered "active".

const LATEST_FILE: &str = "latest";

/// Options used to configure how a [`RotateLog`] is opened.
#[derive(Clone)]
pub struct OpenOptions {
    pub(crate) max_bytes_per_log: u64,
    pub(crate) max_log_count: u8,
    pub(crate) log_open_options: log::OpenOptions,
    pub(crate) auto_sync_threshold: Option<u64>,
}

impl OpenOptions {
    #[allow(clippy::new_without_default)]
    /// Creates a default new set of options ready for configuration.
    ///
    /// The default values are:
    /// - Keep 2 logs.
    /// - A log gets rotated when it exceeds 2GB.
    /// - No indexes.
    /// - Do not create on demand.
    /// - Do not sync automatically on append().
    pub fn new() -> Self {
        // Some "seemingly reasonable" default values. Not scientifically chosen.
        let max_log_count = 2;
        let max_bytes_per_log = 2_000_000_000; // 2 GB
        Self {
            max_bytes_per_log,
            max_log_count,
            log_open_options: log::OpenOptions::new(),
            auto_sync_threshold: None,
        }
    }

    /// Set the maximum [`Log`] count.
    ///
    /// A larger value would hurt lookup performance.
    pub fn max_log_count(mut self, count: u8) -> Self {
        assert!(count >= 1);
        self.max_log_count = count;
        self
    }

    /// Set the maximum bytes per [`Log`].
    pub fn max_bytes_per_log(mut self, bytes: u64) -> Self {
        assert!(bytes > 0);
        self.max_bytes_per_log = bytes;
        self
    }

    /// Set `fysnc` open for underlying [`Log`] objects.
    pub fn fsync(mut self, fsync: bool) -> Self {
        self.log_open_options = self.log_open_options.fsync(fsync);
        self
    }

    /// Sets the checksum type.
    ///
    /// See [log::ChecksumType] for details.
    pub fn checksum_type(mut self, checksum_type: log::ChecksumType) -> Self {
        self.log_open_options = self.log_open_options.checksum_type(checksum_type);
        self
    }

    /// Set whether create the [`RotateLog`] structure if it does not exist.
    pub fn create(mut self, create: bool) -> Self {
        self.log_open_options = self.log_open_options.create(create);
        self
    }

    /// Add an index function.
    pub fn index(mut self, name: &'static str, func: fn(&[u8]) -> Vec<log::IndexOutput>) -> Self {
        self.log_open_options = self.log_open_options.index(name, func);
        self
    }

    /// Set the index definitions.
    ///
    /// See [`IndexDef`] for details.
    pub fn index_defs(mut self, index_defs: Vec<IndexDef>) -> Self {
        self.log_open_options = self.log_open_options.index_defs(index_defs);
        self
    }

    /// Sets the flush filter function.
    ///
    /// The function will be called at [`RotateLog::sync`] time, if there are
    /// changes since `open` (or last `sync`) time.
    ///
    /// The filter function can be used to avoid writing content that already
    /// exists in the latest [`Log`], or rewrite content as needed.
    pub fn flush_filter(mut self, flush_filter: Option<FlushFilterFunc>) -> Self {
        self.log_open_options = self.log_open_options.flush_filter(flush_filter);
        self
    }

    /// Call `sync` automatically if the in-memory buffer size has exceeded
    /// the given size threshold.
    ///
    /// This is useful to make in-memory buffer size bounded.
    pub fn auto_sync_threshold(mut self, threshold: impl Into<Option<u64>>) -> Self {
        self.auto_sync_threshold = threshold.into();
        self
    }

    /// Enable btrfs aware mode where we rotate based on "physical" file size instead of apparent
    /// file size to account for transparent btrfs compression.
    pub fn btrfs_compression(mut self, btrfs: bool) -> Self {
        self.log_open_options = self.log_open_options.btrfs_compression(btrfs);
        self
    }

    /// Open [`RotateLog`] at given location.
    pub fn open(&self, dir: impl AsRef<Path>) -> crate::Result<RotateLog> {
        let dir = dir.as_ref();
        let result: crate::Result<_> = (|| {
            let reader_lock = ScopedDirLock::new_with_options(dir, &READER_LOCK_OPTS)?;
            let change_detector = reader_lock.shared_change_detector()?;
            let span = debug_span!("RotateLog::open", dir = &dir.to_string_lossy().as_ref());
            let _guard = span.enter();

            let latest_and_log = read_latest_and_logs(dir, self);

            let (latest, logs) = match latest_and_log {
                Ok((latest, logs)) => (latest, logs),
                Err(e) => {
                    if !self.log_open_options.create {
                        return Err(e)
                            .context("not creating new logs since OpenOption::create is not set");
                    } else {
                        utils::mkdir_p(dir)?;
                        let lock = ScopedDirLock::new(dir)?;

                        match read_latest_raw(dir) {
                            Ok(latest) => {
                                match read_logs(dir, self, latest) {
                                    Ok(logs) => {
                                        // Both latest and logs are read properly.
                                        (latest, logs)
                                    }
                                    Err(err) => {
                                        // latest is fine, but logs cannot be read.
                                        // Try auto recover by creating an empty log.
                                        let latest = latest.wrapping_add(1);
                                        match create_empty_log(Some(dir), self, latest, &lock) {
                                            Ok(new_log) => {
                                                if let Ok(logs) = read_logs(dir, self, latest) {
                                                    (latest, logs)
                                                } else {
                                                    (latest, vec![create_log_cell(new_log)])
                                                }
                                            }
                                            Err(new_log_err) => {
                                                let msg = "cannot create new empty log after failing to read existing logs";
                                                return Err(new_log_err.message(msg).source(err));
                                            }
                                        }
                                    }
                                }
                            }
                            Err(err) => {
                                if err.kind() == io::ErrorKind::NotFound {
                                    // latest does not exist.
                                    // Most likely, it is a new empty directory.
                                    // Create an empty log and update latest.
                                    let latest = 0;
                                    let new_log = create_empty_log(Some(dir), self, latest, &lock)?;
                                    (latest, vec![create_log_cell(new_log)])
                                } else {
                                    // latest cannot be read for other reasons.
                                    //
                                    // Mark as corrupted, if 'latest' contains a number that cannot be
                                    // parsed.
                                    let corrupted = err.kind() == io::ErrorKind::InvalidData;
                                    let mut result = Err(err).context(dir, "cannot read 'latest'");
                                    if corrupted {
                                        result = result.corruption();
                                    }
                                    return result;
                                }
                            }
                        }
                    }
                }
            };

            let logs_len = AtomicUsize::new(logs.len());
            let mut rotate_log = RotateLog {
                dir: Some(dir.into()),
                open_options: self.clone(),
                logs,
                logs_len,
                pinned_logs: Vec::new(),
                latest,
                reader_lock: Some(reader_lock),
                change_detector: Some(change_detector),
                next_btrfs_size_check: None,
                #[cfg(test)]
                hook_after_log_sync: None,
                consistent_reads: Default::default(),
            };
            rotate_log.update_change_detector_to_match_meta();
            Ok(rotate_log)
        })();

        result.context(|| format!("in rotate::OpenOptions::open({:?})", dir))
    }

    /// Open an-empty [`RotateLog`] in memory. The [`RotateLog`] cannot [`RotateLog::sync`].
    pub fn create_in_memory(&self) -> crate::Result<RotateLog> {
        let result: crate::Result<_> = (|| {
            let cell = create_log_cell(self.log_open_options.open(())?);
            let mut logs = Vec::with_capacity(1);
            logs.push(cell);
            let logs_len = AtomicUsize::new(logs.len());
            Ok(RotateLog {
                dir: None,
                open_options: self.clone(),
                logs,
                logs_len,
                pinned_logs: Vec::new(),
                latest: 0,
                reader_lock: None,
                change_detector: None,
                next_btrfs_size_check: None,
                #[cfg(test)]
                hook_after_log_sync: None,
                consistent_reads: Default::default(),
            })
        })();
        result.context("in rotate::OpenOptions::create_in_memory")
    }

    /// Try repair all logs in the specified directory.
    ///
    /// This just calls into [`log::OpenOptions::repair`] recursively.
    pub fn repair(&self, dir: impl AsRef<Path>) -> crate::Result<String> {
        let dir = dir.as_ref();
        (|| -> crate::Result<_> {
            let _lock = ScopedDirLock::new(dir)?;

            let mut message = RepairMessage::new(dir);
            message += &format!("Processing RotateLog: {:?}\n", dir);
            let read_dir = dir.read_dir().context(dir, "cannot readdir")?;
            let mut ids = Vec::new();

            for entry in read_dir {
                let entry = entry.context(dir, "cannot readdir")?;
                let name = entry.file_name();
                if let Some(name) = name.to_str() {
                    if let Ok(id) = name.parse::<u8>() {
                        ids.push(id);
                    }
                }
            }

            ids.sort_unstable();
            for &id in ids.iter() {
                let name = id.to_string();
                message += &format!("Attempt to repair log {:?}\n", name);
                match self.log_open_options.repair(dir.join(name)) {
                    Ok(log) => message += &log,
                    Err(err) => message += &format!("Failed: {}\n", err),
                }
            }

            let latest_path = dir.join(LATEST_FILE);
            match read_latest_raw(dir) {
                Ok(latest) => message += &format!("Latest = {}\n", latest),
                Err(err) => match err.kind() {
                    io::ErrorKind::NotFound
                    | io::ErrorKind::InvalidData
                    | io::ErrorKind::UnexpectedEof => {
                        let latest = guess_latest(ids);
                        let content = format!("{}", latest);
                        let fsync = false;
                        utils::atomic_write(&latest_path, content, fsync)?;
                        message += &format!("Reset latest to {}\n", latest);
                    }
                    _ => return Err(err).context(&latest_path, "cannot read or parse"),
                },
            };

            Ok(message.into_string())
        })()
        .context(|| format!("in rotate::OpenOptions::repair({:?})", dir))
    }
}

impl OpenOptionsRepair for OpenOptions {
    fn open_options_repair(&self, dir: impl AsRef<Path>) -> crate::Result<String> {
        OpenOptions::repair(self, dir.as_ref())
    }
}

impl OpenOptionsOutput for OpenOptions {
    type Output = RotateLog;

    fn open_path(&self, path: &Path) -> crate::Result<Self::Output> {
        self.open(path)
    }
}

impl fmt::Debug for OpenOptions {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "OpenOptions {{ ")?;
        write!(f, "max_bytes_per_log: {}, ", self.max_bytes_per_log)?;
        write!(f, "max_log_count: {}, ", self.max_log_count)?;
        write!(f, "auto_sync_threshold: {:?}, ", self.auto_sync_threshold)?;
        write!(f, "log_open_options: {:?} }}", &self.log_open_options)?;
        Ok(())
    }
}

impl RotateLog {
    /// Append data to the writable [`Log`].
    pub fn append(&mut self, data: impl AsRef<[u8]>) -> crate::Result<()> {
        self.append_internal(
            |buf| {
                buf.extend_from_slice(data.as_ref());
                crate::Result::Ok(())
            },
            Some(data.as_ref().len()),
        )
    }

    /// Append data directly to the writable [`Log`]'s in-memory buffer.
    pub fn append_direct<E>(
        &mut self,
        cb: impl Fn(&mut dyn ExtendWrite) -> Result<(), E>,
    ) -> crate::Result<()>
    where
        E: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    {
        self.append_internal(cb, None)
    }

    fn append_internal<E>(
        &mut self,
        cb: impl Fn(&mut dyn ExtendWrite) -> Result<(), E>,
        data_len: Option<usize>,
    ) -> crate::Result<()>
    where
        E: Into<Box<dyn std::error::Error + Send + Sync + 'static>>,
    {
        self.maybe_clear_pinned_logs();

        (|| -> crate::Result<_> {
            let threshold = self.open_options.auto_sync_threshold;
            let log = self.writable_log();
            log.append_internal(cb, data_len)?;
            if let Some(threshold) = threshold {
                if log.mem_buf.len() as u64 >= threshold {
                    self.sync()
                        .context("sync triggered by auto_sync_threshold")?;
                }
            }
            Ok(())
        })()
        .context("in RotateLog::append")
    }

    /// Look up an entry using the given index. The `index_id` is the index of
    /// `index_defs` stored in [`OpenOptions`].
    pub fn lookup(
        &self,
        index_id: usize,
        key: impl Into<Bytes>,
    ) -> crate::Result<RotateLogLookupIter<'_>> {
        let key = key.into();
        let result: crate::Result<_> = (|| {
            Ok(RotateLogLookupIter {
                inner_iter: self.logs[0].get().unwrap().lookup(index_id, &key)?,
                end: false,
                log_rotate: self,
                log_index: 0,
                index_id,
                key: key.clone(),
                pinned_logs: if self.is_consistent_reads() {
                    &self.pinned_logs
                } else {
                    &[]
                },
            })
        })();
        result
            .context(|| format!("in RotateLog::lookup({}, {:?})", index_id, key.as_ref()))
            .context(|| format!("  RotateLog.dir = {:?}", self.dir))
    }

    /// Convert a slice to [`Bytes`].
    ///
    /// Do not copy the slice if it's from the main on-disk buffer of
    /// one of the loaded logs.
    pub fn slice_to_bytes(&self, slice: &[u8]) -> Bytes {
        for log in &self.logs {
            if let Some(log) = log.get() {
                if log.disk_buf.range_of_slice(slice).is_some() {
                    return log.slice_to_bytes(slice);
                }
            }
        }
        Bytes::copy_from_slice(slice)
    }

    /// Look up an entry using the given index. The `index_id` is the index of
    /// `index_defs` stored in [`OpenOptions`].
    ///
    /// Unlike [`RotateLog::lookup`], this function only checks the "latest"
    /// (i.e. "writable") [`Log`] without checking others. It is useful to make
    /// sure certain contents depending on other entries are inserted into
    /// the same [`Log`].
    ///
    /// Practically, a `flush_filter` should also be used to make sure dependent
    /// entries are stored in a same [`Log`]. So this function will panic if
    /// `flush_filter` is not set on [`OpenOptions`].
    pub fn lookup_latest(
        &self,
        index_id: usize,
        key: impl AsRef<[u8]>,
    ) -> crate::Result<log::LogLookupIter<'_>> {
        let key = key.as_ref();
        assert!(
            self.open_options.log_open_options.flush_filter.is_some(),
            "programming error: flush_filter should also be set"
        );
        self.logs[0]
            .get()
            .unwrap()
            .lookup(index_id, key)
            .context(|| format!("in RotateLog::lookup_latest({}, {:?})", index_id, key))
            .context(|| format!("  RotateLog.dir = {:?}", self.dir))
    }

    /// Read latest data from disk. Write in-memory entries to disk.
    ///
    /// Return the index of the latest [`Log`].
    ///
    /// For in-memory [`RotateLog`], this function always returns 0.
    pub fn sync(&mut self) -> crate::Result<u8> {
        self.maybe_clear_pinned_logs();

        let result: crate::Result<_> = (|| {
            let span = debug_span!("RotateLog::sync", latest = self.latest as u32);
            if let Some(dir) = &self.dir {
                span.record("dir", dir.to_string_lossy().as_ref());
            }
            let _guard = span.enter();

            if self.dir.is_none() {
                return Ok(0);
            }

            if self.writable_log().iter_dirty().next().is_none() {
                // Read-only path, no need to take directory lock.
                match read_latest(self.dir.as_ref().unwrap()) {
                    Ok(latest) => {
                        if latest != self.latest {
                            // Latest changed. Re-load and write to the real latest Log.
                            // PERF(minor): This can be smarter by avoiding reloading some logs.
                            self.set_logs(
                                latest,
                                read_logs(self.dir.as_ref().unwrap(), &self.open_options, latest)?,
                            );
                        }
                        self.writable_log().sync()?;
                    }
                    Err(err) => {
                        // If latest can not be read, do not error out.
                        // This RotateLog can still be used to answer queries.
                        tracing::error!(?err, "ignoring error reading latest log");
                    }
                }
            } else {
                // Read-write path. Take the directory lock.
                let dir = self.dir.clone().unwrap();
                let lock = ScopedDirLock::new(&dir)?;

                // Re-read latest, since it might have changed after taking the lock.
                let latest = read_latest(self.dir.as_ref().unwrap())?;
                if latest != self.latest {
                    // Latest changed. Re-load and write to the real latest Log.
                    //
                    // This is needed because RotateLog assumes non-latest logs
                    // are read-only. Other processes using RotateLog won't reload
                    // non-latest logs automatically.
                    // PERF(minor): This can be smarter by avoiding reloading some logs.
                    let mut new_logs =
                        read_logs(self.dir.as_ref().unwrap(), &self.open_options, latest)?;
                    if let Some(filter) = self.open_options.log_open_options.flush_filter {
                        let log = new_logs[0].get_mut().unwrap();
                        for entry in self.writable_log().iter_dirty() {
                            let content = entry?;
                            let context = FlushFilterContext { log };
                            match filter(&context, content).map_err(|err| {
                                crate::Error::wrap(err, "failed to run filter function")
                            })? {
                                FlushFilterOutput::Drop => {}
                                FlushFilterOutput::Keep => log.append(content)?,
                                FlushFilterOutput::Replace(content) => log.append(content)?,
                            }
                        }
                    } else {
                        let log = new_logs[0].get_mut().unwrap();
                        // Copy entries to new Logs.
                        for entry in self.writable_log().iter_dirty() {
                            let bytes = entry?;
                            log.append(bytes)?;
                        }
                    }
                    self.set_logs(latest, new_logs);
                }

                let size = self.writable_log().flush()?;

                #[cfg(test)]
                if let Some(func) = self.hook_after_log_sync.as_ref() {
                    func();
                }

                let needs_rotation = if self.open_options.log_open_options.btrfs_compression {
                    self.btrfs_needs_rotation(size)?
                } else {
                    size >= self.open_options.max_bytes_per_log
                };

                if needs_rotation {
                    // `self.writable_log()` will be rotated (i.e., becomes immutable).
                    // Make sure indexes are up-to-date so reading it would not require
                    // building missing indexes in-memory.
                    self.writable_log().finalize_indexes(&lock)?;
                    self.rotate_internal(&lock)?;
                }
            }

            self.update_change_detector_to_match_meta();
            Ok(self.latest)
        })();

        result
            .context("in RotateLog::sync")
            .context(|| format!("  RotateLog.dir = {:?}", self.dir))
    }

    fn btrfs_needs_rotation(&mut self, apparent_size: u64) -> crate::Result<bool> {
        // Use predicted threshold based on btrfs compression ratio, if available.
        let threshold = self
            .next_btrfs_size_check
            .unwrap_or(self.open_options.max_bytes_per_log);

        if apparent_size < threshold {
            // Avoid checking btrfs size if we haven't reached the threshold yet.
            return Ok(false);
        }

        let btrfs_size = self.writable_log().btrfs_size()?;

        debug!(btrfs_size, apparent_size, "btrfs physical size");

        if btrfs_size >= self.open_options.max_bytes_per_log {
            // Physical log size has passed the threshold - time to rotate.
            return Ok(true);
        }

        if btrfs_size > 0 && apparent_size > 0 && !self.open_options.log_open_options.fsync {
            // If we aren't fsyncing, compute compression ratio and predict when we
            // are going to need rotation. This is to minimize btrfs size queries,
            // which are relatively slow because they fsync the file.
            let compression_ratio = (btrfs_size as f64) / (apparent_size as f64);
            if compression_ratio > 0f64 {
                let predicted_threshold =
                    (self.open_options.max_bytes_per_log as f64 / compression_ratio) as u64;

                // Cap our predicted threshold increase to self.open_options.max_bytes_per_log. For
                // example, if max_bytes_per_log=10MB, and after 10MB of writes the log is only
                // 100KB, we would "predict" a new threshold of 1GB based on the 1% compression
                // ratio. Instead, cap the threshold at 20MB, lest the high compression was just
                // temporary.
                let predicted_threshold =
                    predicted_threshold.min(apparent_size + self.open_options.max_bytes_per_log);

                debug!(
                    predicted_threshold,
                    compression_ratio, "setting predicted rotation threshold"
                );
                self.next_btrfs_size_check = Some(predicted_threshold);
            }
        }

        Ok(false)
    }

    fn update_change_detector_to_match_meta(&mut self) {
        let meta = &self.writable_log().meta;
        let value = meta.primary_len ^ meta.epoch ^ ((self.latest as u64) << 56);
        if let Some(detector) = &self.change_detector {
            detector.set(value);
        }
    }

    /// Attempt to remove outdated logs.
    ///
    /// Does nothing if the content of the 'latest' file has changed on disk,
    /// which indicates rotation was triggered elsewhere, or the [`RotateLog`]
    /// is in-memory.
    pub fn remove_old_logs(&mut self) -> crate::Result<()> {
        if let Some(dir) = &self.dir {
            let lock = ScopedDirLock::new(dir)?;
            let latest = read_latest(dir)?;
            if latest == self.latest {
                self.try_remove_old_logs(&lock);
            }
        }
        Ok(())
    }

    /// Returns `true` if `sync` will load more data on disk.
    ///
    /// This function is optimized to be called frequently. It does not access
    /// the filesystem directly, but communicate using a shared mmap buffer.
    ///
    /// This is not about testing buffered pending changes. To access buffered
    /// pending changes, use [`RotateLog::iter_dirty`] instead.
    pub fn is_changed_on_disk(&self) -> bool {
        match &self.change_detector {
            Some(detector) => detector.is_changed(),
            None => false,
        }
    }

    /// Force create a new [`Log`]. Bump latest.
    ///
    /// This function requires it's protected by a directory lock, and the
    /// callsite makes sure that [`Log`]s are consistent (ex. up-to-date,
    /// and do not have dirty entries in non-writable logs).
    fn rotate_internal(&mut self, lock: &ScopedDirLock) -> crate::Result<()> {
        ROTATE_COUNT.fetch_add(1, Ordering::Relaxed);

        // This is relative to the primary log, so clear it out when rotating.
        self.next_btrfs_size_check.take();

        let span = debug_span!("RotateLog::rotate", latest = self.latest as u32);
        if let Some(dir) = &self.dir {
            span.record("dir", dir.to_string_lossy().as_ref());
        }
        let _guard = span.enter();

        // Create a new Log. Bump latest.
        let next = self.latest.wrapping_add(1);
        let log = create_empty_log(
            Some(self.dir.as_ref().unwrap()),
            &self.open_options,
            next,
            lock,
        )?;
        if self.logs.len() >= self.open_options.max_log_count as usize {
            if let Some(log) = self.logs.pop().and_then(|mut l| l.take()) {
                if self.is_consistent_reads() {
                    self.pinned_logs.push(log);
                }
            }
        }

        self.logs.insert(0, create_log_cell(log));
        self.logs_len = AtomicUsize::new(self.logs.len());
        self.latest = next;
        self.try_remove_old_logs(lock);
        Ok(())
    }

    /// Renamed. Use [`RotateLog::sync`] instead.
    pub fn flush(&mut self) -> crate::Result<u8> {
        self.sync()
    }

    fn set_logs(&mut self, latest: u8, logs: Vec<OnceCell<Log>>) {
        // This is relative to the primary log, so clear it out when logs changed.
        self.next_btrfs_size_check.take();

        if self.is_consistent_reads() {
            // If we are in "consistent read" mode, store any Logs we would be rotating out into
            // self.pinned_logs. If "latest" has wrapped more than once, we may not pin the right
            // Logs.
            for mut to_pin in std::mem::take(&mut self.logs)
                .into_iter()
                .rev()
                .take(latest.wrapping_sub(self.latest) as usize)
            {
                match to_pin.take() {
                    Some(log) => self.pinned_logs.push(log),
                    None => break,
                }
            }
        }

        self.logs_len = AtomicUsize::new(logs.len());
        self.logs = logs;
        self.latest = latest;
    }

    #[allow(clippy::nonminimal_bool)]
    fn try_remove_old_logs(&self, _lock: &ScopedDirLock) {
        if let Ok(read_dir) = self.dir.as_ref().unwrap().read_dir() {
            let latest = self.latest;
            let earliest = latest.wrapping_sub(self.open_options.max_log_count - 1);
            for entry in read_dir {
                if let Ok(entry) = entry {
                    let name = entry.file_name();
                    debug!("Inspecting {:?} for rotate log removal", name);
                    if let Some(name) = name.to_str() {
                        if let Ok(id) = name.parse::<u8>() {
                            if (latest >= earliest && (id > latest || id < earliest))
                                || (latest < earliest && (id > latest && id < earliest))
                            {
                                // Explicitly delete the `meta` file first. This marks
                                // the log as "deleted" in an atomic way.
                                //
                                // Errors are not fatal. On Windows, this can fail if
                                // other processes have files in entry.path() mmap-ed.
                                // Newly opened or flushed RotateLog will unmap files.
                                // New rotation would trigger remove_dir_all to try
                                // remove old logs again.
                                match fs::remove_file(entry.path().join(log::META_FILE)) {
                                    Ok(()) => {}
                                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                                        // Meta file is already deleted.
                                    }
                                    Err(e) => {
                                        // Don't delete the log if we were unable to delete the
                                        // meta file.
                                        debug!(
                                            "Error removing rotate log meta: {:?} {:?}",
                                            name, e
                                        );
                                        continue;
                                    }
                                }

                                // Delete the rest of the directory.
                                let res = fs::remove_dir_all(entry.path());
                                match res {
                                    Ok(_) => debug!("Removed rotate log: {:?}", name),
                                    Err(err) => {
                                        debug!("Error removing rotate log directory: {:?}", err)
                                    }
                                };
                            } else {
                                debug!(
                                    "Not removing rotate log: {:?} (latest: {:?}, earliest: {:?})",
                                    name, latest, earliest
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    /// Get the writable [`Log`].
    fn writable_log(&mut self) -> &mut Log {
        self.logs[0].get_mut().unwrap()
    }

    /// Lazily load a log. The 'latest' (or 'writable') log has index 0.
    fn load_log(&self, index: usize) -> crate::Result<Option<&Log>> {
        if index >= self.logs_len.load(SeqCst) {
            return Ok(None);
        }
        match self.logs.get(index) {
            Some(cell) => {
                let id = self.latest.wrapping_sub(index as u8);
                if let Some(dir) = &self.dir {
                    let log = cell.get_or_try_init(|| {
                        let mut open_options = self.open_options.log_open_options.clone();
                        if index > 0 {
                            open_options = open_options.with_zero_index_lag();
                        }
                        let log = load_log(dir, id, open_options);
                        trace!(
                            name = "RotateLog::load_log",
                            index = index,
                            success = log.is_ok()
                        );
                        log
                    });
                    match log {
                        Ok(log) => Ok(Some(log)),
                        Err(err) => {
                            // Logically truncate self.logs. This avoids loading broken Logs again.
                            self.logs_len.store(index, SeqCst);
                            Err(err)
                        }
                    }
                } else {
                    Ok(cell.get())
                }
            }
            None => unreachable!(),
        }
    }

    /// Iterate over all the entries.
    ///
    /// The entries are returned in FIFO order.
    pub fn iter(&self) -> impl Iterator<Item = crate::Result<&[u8]>> {
        let logs = self.logs();
        logs.into_iter().rev().flat_map(|log| log.iter())
    }

    /// Iterate over all dirty entries.
    pub fn iter_dirty(&self) -> impl Iterator<Item = crate::Result<&[u8]>> {
        self.logs[0].get().unwrap().iter_dirty()
    }

    /// Guarantee read-after-write consistency for this RotateLog while guard is alive. Log rotation
    /// still happens as normal, but this RotateLog avoids dropping Log objects that have been
    /// rotated.
    ///
    /// BUG: This currently does not work properly if there are more than 255 rotations by another
    /// RotateLog instance before we reload from disk. The "latest" number is represented as a u8,
    /// so we can't tell the difference between 1 and 256 rotations.
    pub fn with_consistent_reads(&mut self) -> ConsistentReadGuard {
        self.consistent_reads.fetch_add(1, Ordering::AcqRel);
        ConsistentReadGuard {
            count: self.consistent_reads.clone(),
        }
    }

    /// Return whether we are in "consistent read" mode. This mode means we should not drop any
    /// Logs, lest we lose track of writes we made while consistent read mode was active.
    fn is_consistent_reads(&self) -> bool {
        self.consistent_reads.load(Ordering::Acquire) > 0
    }

    /// If we aren't in "consistent read" mode, clear out any Logs we may have pinned.
    fn maybe_clear_pinned_logs(&mut self) {
        if !self.is_consistent_reads() {
            self.pinned_logs.clear();
        }
    }
}

pub struct ConsistentReadGuard {
    count: Arc<AtomicU64>,
}

impl Drop for ConsistentReadGuard {
    fn drop(&mut self) {
        self.count.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Wrap `Log` in a `OnceCell`.
fn create_log_cell(log: Log) -> OnceCell<Log> {
    let cell = OnceCell::new();
    cell.set(log)
        .expect("cell is empty so cell.set cannot fail");
    cell
}

/// Load a single log at the given location.
fn load_log(dir: &Path, id: u8, open_options: log::OpenOptions) -> crate::Result<Log> {
    let name = format!("{}", id);
    let log_path = dir.join(name);
    open_options.create(false).open(log_path)
}

/// Get access to internals of [`RotateLog`].
///
/// This can be useful when there are low-level needs. For example:
/// - Get access to individual logs for things like range query.
/// - Rotate logs manually.
pub trait RotateLowLevelExt {
    /// Get a view of all individual logs. Newest first.
    fn logs(&self) -> Vec<&Log>;
}

impl RotateLowLevelExt for RotateLog {
    fn logs(&self) -> Vec<&Log> {
        (0..)
            .map(|i| self.load_log(i))
            .take_while(|res| match res {
                Ok(Some(_)) => true,
                _ => false,
            })
            .map(|res| res.unwrap().unwrap())
            .collect()
    }
}

/// Iterator over [`RotateLog`] entries selected by an index lookup.
pub struct RotateLogLookupIter<'a> {
    inner_iter: log::LogLookupIter<'a>,
    end: bool,
    log_rotate: &'a RotateLog,
    pinned_logs: &'a [Log],
    log_index: usize,
    index_id: usize,
    key: Bytes,
}

impl<'a> RotateLogLookupIter<'a> {
    fn load_next_log(&mut self) -> crate::Result<()> {
        // Iterate over self.log_rotate.logs (active logs) along with self.pinned_logs, which are
        // previously rotated out Logs that we have temporarily held onto to provide
        // read-after-write consistency.
        if self.log_index + 1 >= self.log_rotate.logs.len() + self.pinned_logs.len() {
            self.end = true;
            Ok(())
        } else {
            // Try the next log
            self.log_index += 1;

            let log = if self.log_index >= self.log_rotate.logs.len() {
                Ok(self
                    .pinned_logs
                    .get(self.log_index - self.log_rotate.logs.len()))
            } else {
                self.log_rotate.load_log(self.log_index)
            };

            match log {
                Ok(None) => {
                    self.end = true;
                    Ok(())
                }
                Err(_err) => {
                    self.end = true;
                    // Not fatal (since RotateLog is designed to be able
                    // to drop data).
                    Ok(())
                }
                Ok(Some(log)) => match log.lookup(self.index_id, &self.key) {
                    Err(err) => {
                        self.end = true;
                        Err(err)
                    }
                    Ok(iter) => {
                        self.inner_iter = iter;
                        Ok(())
                    }
                },
            }
        }
    }

    /// Consume iterator, returning whether the iterator has any data.
    pub fn is_empty(mut self) -> crate::Result<bool> {
        while !self.end {
            if !self.inner_iter.is_empty() {
                return Ok(false);
            }
            self.load_next_log()?;
        }
        Ok(true)
    }
}

impl<'a> Iterator for RotateLogLookupIter<'a> {
    type Item = crate::Result<&'a [u8]>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.end {
            return None;
        }
        match self.inner_iter.next() {
            None => {
                if let Err(err) = self.load_next_log() {
                    return Some(Err(err));
                }

                if self.end {
                    return None;
                }

                self.next()
            }
            Some(Err(err)) => {
                self.end = true;
                Some(Err(err))
            }
            Some(Ok(slice)) => Some(Ok(slice)),
        }
    }
}

fn create_empty_log(
    dir: Option<&Path>,
    open_options: &OpenOptions,
    latest: u8,
    _lock: &ScopedDirLock,
) -> crate::Result<Log> {
    Ok(match dir {
        Some(dir) => {
            let latest_path = dir.join(LATEST_FILE);
            let latest_str = format!("{}", latest);
            let log_path = dir.join(&latest_str);
            let opts = open_options.log_open_options.clone().create(true);
            opts.delete_content(&log_path)?;
            let log = opts.open(&log_path)?;
            utils::atomic_write(latest_path, latest_str.as_bytes(), false)?;
            log
        }
        None => open_options.log_open_options.clone().open(())?,
    })
}

fn read_latest(dir: &Path) -> crate::Result<u8> {
    read_latest_raw(dir).context(dir, "cannot read latest")
}

// Unlike read_latest, this function returns io::Result.
fn read_latest_raw(dir: &Path) -> io::Result<u8> {
    let latest_path = dir.join(LATEST_FILE);
    let data = utils::atomic_read(&latest_path)?;
    let content: String = String::from_utf8(data).map_err(|_e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{:?}: failed to read as utf8 string", latest_path),
        )
    })?;
    let id: u8 = content.parse().map_err(|_e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{:?}: failed to parse {:?} as u8 integer",
                latest_path, content
            ),
        )
    })?;
    Ok(id)
}

fn read_logs(
    dir: &Path,
    open_options: &OpenOptions,
    latest: u8,
) -> crate::Result<Vec<OnceCell<Log>>> {
    let mut logs = Vec::with_capacity(open_options.max_log_count as usize);

    // Make sure the first log (latest) can be loaded.
    let log = load_log(dir, latest, open_options.log_open_options.clone())?;
    logs.push(create_log_cell(log));

    // Lazily load the rest of logs.
    for index in 1..open_options.max_log_count {
        let id = latest.wrapping_sub(index);
        // Do a quick check about whether the log exists or not so we
        // can avoid unnecessary `Log::open`.
        let name = format!("{}", id);
        let log_path = dir.join(&name);
        if !log_path.is_dir() {
            break;
        }
        logs.push(OnceCell::new());
    }
    trace!(
        name = "RotateLog::read_logs",
        max_log_count = open_options.max_log_count,
        logs_len = logs.len()
    );

    Ok(logs)
}

fn read_latest_and_logs(
    dir: &Path,
    open_options: &OpenOptions,
) -> crate::Result<(u8, Vec<OnceCell<Log>>)> {
    let latest = read_latest(dir)?;
    Ok((latest, read_logs(dir, open_options, latest)?))
}

/// Given a list of ids, guess a `latest`.
fn guess_latest(mut ids: Vec<u8>) -> u8 {
    // Guess a sensible `latest` from `ids`.
    ids.sort_unstable();

    let mut id_to_ignore = 255;
    loop {
        match ids.pop() {
            Some(id) => {
                // Remove 255, 254, at the end, since they might have been wrapped.
                // For example, guess([0, 1, 2, 254, 255]) is 2.
                if id == id_to_ignore {
                    id_to_ignore -= 1;
                    if id_to_ignore == 0 {
                        // All 255 logs exist - rare.
                        break 0;
                    }
                    continue;
                } else {
                    // This is probably the desirable id.
                    // For example, guess([3, 4, 5]) is 5.
                    break id;
                }
            }
            None => {
                // For example, guess([]) is 0.
                break 0;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use log::IndexOutput;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_open() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rotate");

        assert!(OpenOptions::new().create(false).open(&path).is_err());
        assert!(OpenOptions::new().create(true).open(&path).is_ok());
        assert!(
            OpenOptions::new()
                .checksum_type(log::ChecksumType::Xxhash64)
                .create(false)
                .open(&path)
                .is_ok()
        );
    }

    // lookup via index 0
    fn lookup<'a>(rotate: &'a RotateLog, key: &[u8]) -> Vec<&'a [u8]> {
        let values = rotate
            .lookup(0, key.to_vec())
            .unwrap()
            .collect::<crate::Result<Vec<&[u8]>>>()
            .unwrap();
        for value in &values {
            let b1 = rotate.slice_to_bytes(value);
            let b2 = rotate.slice_to_bytes(value);
            // Dirty entries cannot be zero-copied.
            if rotate
                .iter_dirty()
                .any(|i| i.unwrap().as_ptr() == value.as_ptr())
            {
                continue;
            }
            assert_eq!(
                b1.as_ptr(),
                b2.as_ptr(),
                "slice_to_bytes should return zero-copy"
            );
        }
        values
    }

    fn iter(rotate: &RotateLog) -> Vec<&[u8]> {
        rotate
            .iter()
            .collect::<crate::Result<Vec<&[u8]>>>()
            .unwrap()
    }

    #[test]
    fn test_trivial_append_lookup() {
        let dir = tempdir().unwrap();
        let opts = OpenOptions::new()
            .create(true)
            .index_defs(vec![IndexDef::new("two-bytes", |_| {
                vec![IndexOutput::Reference(0..2)]
            })]);

        let rotate = opts.open(&dir).unwrap();
        let rotate_mem = opts.create_in_memory().unwrap();

        for rotate in &mut [rotate, rotate_mem] {
            rotate.append(b"aaa").unwrap();
            rotate.append(b"abbb").unwrap();
            rotate.append(b"abc").unwrap();

            assert_eq!(lookup(rotate, b"aa"), vec![b"aaa"]);
            assert_eq!(lookup(rotate, b"ab"), vec![&b"abc"[..], b"abbb"]);
            assert_eq!(lookup(rotate, b"ac"), Vec::<&[u8]>::new());
        }
    }

    #[test]
    fn test_simple_rotate() {
        let dir = tempdir().unwrap();
        let mut rotate = OpenOptions::new()
            .create(true)
            .max_bytes_per_log(100)
            .max_log_count(2)
            .index("first-byte", |_| vec![IndexOutput::Reference(0..1)])
            .open(&dir)
            .unwrap();

        // No rotate.
        rotate.append(b"a").unwrap();
        assert_eq!(rotate.sync().unwrap(), 0);
        rotate.append(b"a").unwrap();
        assert_eq!(rotate.sync().unwrap(), 0);

        // Trigger rotate. "a" is still accessible.
        rotate.append(vec![b'b'; 100]).unwrap();
        assert_eq!(rotate.sync().unwrap(), 1);
        assert_eq!(lookup(&rotate, b"a").len(), 2);

        // Trigger rotate again. Only new entries are accessible.
        // Older directories should be deleted automatically.
        rotate.append(vec![b'c'; 50]).unwrap();
        assert_eq!(rotate.sync().unwrap(), 1);
        rotate.append(vec![b'd'; 50]).unwrap();
        assert_eq!(rotate.sync().unwrap(), 2);
        assert_eq!(lookup(&rotate, b"a").len(), 0);
        assert_eq!(lookup(&rotate, b"b").len(), 0);
        assert_eq!(lookup(&rotate, b"c").len(), 1);
        assert_eq!(lookup(&rotate, b"d").len(), 1);
        assert!(!dir.path().join("0").exists());
    }

    #[test]
    fn test_manual_remove_old_logs() {
        let dir = tempdir().unwrap();
        let dir = &dir;
        let open = |n: u8| -> RotateLog {
            OpenOptions::new()
                .create(true)
                .max_bytes_per_log(1)
                .max_log_count(n)
                .open(dir)
                .unwrap()
        };
        let read_all =
            |log: &RotateLog| -> Vec<Vec<u8>> { log.iter().map(|v| v.unwrap().to_vec()).collect() };

        // Create 5 logs
        {
            let mut rotate = open(5);
            for i in 0..5 {
                rotate.append(vec![i]).unwrap();
                rotate.sync().unwrap();
            }
        }

        // Content depends on max_log_count.
        {
            let rotate = open(4);
            assert_eq!(read_all(&rotate), [[2], [3], [4]]);
            let rotate = open(3);
            assert_eq!(read_all(&rotate), [[3], [4]]);
        }

        // Remove old logs.
        {
            let mut rotate = open(3);
            rotate.remove_old_logs().unwrap();
        }

        // Verify that [2] is removed.
        {
            let rotate = open(4);
            assert_eq!(read_all(&rotate), [[3], [4]]);
        }
    }

    fn test_wrapping_rotate(max_log_count: u8) {
        let dir = tempdir().unwrap();
        let mut rotate = OpenOptions::new()
            .create(true)
            .max_bytes_per_log(10)
            .max_log_count(max_log_count)
            .open(&dir)
            .unwrap();

        let count = || {
            fs::read_dir(&dir)
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                // On Windows, the "lock" file was created by open_dir.
                .filter(|name| name != "lock" && name != "rlock")
                .count()
        };

        for i in 1..=(max_log_count - 1) {
            rotate.append(b"abcdefghijklmn").unwrap();
            assert_eq!(rotate.sync().unwrap(), i);
            assert_eq!(count(), (i as usize) + 2);
        }

        for i in max_log_count..=255 {
            rotate.append(b"abcdefghijklmn").unwrap();
            assert_eq!(rotate.sync().unwrap(), i);
            assert_eq!(count(), (max_log_count as usize) + 1);
        }

        for _ in 0..=max_log_count {
            rotate.append(b"abcdefghijklmn").unwrap();
            assert_eq!(count(), (max_log_count as usize) + 1);
        }
    }

    #[test]
    fn test_wrapping_rotate_10() {
        test_wrapping_rotate(10)
    }

    #[test]
    fn test_wrapping_rotate_255() {
        test_wrapping_rotate(255)
    }

    #[test]
    fn test_lookup_rotated() {
        // Look up or iteration should work with rotated logs.
        let dir = tempdir().unwrap();
        let open_opts = OpenOptions::new()
            .create(true)
            .max_bytes_per_log(1)
            .max_log_count(3)
            .index("first-byte", |_| vec![IndexOutput::Reference(0..1)]);

        // Prepare test data
        let mut rotate1 = open_opts.open(&dir).unwrap();
        rotate1.append(b"a1").unwrap();
        assert_eq!(rotate1.sync().unwrap(), 1);
        rotate1.append(b"a2").unwrap();
        assert_eq!(rotate1.sync().unwrap(), 2);

        // Warm up rotate1.
        assert_eq!(lookup(&rotate1, b"a"), vec![b"a2", b"a1"]);
        assert_eq!(iter(&rotate1), vec![b"a1", b"a2"]);

        // rotate2 has lazy logs
        let rotate2 = open_opts.open(&dir).unwrap();

        // Trigger rotate from another RotateLog
        let mut rotate3 = open_opts.open(&dir).unwrap();
        rotate3.append(b"a3").unwrap();
        assert_eq!(rotate3.sync().unwrap(), 3);

        // rotate1 can still use its existing indexes even if "a1"
        // might have been deleted (on Unix).
        assert_eq!(lookup(&rotate1, b"a"), vec![b"a2", b"a1"]);
        assert_eq!(iter(&rotate1), vec![b"a1", b"a2"]);

        // rotate2 does not have access to the deleted "a1".
        // (on Windows, 'meta' can be deleted, while mmap-ed files cannot)
        assert_eq!(lookup(&rotate2, b"a"), vec![b"a2"]);
        assert_eq!(iter(&rotate2), vec![b"a2"]);
    }

    #[test]
    fn test_is_empty() -> crate::Result<()> {
        let dir = tempdir().unwrap();
        let open_opts = OpenOptions::new()
            .create(true)
            .max_bytes_per_log(2)
            .max_log_count(4)
            .index("first-byte", |_| vec![IndexOutput::Reference(0..1)]);

        let mut rotate = open_opts.open(&dir)?;
        rotate.append(b"a1")?;
        assert_eq!(rotate.sync()?, 1);

        rotate.append(b"a2")?;
        assert_eq!(rotate.sync()?, 2);

        rotate.append(b"b1")?;
        assert_eq!(rotate.sync()?, 3);

        assert_eq!(lookup(&rotate, b"a"), vec![b"a2", b"a1"]);
        assert_eq!(lookup(&rotate, b"b"), vec![b"b1"]);

        assert!(!rotate.lookup(0, b"a".to_vec())?.is_empty()?);
        assert!(!rotate.lookup(0, b"b".to_vec())?.is_empty()?);
        assert!(rotate.lookup(0, b"c".to_vec())?.is_empty()?);

        Ok(())
    }

    #[test]
    fn test_lookup_truncated_meta() {
        // Look up or iteration should work with rotated logs.
        let dir = tempdir().unwrap();
        let open_opts = OpenOptions::new()
            .create(true)
            .max_bytes_per_log(1)
            .max_log_count(3)
            .index("first-byte", |_| vec![IndexOutput::Reference(0..1)]);

        // Prepare test data
        let mut rotate1 = open_opts.open(&dir).unwrap();
        rotate1.append(b"a1").unwrap();
        assert_eq!(rotate1.sync().unwrap(), 1);
        rotate1.append(b"a2").unwrap();
        assert_eq!(rotate1.sync().unwrap(), 2);

        // Warm up rotate1
        assert_eq!(lookup(&rotate1, b"a"), vec![b"a2", b"a1"]);
        assert_eq!(iter(&rotate1), vec![b"a1", b"a2"]);

        // rotate2 has lazy logs
        let rotate2 = open_opts.open(&dir).unwrap();

        // Break logs by truncating "meta".
        utils::atomic_write(dir.path().join("0").join(log::META_FILE), "", false).unwrap();

        // rotate1 can still use its existing indexes even if "a1"
        // might have been deleted (on Unix).
        assert_eq!(lookup(&rotate1, b"a"), vec![b"a2", b"a1"]);
        assert_eq!(iter(&rotate1), vec![b"a1", b"a2"]);

        // rotate2 does not have access to the deleted "a1".
        assert_eq!(lookup(&rotate2, b"a"), vec![b"a2"]);
        assert_eq!(iter(&rotate2), vec![b"a2"]);
    }

    #[test]
    fn test_concurrent_writes() {
        let dir = tempdir().unwrap();
        let mut rotate1 = OpenOptions::new()
            .create(true)
            .max_bytes_per_log(100)
            .max_log_count(2)
            .open(&dir)
            .unwrap();
        let mut rotate2 = OpenOptions::new()
            .max_bytes_per_log(100)
            .max_log_count(2)
            .open(&dir)
            .unwrap();

        // rotate1 triggers a rotation.
        rotate1.append(vec![b'a'; 100]).unwrap();
        assert_eq!(rotate1.sync().unwrap(), 1);

        let size = |log_index: u64| {
            dir.path()
                .join(format!("{}", log_index))
                .join(log::PRIMARY_FILE)
                .metadata()
                .unwrap()
                .len()
        };

        let size1 = size(1);

        // rotate2 writes to the right location ("1"), not "0";
        rotate2.append(vec![b'b'; 100]).unwrap();
        assert_eq!(rotate2.sync().unwrap(), 2);

        #[cfg(unix)]
        {
            assert!(!dir.path().join("0").exists());
        }
        assert!(size(1) > size1 + 100);
        assert!(size(2) > 0);
    }

    #[test]
    fn test_flush_filter() {
        let dir = tempdir().unwrap();

        let read_log = |name: &str| -> Vec<Vec<u8>> {
            let log = Log::open(dir.path().join(name), Vec::new()).unwrap();
            log.iter().map(|v| v.unwrap().to_vec()).collect()
        };

        let mut rotate1 = OpenOptions::new()
            .create(true)
            .max_bytes_per_log(100)
            .flush_filter(Some(|ctx, bytes| {
                // 'aa' is not inserted yet. It should not exist in the log.
                assert!(!ctx.log.iter().any(|x| x.unwrap() == b"aa"));
                Ok(match bytes.len() {
                    1 => FlushFilterOutput::Replace(b"xx".to_vec()),
                    _ => FlushFilterOutput::Keep,
                })
            }))
            .open(&dir)
            .unwrap();

        let mut rotate2 = OpenOptions::new()
            .max_bytes_per_log(100)
            .open(&dir)
            .unwrap();

        rotate2.append(vec![b'a'; 3]).unwrap();
        rotate2.sync().unwrap();

        rotate1.append(vec![b'a'; 1]).unwrap(); // replaced to 'xx'
        rotate1.append(vec![b'a'; 2]).unwrap();
        assert_eq!(rotate1.sync().unwrap(), 0); // trigger flush filter by Log
        assert_eq!(read_log("0"), vec![&b"aaa"[..], b"xx", b"aa"]);

        rotate1.append(vec![b'a'; 1]).unwrap(); // not replaced
        assert_eq!(rotate1.sync().unwrap(), 0); // do not trigger flush filter
        assert_eq!(read_log("0").last().unwrap(), b"a");

        rotate1.append(vec![b'a'; 1]).unwrap(); // replaced to 'xx'
        rotate1.append(vec![b'a'; 2]).unwrap();

        rotate2.append(vec![b'a'; 100]).unwrap(); // rotate
        assert_eq!(rotate2.sync().unwrap(), 1);

        assert_eq!(rotate1.sync().unwrap(), 1); // trigger flush filter by RotateLog
        assert_eq!(read_log("1"), vec![b"xx", b"aa"]);
    }

    #[test]
    fn test_is_changed_on_disk() {
        let dir = tempdir().unwrap();
        let open_opts = OpenOptions::new()
            .create(true)
            .max_bytes_per_log(5000)
            .max_log_count(2);

        // Repeat a few times to trigger rotation.
        for _ in 0..10 {
            let mut rotate1 = open_opts.open(&dir).unwrap();
            let mut rotate2 = open_opts.open(&dir).unwrap();

            assert!(!rotate1.is_changed_on_disk());
            assert!(!rotate2.is_changed_on_disk());

            // no-op sync() does not set is_changed().
            rotate1.sync().unwrap();
            assert!(!rotate2.is_changed_on_disk());

            // change before flush does not set is_changed().
            rotate1.append([b'a'; 1000]).unwrap();

            assert!(!rotate1.is_changed_on_disk());
            assert!(!rotate2.is_changed_on_disk());

            // sync() does not set is_changed().
            rotate1.sync().unwrap();
            assert!(!rotate1.is_changed_on_disk());

            // rotate2 should be able to detect the on-disk change from rotate1.
            assert!(rotate2.is_changed_on_disk());

            // is_changed() does not clear is_changed().
            assert!(rotate2.is_changed_on_disk());

            // read-only sync() should clear is_changed().
            rotate2.sync().unwrap();
            assert!(!rotate2.is_changed_on_disk());
            // ... and not set other Logs' is_changed().
            assert!(!rotate1.is_changed_on_disk());

            rotate2.append([b'a'; 1000]).unwrap();
            rotate2.sync().unwrap();

            // rotate1 should be able to detect the on-disk change from rotate2.
            assert!(rotate1.is_changed_on_disk());

            // read-write sync() should clear is_changed().
            rotate1.append([b'a'; 1000]).unwrap();
            rotate1.sync().unwrap();
            assert!(!rotate1.is_changed_on_disk());
        }
    }

    #[test]
    fn test_lookup_latest() {
        let dir = tempdir().unwrap();
        let mut rotate = OpenOptions::new()
            .create(true)
            .max_bytes_per_log(100)
            .flush_filter(Some(|_, _| panic!()))
            .index("first-byte", |_| vec![IndexOutput::Reference(0..1)])
            .open(&dir)
            .unwrap();

        rotate.append(vec![b'a'; 101]).unwrap();
        rotate.sync().unwrap(); // trigger rotate
        rotate.append(vec![b'b'; 10]).unwrap();

        assert_eq!(rotate.lookup_latest(0, b"b").unwrap().count(), 1);
        assert_eq!(rotate.lookup_latest(0, b"a").unwrap().count(), 0);

        rotate.append(vec![b'c'; 101]).unwrap();
        rotate.sync().unwrap(); // trigger rotate again

        rotate.append(vec![b'd'; 10]).unwrap();
        rotate.sync().unwrap(); // not trigger rotate
        rotate.append(vec![b'e'; 10]).unwrap();

        assert_eq!(rotate.lookup_latest(0, b"c").unwrap().count(), 0);
        assert_eq!(rotate.lookup_latest(0, b"d").unwrap().count(), 1);
        assert_eq!(rotate.lookup_latest(0, b"e").unwrap().count(), 1);
    }

    #[test]
    #[should_panic]
    fn test_lookup_latest_panic() {
        let dir = tempdir().unwrap();
        let rotate = OpenOptions::new()
            .create(true)
            .index("first-byte", |_| vec![IndexOutput::Reference(0..1)])
            .open(&dir)
            .unwrap();
        rotate.lookup_latest(0, b"a").unwrap(); // flush_filter is not set
    }

    #[test]
    fn test_iter() {
        let dir = tempdir().unwrap();
        let mut rotate = OpenOptions::new()
            .create(true)
            .max_bytes_per_log(100)
            .open(&dir)
            .unwrap();

        let a = vec![b'a'; 101];
        let b = vec![b'b'; 10];

        rotate.append(a.clone()).unwrap();
        assert_eq!(
            rotate.iter_dirty().collect::<Result<Vec<_>, _>>().unwrap(),
            vec![&a[..]]
        );

        rotate.sync().unwrap(); // trigger rotate
        rotate.append(b.clone()).unwrap();
        rotate.append(a.clone()).unwrap();
        rotate.append(a.clone()).unwrap();
        assert_eq!(
            rotate.iter_dirty().collect::<Result<Vec<_>, _>>().unwrap(),
            vec![&b[..], &a, &a]
        );

        assert_eq!(
            rotate.iter().map(|e| e.unwrap()).collect::<Vec<&[u8]>>(),
            vec![&a[..], &b, &a, &a],
        );

        rotate.sync().unwrap(); // trigger rotate
        assert_eq!(
            rotate.iter().map(|e| e.unwrap()).collect::<Vec<&[u8]>>(),
            vec![&b[..], &a, &a],
        );
    }

    #[test]
    fn test_recover_from_empty_logs() {
        let dir = tempdir().unwrap();
        let rotate = OpenOptions::new().create(true).open(&dir).unwrap();
        drop(rotate);

        // Delete all logs, but keep "latest".
        for dirent in fs::read_dir(&dir).unwrap() {
            let dirent = dirent.unwrap();
            let path = dirent.path();
            if path.is_dir() {
                fs::remove_dir_all(path).unwrap();
            }
        }

        let _ = OpenOptions::new().create(true).open(&dir).unwrap();
    }

    #[test]
    fn test_recover_from_occupied_logs() {
        let dir = tempdir().unwrap();

        // Take the "1" spot.
        // Note: Cannot use "2" - it will be deleted by rotate -> try_remove_old_logs.
        {
            let mut log = log::OpenOptions::new()
                .create(true)
                .open(dir.path().join("1"))
                .unwrap();
            log.append(&[b'b'; 100][..]).unwrap();
            log.append(&[b'c'; 100][..]).unwrap();
            log.sync().unwrap();
        }

        // Rotate to "1" and "2".
        let mut rotate = OpenOptions::new()
            .create(true)
            .max_bytes_per_log(100)
            .max_log_count(3)
            .open(&dir)
            .unwrap();
        for i in [1, 2] {
            rotate.append(vec![b'a'; 101]).unwrap();
            assert_eq!(rotate.sync().unwrap(), i); // trigger rotate
        }

        // Content in the old "1" log should not appear here.
        assert_eq!(
            rotate.iter().map(|b| b.unwrap()[0]).collect::<Vec<_>>(),
            vec![b'a'; 2]
        );
    }

    #[test]
    fn test_index_lag() {
        let dir = tempdir().unwrap();
        let opts = OpenOptions::new()
            .create(true)
            .index_defs(vec![
                IndexDef::new("idx", |_| vec![IndexOutput::Reference(0..2)])
                    .lag_threshold(u64::MAX),
            ])
            .max_bytes_per_log(100)
            .max_log_count(3);

        let size = |name: &str| dir.path().join(name).metadata().unwrap().len();

        let mut rotate = opts.open(&dir).unwrap();
        rotate.append(vec![b'x'; 200]).unwrap();
        rotate.sync().unwrap();
        rotate.append(vec![b'y'; 200]).unwrap();
        rotate.sync().unwrap();
        rotate.append(vec![b'z'; 10]).unwrap();
        rotate.sync().unwrap();

        // First 2 logs become immutable, indexes are written regardless of
        // lag_threshold.
        assert!(size("0/index2-idx") > 0);
        assert!(size("0/log") > 100);

        assert!(size("1/index2-idx") > 0);
        assert!(size("1/log") > 100);

        // The "current" log is still mutable. Its index respects lag_threshold,
        // and is logically empty (because side effect of delete_content, the
        // index has some bytes in it).
        assert_eq!(size("2/index2-idx"), 25);
        assert!(size("2/log") < 100);
    }

    #[test]
    fn test_sync_missing_latest() {
        let dir = tempdir().unwrap();
        let opts = OpenOptions::new()
            .max_bytes_per_log(10000)
            .max_log_count(10);
        let mut rotate = opts.clone().create(true).open(&dir).unwrap();
        rotate.append(vec![b'x'; 200]).unwrap();
        rotate.sync().unwrap();

        let mut rotate2 = opts.open(&dir).unwrap();
        fs::remove_file(dir.path().join(LATEST_FILE)).unwrap();
        rotate2.sync().unwrap(); // not a failure
        rotate2.append(vec![b'y'; 200]).unwrap();
        rotate2.sync().unwrap_err(); // a failure
    }

    #[test]
    fn test_auto_sync_threshold() {
        let dir = tempdir().unwrap();
        let opts = OpenOptions::new().auto_sync_threshold(100).create(true);

        let mut rotate = opts.create(true).open(&dir).unwrap();
        rotate.append(vec![b'x'; 50]).unwrap();
        assert_eq!(rotate.logs()[0].iter_dirty().count(), 1);
        rotate.append(vec![b'x'; 50]).unwrap(); // trigger sync
        assert_eq!(rotate.logs()[0].iter_dirty().count(), 0);
    }

    #[test]
    fn test_auto_sync_threshold_with_racy_index_update_on_open() {
        fn index_defs(lag_threshold: u64) -> Vec<IndexDef> {
            let index_names = ["a"];
            (0..index_names.len())
                .map(|i| {
                    IndexDef::new(index_names[i], |_| vec![IndexOutput::Reference(0..1)])
                        .lag_threshold(lag_threshold)
                })
                .collect()
        }

        fn open_opts(lag_threshold: u64) -> OpenOptions {
            let index_defs = index_defs(lag_threshold);
            OpenOptions::new()
                .auto_sync_threshold(1000)
                .max_bytes_per_log(400)
                .max_log_count(10)
                .create(true)
                .index_defs(index_defs)
        }

        let dir = tempdir().unwrap();
        let path = dir.path();
        let data: &[u8] = &[b'x'; 100];
        let n = 10;
        for _i in 0..n {
            let mut rotate1 = open_opts(300).open(path).unwrap();
            rotate1.hook_after_log_sync = Some({
                let path = path.to_path_buf();
                Box::new(move || {
                    // This might updating indexes (see D20042046 and D20286509).
                    let rotate2 = open_opts(100).open(&path).unwrap();
                    // Force loading "lazy" indexes.
                    let _all = rotate2.iter().collect::<Result<Vec<_>, _>>().unwrap();
                })
            });
            rotate1.append(data).unwrap();
            rotate1.sync().unwrap();
        }

        // Verify that data can be read through index.
        let rotate1 = open_opts(300).open(path).unwrap();
        let mut count = 0;
        for entry in rotate1.lookup(0, b"x" as &[u8]).unwrap() {
            let entry = entry.unwrap();
            assert_eq!(entry, data);
            count += 1;
        }
        assert_eq!(count, n);
    }

    #[test]
    fn test_reindex_old_logs() {
        let dir = tempdir().unwrap();
        let opts = OpenOptions::new()
            .max_bytes_per_log(10)
            .max_log_count(10)
            .create(true);

        let mut rotate = opts.clone().create(true).open(&dir).unwrap();
        for i in 0..2u8 {
            rotate.append(vec![i; 50]).unwrap();
            rotate.sync().unwrap(); // rotate
        }

        // New OpenOptions: With indexes.
        let opts = opts.index("a", |_data| vec![IndexOutput::Reference(0..1)]);

        // Triggers rebuilding indexes.
        let rotate = opts.create(true).open(&dir).unwrap();

        // Because older log is lazy. It hasn't been loaded yet. So it does not have the index.
        assert!(!dir.path().join("1/index2-a").exists());
        assert!(!dir.path().join("0/index2-a").exists());

        // Do an index lookup. This will trigger loading old logs.
        let mut iter = rotate.lookup(0, b"\x00".to_vec()).unwrap();

        // The iterator is lazy. So it does not build the index immediately.
        assert!(!dir.path().join("1/index2-a").exists());

        // Iterate through all logs.
        assert_eq!(iter.next().unwrap().unwrap(), &[0; 50][..]);

        // Now the index is built for older logs.
        assert!(dir.path().join("1/index2-a").exists());
        assert!(dir.path().join("0/index2-a").exists());
    }

    #[test]
    fn test_repair_latest() {
        assert_eq!(guess_latest(vec![]), 0);
        assert_eq!(guess_latest(vec![3, 4, 5]), 5);
        assert_eq!(guess_latest(vec![0, 1, 2, 254, 255]), 2);
        assert_eq!(guess_latest((0..=255).collect::<Vec<_>>()), 0);

        let dir = tempdir().unwrap();
        let opts = OpenOptions::new().max_bytes_per_log(100).max_log_count(10);
        let mut rotate = opts.clone().create(true).open(&dir).unwrap();
        for i in 1..=2 {
            rotate.append(vec![b'x'; 200]).unwrap();
            assert_eq!(rotate.sync().unwrap(), i);
        }

        // Corrupt "latest".
        let latest_path = dir.path().join(LATEST_FILE);
        utils::atomic_write(latest_path, "NaN", false).unwrap();
        assert!(opts.open(&dir).is_err());
        assert_eq!(
            opts.repair(&dir)
                .unwrap()
                .lines()
                .filter(|l| !l.contains("Processing"))
                .collect::<Vec<_>>()
                .join("\n"),
            r#"Attempt to repair log "0"
Verified 1 entries, 223 bytes in log
Attempt to repair log "1"
Verified 1 entries, 223 bytes in log
Attempt to repair log "2"
Verified 0 entries, 12 bytes in log
Reset latest to 2"#
        );
        opts.open(&dir).unwrap();

        // Delete "latest".
        fs::remove_file(dir.path().join(LATEST_FILE)).unwrap();
        assert!(opts.open(&dir).is_err());

        // Repair can fix it.
        assert_eq!(
            opts.repair(&dir)
                .unwrap()
                .lines()
                .filter(|l| !l.contains("Processing"))
                .collect::<Vec<_>>()
                .join("\n"),
            r#"Attempt to repair log "0"
Verified 1 entries, 223 bytes in log
Attempt to repair log "1"
Verified 1 entries, 223 bytes in log
Attempt to repair log "2"
Verified 0 entries, 12 bytes in log
Reset latest to 2"#
        );
        opts.open(&dir).unwrap();
    }

    #[test]
    fn test_load_broken_logs_once() {
        let dir = tempdir().unwrap();
        let open_opts = OpenOptions::new()
            .create(true)
            .max_log_count(10)
            .max_bytes_per_log(100);
        let mut log = open_opts.open(dir.path()).unwrap();

        // Create 0, 1, 2, 3 logs
        for i in 0..4 {
            log.append(&[i; 200][..]).unwrap();
            log.sync().unwrap();
        }

        // Break 1/
        utils::atomic_write(dir.path().join("1").join("meta"), "foo", false).unwrap();
        let log = open_opts.open(dir.path()).unwrap();

        // The broken log should only be loaded once.
        assert!(log.load_log(3).is_err()); // Reports error loading the broken Log.
        assert!(log.load_log(3).is_ok()); // The error is "cached" - not loading the Log again.

        // Logs iteration will only have 2, no 0 or 1.
        assert_eq!(
            log.iter().map(|i| i.unwrap()[0]).collect::<Vec<_>>(),
            [2, 3]
        );
    }

    #[test]
    fn test_multithread_sync() {
        let dir = tempdir().unwrap();

        // Release mode runs much faster.
        const THREAD_COUNT: u8 = if cfg!(debug_assertions) { 10 } else { 30 };
        const WRITE_COUNT_PER_THREAD: u8 = if cfg!(debug_assertions) { 10 } else { 50 };

        // Some indexes. They have different lag_threshold.
        fn index_ref(data: &[u8]) -> Vec<IndexOutput> {
            vec![IndexOutput::Reference(0..data.len() as u64)]
        }
        fn index_copy(data: &[u8]) -> Vec<IndexOutput> {
            vec![IndexOutput::Owned(data.to_vec().into_boxed_slice())]
        }
        let indexes = vec![
            IndexDef::new("key1", index_ref).lag_threshold(1),
            IndexDef::new("key2", index_ref).lag_threshold(50),
            IndexDef::new("key3", index_ref).lag_threshold(1000),
            IndexDef::new("key4", index_copy).lag_threshold(1),
            IndexDef::new("key5", index_copy).lag_threshold(50),
            IndexDef::new("key6", index_copy).lag_threshold(1000),
        ];
        let index_len = indexes.len();
        let open_opts = OpenOptions::new()
            .create(true)
            .max_log_count(200)
            .max_bytes_per_log(200)
            .index_defs(indexes);

        use std::sync::Arc;
        use std::sync::Barrier;
        let barrier = Arc::new(Barrier::new(THREAD_COUNT as usize));
        let threads: Vec<_> = (0..THREAD_COUNT)
            .map(|i| {
                let barrier = barrier.clone();
                let open_opts = open_opts.clone();
                let path = dir.path().to_path_buf();
                std::thread::spawn(move || {
                    barrier.wait();
                    let mut log = open_opts.open(path).unwrap();
                    for j in 1..=WRITE_COUNT_PER_THREAD {
                        let buf = [i, j];
                        log.append(buf).unwrap();
                        if j % (i + 1) == 0 || j == WRITE_COUNT_PER_THREAD {
                            log.sync().unwrap();
                            // Verify that the indexes match the entries.
                            for entry in log.iter().map(|d| d.unwrap()) {
                                for index_id in 0..index_len {
                                    for index_value in log.lookup(index_id, entry.to_vec()).unwrap()
                                    {
                                        assert_eq!(index_value.unwrap(), entry);
                                    }
                                }
                            }
                        }
                    }
                })
            })
            .collect();

        // Wait for them.
        for thread in threads {
            thread.join().expect("joined");
        }

        // Check how many entries were written.
        let log = open_opts.open(dir.path()).unwrap();
        let count = log.iter().count() as u64;
        assert_eq!(count, THREAD_COUNT as u64 * WRITE_COUNT_PER_THREAD as u64);
    }

    #[test]
    fn test_btrfs_rotate() {
        let dir = tempdir().unwrap();

        if fsinfo::fstype(dir.path()).unwrap() != fsinfo::FsType::BTRFS {
            return;
        }

        #[cfg(target_os = "linux")]
        {
            use rand::RngCore;

            let mut rotate = OpenOptions::new()
                .create(true)
                .max_bytes_per_log(50)
                .max_log_count(2)
                .index("first-byte", |_| vec![IndexOutput::Reference(0..1)])
                .btrfs_compression(true)
                .open(&dir)
                .unwrap();

            // No rotation - log is compressed well.
            let aaa = vec![b'a'; 100];
            rotate.append(&aaa).unwrap();
            assert_eq!(rotate.sync().unwrap(), 0);
            assert_eq!(lookup(&rotate, b"a"), vec![&aaa]);

            // Sanity check we compressed well.
            let btrfs_size = rotate.writable_log().btrfs_size().unwrap();
            assert!(btrfs_size > 0);
            assert!(btrfs_size < 50);

            // Rotate triggered.
            let mut random_bytes = vec![0u8; 100];
            rand::thread_rng().fill_bytes(&mut random_bytes);
            random_bytes[0] = b'b';
            rotate.append(&random_bytes).unwrap();
            assert_eq!(rotate.sync().unwrap(), 1);
            assert_eq!(lookup(&rotate, b"a"), vec![&aaa]);
            assert_eq!(lookup(&rotate, b"b"), vec![&random_bytes]);

            let rotated_btrfs_size = rotate.logs[1].get_mut().unwrap().btrfs_size().unwrap();
            assert!(rotated_btrfs_size >= 50);
        }
    }

    #[test]
    fn test_consistent_reads() {
        // Set threshold low so we get a lot of rotation
        let open_opts = OpenOptions::new()
            .create(true)
            .max_log_count(2)
            .max_bytes_per_log(1)
            .auto_sync_threshold(0)
            .index_defs(vec![IndexDef::new("key", |data| {
                vec![IndexOutput::Reference(0..data.len() as u64)]
            })]);

        let get = |log: &RotateLog, key: &[u8]| -> Option<Vec<u8>> {
            Some(
                log.lookup(0, key.to_vec())
                    .unwrap()
                    .next()?
                    .unwrap()
                    .to_vec(),
            )
        };

        // Sanity check how rotation is working.
        {
            let dir = tempdir().unwrap();

            let mut log = open_opts.open(dir.path()).unwrap();

            // Log 0 is rotated immediately to log 1.
            log.append(b"a").unwrap();
            assert_eq!(get(&log, b"a"), Some(vec![b'a']));

            // Log 1 is dropped - log 0 rotated to log 1.
            log.append(b"b").unwrap();
            assert_eq!(get(&log, b"b"), Some(vec![b'b']));
            assert_eq!(get(&log, b"a"), None);
        }

        // Now again with consistent reads enabled.
        {
            let dir = tempdir().unwrap();

            let mut log = open_opts.open(dir.path()).unwrap();

            let _guard = log.with_consistent_reads();

            log.append(b"a").unwrap();
            assert_eq!(get(&log, b"a"), Some(vec![b'a']));

            // All entries still readable.
            log.append(b"b").unwrap();
            log.append(b"c").unwrap();
            assert_eq!(get(&log, b"c"), Some(vec![b'c']));
            assert_eq!(get(&log, b"b"), Some(vec![b'b']));
            assert_eq!(get(&log, b"a"), Some(vec![b'a']));

            // Drop consistent guard - rotated data is gone.
            drop(_guard);
            assert_eq!(get(&log, b"a"), None);
        }

        // Now with consistent reads and another writer to the log.
        {
            let dir = tempdir().unwrap();

            let mut log1 = open_opts.open(dir.path()).unwrap();
            let mut log2 = open_opts.open(dir.path()).unwrap();

            let _guard = log1.with_consistent_reads();

            log1.append(b"a").unwrap();

            log2.append(b"z").unwrap();
            log2.append(b"z").unwrap();
            log2.append(b"z").unwrap();

            log1.append(b"b").unwrap();

            log2.append(b"z").unwrap();
            log2.append(b"z").unwrap();
            log2.append(b"z").unwrap();

            log1.append(b"c").unwrap();

            log2.append(b"z").unwrap();
            log2.append(b"z").unwrap();
            log2.append(b"z").unwrap();

            assert_eq!(get(&log1, b"c"), Some(vec![b'c']));
            assert_eq!(get(&log1, b"b"), Some(vec![b'b']));
            assert_eq!(get(&log1, b"a"), Some(vec![b'a']));
            // Can't see "z" - it was rotated out when we appended "c".
            assert_eq!(get(&log1, b"z"), None);

            // Not buffering anything.
            assert!(log1.writable_log().mem_buf.is_empty());

            log1.sync().unwrap();
            assert_eq!(get(&log1, b"c"), Some(vec![b'c']));
            assert_eq!(get(&log1, b"b"), Some(vec![b'b']));
            assert_eq!(get(&log1, b"a"), Some(vec![b'a']));
            // Now we can see "z" since we reloaded logs from disk.
            assert_eq!(get(&log1, b"z"), Some(vec![b'z']));

            // Drop consistent guard - rotated data is gone.
            drop(_guard);
            assert_eq!(get(&log1, b"a"), None);
            assert_eq!(get(&log1, b"z"), Some(vec![b'z']));

            // Pinned logs haven't been cleaned up yet.
            assert!(!log1.pinned_logs.is_empty());

            // But they get cleaned at the next chance.
            log1.sync().unwrap();
            assert!(log1.pinned_logs.is_empty());
        }
    }
}
