// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Rotation support for a set of [`Log`]s.

use crate::errors::{IoResultExt, ResultExt};
use crate::lock::ScopedDirLock;
use crate::log::{self, FlushFilterContext, FlushFilterFunc, FlushFilterOutput, IndexDef, Log};
use crate::utils;
use bytes::Bytes;
use once_cell::sync::OnceCell;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A collection of [`Log`]s that get rotated or deleted automatically when they
/// exceed size or count limits.
///
/// Writes go to the active [`Log`]. Reads scan through all [`Log`]s.
pub struct RotateLog {
    dir: Option<PathBuf>,
    open_options: OpenOptions,
    logs: Vec<OnceCell<Log>>,
    latest: u8,
}

// On disk, a RotateLog is a directory containing:
// - 0/, 1/, 2/, 3/, ...: one Log per directory.
// - latest: a file, the name of the directory that is considered "active".

const LATEST_FILE: &str = "latest";

/// Options used to configure how a [`RotateLog`] is opened.
#[derive(Clone)]
pub struct OpenOptions {
    max_bytes_per_log: u64,
    max_log_count: u8,
    log_open_options: log::OpenOptions,
}

impl OpenOptions {
    /// Creates a default new set of options ready for configuration.
    ///
    /// The default values are:
    /// - Keep 2 logs.
    /// - A log gets rotated when it exceeds 2GB.
    /// - No indexes.
    /// - Do not create on demand.
    pub fn new() -> Self {
        // Some "seemingly reasonable" default values. Not scientifically chosen.
        let max_log_count = 2;
        let max_bytes_per_log = 2_000_000_000; // 2 GB
        Self {
            max_bytes_per_log,
            max_log_count,
            log_open_options: log::OpenOptions::new(),
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

    /// Open [`RotateLog`] at given location.
    pub fn open(&self, dir: impl AsRef<Path>) -> crate::Result<RotateLog> {
        let dir = dir.as_ref();
        let result: crate::Result<_> = (|| {
            let latest_and_log = read_latest_and_logs(dir, &self);

            let (latest, logs) = match latest_and_log {
                Ok((latest, logs)) => (latest, logs),
                Err(e) => {
                    if !self.log_open_options.create {
                        return Err(e)
                            .context("not creating new logs since OpenOption::create is not set");
                    } else {
                        utils::mkdir_p(dir)?;
                        let lock = ScopedDirLock::new(&dir)?;

                        match read_latest_raw(dir) {
                            Ok(latest) => {
                                match read_logs(dir, &self, latest) {
                                    Ok(logs) => {
                                        // Both latest and logs are read properly.
                                        (latest, logs)
                                    }
                                    Err(err) => {
                                        // latest is fine, but logs cannot be read.
                                        // Try auto recover by creating an empty log.
                                        let latest = latest.wrapping_add(1);
                                        match create_empty_log(Some(dir), &self, latest, &lock) {
                                            Ok(new_log) => {
                                                if let Ok(logs) = read_logs(dir, &self, latest) {
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
                                    let new_log =
                                        create_empty_log(Some(dir), &self, latest, &lock)?;
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

            Ok(RotateLog {
                dir: Some(dir.into()),
                open_options: self.clone(),
                logs,
                latest,
            })
        })();

        result
            .context(|| format!("in rotate::OpenOptions::open({:?})", dir))
            .context(|| format!("  OpenOptions = {:?}", self))
    }

    /// Open an-empty [`RotateLog`] in memory. The [`RotateLog`] cannot [`sync`].
    pub fn create_in_memory(&self) -> crate::Result<RotateLog> {
        let result: crate::Result<_> = (|| {
            let cell = create_log_cell(self.log_open_options.create_in_memory()?);
            let mut logs = Vec::with_capacity(1);
            logs.push(cell);
            Ok(RotateLog {
                dir: None,
                open_options: self.clone(),
                logs,
                latest: 0,
            })
        })();
        result
            .context("in rotate::OpenOptions::create_in_memory")
            .context(|| format!("  OpenOptions = {:?}", self))
    }

    /// Try repair all logs in the specified directory.
    ///
    /// This just calls into [`log::OpenOptions::repair`] recursively.
    pub fn repair(&self, dir: impl AsRef<Path>) -> crate::Result<String> {
        let dir = dir.as_ref();
        (|| -> crate::Result<_> {
            let _lock = ScopedDirLock::new(&dir)?;

            let mut message = String::new();
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
                match self.log_open_options.repair(&dir.join(name)) {
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
                        fs::write(&latest_path, content).context(&latest_path, "cannot write")?;
                        message += &format!("Reset latest to {}\n", latest);
                    }
                    _ => return Err(err).context(&latest_path, "cannot read or parse"),
                },
            };

            Ok(message)
        })()
        .context(|| format!("in rotate::OpenOptions::repair({:?})", dir))
    }
}

impl fmt::Debug for OpenOptions {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "OpenOptions {{ ")?;
        write!(f, "max_bytes_per_log: {}, ", self.max_bytes_per_log,)?;
        write!(f, "max_log_count: {}, ", self.max_log_count)?;
        write!(f, "log_open_options: {:?} }}", &self.log_open_options)?;
        Ok(())
    }
}

impl RotateLog {
    /// Append data to the writable [`Log`].
    pub fn append(&mut self, data: impl AsRef<[u8]>) -> crate::Result<()> {
        self.writable_log().append(data)?;
        Ok(())
    }

    /// Look up an entry using the given index. The `index_id` is the index of
    /// `index_defs` stored in [`OpenOptions`].
    pub fn lookup(
        &self,
        index_id: usize,
        key: impl Into<Bytes>,
    ) -> crate::Result<RotateLogLookupIter> {
        let key = key.into();
        let result: crate::Result<_> = (|| {
            Ok(RotateLogLookupIter {
                inner_iter: self.logs[0].get().unwrap().lookup(index_id, &key)?,
                end: false,
                log_rotate: self,
                log_index: 0,
                index_id,
                key: key.clone(),
            })
        })();
        result
            .context(|| format!("in RotateLog::lookup({}, {:?})", index_id, key.as_ref()))
            .context(|| format!("  RotateLog.dir = {:?}", self.dir))
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
    ) -> crate::Result<log::LogLookupIter> {
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
        let result: crate::Result<_> = (|| {
            if self.dir.is_none() {
                return Ok(0);
            }

            if self.writable_log().iter_dirty().nth(0).is_none() {
                // Read-only path, no need to take directory lock.
                if let Ok(latest) = read_latest(self.dir.as_ref().unwrap()) {
                    if latest != self.latest {
                        // Latest changed. Re-load and write to the real latest Log.
                        // PERF(minor): This can be smarter by avoiding reloading some logs.
                        self.logs =
                            read_logs(self.dir.as_ref().unwrap(), &self.open_options, latest)?;
                        self.latest = latest;
                    }
                    self.writable_log().sync()?;
                } else {
                    // If latest can not be read, do not error out.
                    // This RotateLog can still be used to answer queries.
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
                                FlushFilterOutput::Drop => (),
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
                    self.logs = new_logs;
                    self.latest = latest;
                }

                let size = self.writable_log().flush()?;
                if size >= self.open_options.max_bytes_per_log {
                    // `self.writable_log()` will be rotated (i.e., becomes immutable).
                    // Make sure indexes are up-to-date so reading it would not require
                    // building missing indexes in-memory.
                    self.writable_log().finalize_indexes()?;
                    self.rotate_internal(&lock)?;
                }
            }

            Ok(self.latest)
        })();

        result
            .context("in RotateLog::sync")
            .context(|| format!("  RotateLog.dir = {:?}", self.dir))
    }

    /// Force create a new [`Log`]. Bump latest.
    ///
    /// This function requires it's protected by a directory lock, and the
    /// callsite makes sure that [`Log`]s are consistent (ex. up-to-date,
    /// and do not have dirty entries in non-writable logs).
    fn rotate_internal(&mut self, lock: &ScopedDirLock) -> crate::Result<()> {
        // Create a new Log. Bump latest.
        let next = self.latest.wrapping_add(1);
        let log = create_empty_log(
            Some(self.dir.as_ref().unwrap()),
            &self.open_options,
            next,
            &lock,
        )?;
        if self.logs.len() >= self.open_options.max_log_count as usize {
            self.logs.pop();
        }
        self.logs.insert(0, create_log_cell(log));
        self.latest = next;
        self.try_remove_old_logs(lock);
        Ok(())
    }

    /// Renamed. Use [`RotateLog::sync`] instead.
    pub fn flush(&mut self) -> crate::Result<u8> {
        self.sync()
    }

    fn try_remove_old_logs(&self, _lock: &ScopedDirLock) {
        if let Ok(read_dir) = self.dir.as_ref().unwrap().read_dir() {
            let latest = self.latest;
            let earliest = latest.wrapping_sub(self.open_options.max_log_count - 1);
            for entry in read_dir {
                if let Ok(entry) = entry {
                    let name = entry.file_name();
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
                                let _ = fs::remove_file(entry.path().join(log::META_FILE))
                                    .and_then(|_| fs::remove_dir_all(entry.path()));
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
        match self.logs.get(index) {
            Some(cell) => {
                let id = self.latest.wrapping_sub(index as u8);
                if let Some(dir) = &self.dir {
                    Ok(Some(cell.get_or_try_init(|| {
                        load_log(&dir, id, &self.open_options)
                    })?))
                } else {
                    Ok(cell.get())
                }
            }
            None => Ok(None),
        }
    }

    /// Iterate over all the entries.
    ///
    /// The entries are returned in FIFO order.
    pub fn iter(&self) -> impl Iterator<Item = crate::Result<&[u8]>> {
        let logs = self.logs();
        logs.into_iter().rev().flat_map(|log| log.iter())
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
fn load_log(dir: &Path, id: u8, open_options: &OpenOptions) -> crate::Result<Log> {
    let name = format!("{}", id);
    let log_path = dir.join(&name);
    open_options
        .log_open_options
        .clone()
        .create(false)
        .open(&log_path)
}

/// Get access to internals of [`RotateLog`].
///
/// This can be useful when there are low-level needs. For example:
/// - Get access to individual logs for things like range query.
/// - Rotate logs manually.
pub trait RotateLowLevelExt {
    /// Get a view of all individual logs. Newest first.
    fn logs(&self) -> Vec<&Log>;

    /// Forced rotate. This can be useful as a quick way to ensure new
    /// data can be written when data corruption happens.
    ///
    /// Data not written will get lost.
    fn force_rotate(&mut self) -> crate::Result<()>;
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

    fn force_rotate(&mut self) -> crate::Result<()> {
        if self.dir.is_none() {
            // rotate does not make sense for an in-memory RotateLog.
            return Ok(());
        }
        // Read-write path. Take the directory lock.
        let dir = self.dir.clone().unwrap();
        let lock = ScopedDirLock::new(&dir)?;
        self.latest = read_latest(self.dir.as_ref().unwrap())?;
        self.rotate_internal(&lock)?;
        self.logs = read_logs(self.dir.as_ref().unwrap(), &self.open_options, self.latest)?;
        Ok(())
    }
}

/// Iterator over [`RotateLog`] entries selected by an index lookup.
pub struct RotateLogLookupIter<'a> {
    inner_iter: log::LogLookupIter<'a>,
    end: bool,
    log_rotate: &'a RotateLog,
    log_index: usize,
    index_id: usize,
    key: Bytes,
}

impl<'a> Iterator for RotateLogLookupIter<'a> {
    type Item = crate::Result<&'a [u8]>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.end {
            return None;
        }
        match self.inner_iter.next() {
            None => {
                if self.log_index + 1 >= self.log_rotate.logs.len() {
                    self.end = true;
                    return None;
                } else {
                    // Try the next log
                    self.log_index += 1;
                    match self.log_rotate.load_log(self.log_index) {
                        Ok(None) => {
                            self.end = true;
                            return None;
                        }
                        Err(_err) => {
                            self.end = true;
                            // Not fatal (since RotateLog is designed to be able
                            // to drop data).
                            return None;
                        }
                        Ok(Some(log)) => {
                            self.inner_iter = match log.lookup(self.index_id, &self.key) {
                                Err(err) => {
                                    self.end = true;
                                    return Some(Err(err));
                                }
                                Ok(iter) => iter,
                            }
                        }
                    }
                    self.next()
                }
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
            utils::atomic_write(&latest_path, latest_str.as_bytes(), false)?;
            log
        }
        None => open_options.log_open_options.clone().create_in_memory()?,
    })
}

fn read_latest(dir: &Path) -> crate::Result<u8> {
    read_latest_raw(dir).context(dir, "cannot read latest")
}

// Unlike read_latest, this function returns io::Result.
fn read_latest_raw(dir: &Path) -> io::Result<u8> {
    let latest_path = dir.join(LATEST_FILE);
    let content: String = fs::read_to_string(&latest_path)?;
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
    let log = load_log(dir, latest, open_options)?;
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
    use super::*;
    use tempfile::tempdir;

    use log::IndexOutput;

    #[test]
    fn test_open() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("rotate");

        assert!(OpenOptions::new().create(false).open(&path).is_err());
        assert!(OpenOptions::new().create(true).open(&path).is_ok());
        assert!(OpenOptions::new()
            .checksum_type(log::ChecksumType::Xxhash64)
            .create(false)
            .open(&path)
            .is_ok());
    }

    // lookup via index 0
    fn lookup<'a>(rotate: &'a RotateLog, key: &[u8]) -> Vec<&'a [u8]> {
        rotate
            .lookup(0, key)
            .unwrap()
            .collect::<crate::Result<Vec<&[u8]>>>()
            .unwrap()
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

        let rotate = opts.clone().open(&dir).unwrap();
        let rotate_mem = opts.clone().create_in_memory().unwrap();

        for rotate in &mut [rotate, rotate_mem] {
            rotate.append(b"aaa").unwrap();
            rotate.append(b"abbb").unwrap();
            rotate.append(b"abc").unwrap();

            assert_eq!(lookup(&rotate, b"aa"), vec![b"aaa"]);
            assert_eq!(lookup(&rotate, b"ab"), vec![&b"abc"[..], b"abbb"]);
            assert_eq!(lookup(&rotate, b"ac"), Vec::<&[u8]>::new());
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
                .filter(|name| name != "lock")
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
    fn test_force_rotate() {
        let dir = tempdir().unwrap();
        let mut rotate = OpenOptions::new()
            .create(true)
            .max_bytes_per_log(1 << 30)
            .max_log_count(3)
            .open(&dir)
            .unwrap();

        use super::RotateLowLevelExt;
        assert_eq!(rotate.logs().len(), 1);
        rotate.force_rotate().unwrap();
        assert_eq!(rotate.logs().len(), 2);
        rotate.force_rotate().unwrap();
        assert_eq!(rotate.logs().len(), 3);
        rotate.force_rotate().unwrap();
        assert_eq!(rotate.logs().len(), 3);
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
        fs::write(dir.path().join("0").join(log::META_FILE), "").unwrap();

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
        rotate.sync().unwrap(); // trigger rotate
        rotate.append(b.clone()).unwrap();
        rotate.sync().unwrap();

        assert_eq!(
            rotate
                .iter()
                .map(|e| e.unwrap().to_vec())
                .collect::<Vec<Vec<u8>>>(),
            vec![a, b]
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
                .open(&dir.path().join("1"))
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
        for &i in [1, 2].iter() {
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
            .index_defs(vec![IndexDef::new("idx", |_| {
                vec![IndexOutput::Reference(0..2)]
            })
            .lag_threshold(u64::max_value())])
            .max_bytes_per_log(100)
            .max_log_count(3);

        let size = |name: &str| dir.path().join(name).metadata().unwrap().len();

        let mut rotate = opts.clone().open(&dir).unwrap();
        rotate.append(vec![b'x'; 200]).unwrap();
        rotate.sync().unwrap();
        rotate.append(vec![b'y'; 200]).unwrap();
        rotate.sync().unwrap();
        rotate.append(vec![b'z'; 10]).unwrap();
        rotate.sync().unwrap();

        // First 2 logs become immutable, indexes are written regardless of
        // lag_threshold.
        assert!(size("0/index-idx") > 0);
        assert!(size("0/log") > 100);

        assert!(size("1/index-idx") > 0);
        assert!(size("1/log") > 100);

        // The "current" log is still mutable. Its index respects lag_threshold,
        // and is logically empty (because side effect of delete_content, the
        // index has some bytes in it).
        assert_eq!(size("2/index-idx"), 10);
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

        let mut rotate2 = opts.clone().open(&dir).unwrap();
        fs::remove_file(dir.path().join(LATEST_FILE)).unwrap();
        rotate2.sync().unwrap(); // not a failure
        rotate2.append(vec![b'y'; 200]).unwrap();
        rotate2.sync().unwrap_err(); // a failure
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
        fs::write(&latest_path, "NaN").unwrap();
        assert!(opts.open(&dir).is_err());
        assert_eq!(
            opts.repair(&dir).unwrap(),
            r#"Attempt to repair log "0"
Verified 1 entries, 223 bytes in log
Attempt to repair log "1"
Verified 1 entries, 223 bytes in log
Attempt to repair log "2"
Verified 0 entries, 12 bytes in log
Reset latest to 2
"#
        );
        opts.open(&dir).unwrap();

        // Delete "latest".
        fs::remove_file(dir.path().join(LATEST_FILE)).unwrap();
        assert!(opts.open(&dir).is_err());

        // Repair can fix it.
        assert_eq!(
            opts.repair(&dir).unwrap(),
            r#"Attempt to repair log "0"
Verified 1 entries, 223 bytes in log
Attempt to repair log "1"
Verified 1 entries, 223 bytes in log
Attempt to repair log "2"
Verified 0 entries, 12 bytes in log
Reset latest to 2
"#
        );
        opts.open(&dir).unwrap();
    }

    #[test]
    fn test_multithread_sync() {
        let dir = tempdir().unwrap();

        // Release mode runs much faster.
        #[cfg(debug_assertions)]
        const THREAD_COUNT: u8 = 10;
        #[cfg(not(debug_assertions))]
        const THREAD_COUNT: u8 = 30;

        #[cfg(debug_assertions)]
        const WRITE_COUNT_PER_THREAD: u8 = 10;
        #[cfg(not(debug_assertions))]
        const WRITE_COUNT_PER_THREAD: u8 = 50;

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

        use std::sync::{Arc, Barrier};
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
                        log.append(&buf).unwrap();
                        if j % (i + 1) == 0 || j == WRITE_COUNT_PER_THREAD {
                            log.sync().unwrap();
                            // Verify that the indexes match the entries.
                            for entry in log.iter().map(|d| d.unwrap()) {
                                for index_id in 0..index_len {
                                    for index_value in log.lookup(index_id, entry).unwrap() {
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
}
