// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Rotation support for a set of [`Log`]s.

use atomicwrites::{AllowOverwrite, AtomicFile};
use bytes::Bytes;
use lock::ScopedFileLock;
use log::{self, IndexDef, Log};
use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use utils::open_dir;

/// A collection of [`Log`]s that get rotated or deleted automatically when they
/// exceed size or count limits.
///
/// Writes go to the active [`Log`]. Reads scan through all [`Log`]s.
pub struct LogRotate {
    dir: PathBuf,
    open_options: OpenOptions,
    logs: Vec<Log>,
    latest: u64,
}

// On disk, a LogRotate is a directory containing:
// - 0/, 1/, 2/, 3/, ...: one Log per directory.
// - latest: a file, the name of the directory that is considered "active".

const LATEST_FILE: &str = "latest";

/// Options used to configure how a [`LogRotate`] is opened.
pub struct OpenOptions {
    max_bytes_per_log: u64,
    max_log_count: u64,
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
    pub fn max_log_count(mut self, count: u64) -> Self {
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

    /// Set whether create the [`LogRotate`] structure if it does not exist.
    pub fn create(mut self, create: bool) -> Self {
        self.log_open_options = self.log_open_options.create(create);
        self
    }

    /// Set the index definitions.
    ///
    /// See [`IndexDef`] for details.
    pub fn index_defs(mut self, index_defs: Vec<IndexDef>) -> Self {
        self.log_open_options = self.log_open_options.index_defs(index_defs);
        self
    }

    /// Open [`LogRotate`] at given location.
    pub fn open(self, dir: impl AsRef<Path>) -> io::Result<LogRotate> {
        let dir = dir.as_ref();

        let latest_path = dir.join(LATEST_FILE);
        let (latest, logs) = if !self.log_open_options.create || latest_path.exists() {
            let latest = read_latest(dir)?;
            (latest, read_logs(dir, &self, latest)?)
        } else {
            fs::create_dir_all(dir)?;
            let mut lock_file = open_dir(dir)?;
            let _lock = ScopedFileLock::new(&mut lock_file, true)?;
            if latest_path.exists() {
                // Two creates raced and the other one has created basic files.
                let latest = read_latest(dir)?;
                (latest, read_logs(dir, &self, latest)?)
            } else {
                (0, vec![create_empty_log(dir, &self, 0)?])
            }
        };

        Ok(LogRotate {
            dir: dir.into(),
            open_options: self,
            logs,
            latest,
        })
    }
}

impl LogRotate {
    /// Append data to the writable [`Log`].
    pub fn append(&mut self, data: impl AsRef<[u8]>) -> io::Result<()> {
        self.writable_log().append(data)
    }

    /// Look up an entry using the given index. The `index_id` is the index of
    /// `index_defs` stored in [`OpenOptions`].
    pub fn lookup(
        &self,
        index_id: usize,
        key: impl Into<Bytes>,
    ) -> io::Result<LogRotateLookupIter> {
        let key = key.into();
        Ok(LogRotateLookupIter {
            inner_iter: self.logs[0].lookup(index_id, &key)?,
            end: false,
            log_rotate: self,
            log_index: 0,
            index_id,
            key,
        })
    }

    /// Write in-memory entries to disk.
    ///
    /// Return the index of the latest [`Log`].
    pub fn flush(&mut self) -> io::Result<u64> {
        let mut lock_file = open_dir(&self.dir)?;
        let _lock = ScopedFileLock::new(&mut lock_file, true)?;

        let latest = read_latest(&self.dir)?;
        if latest != self.latest {
            // Latest changed. Re-load and write to the real latest Log.
            //
            // This is needed because LogRotate assumes non-latest logs
            // are read-only. Other processes using LogRotate won't reload
            // non-latest logs automatically.

            // PERF(minor): This can be smarter by avoiding reloading some logs.
            let mut new_logs = read_logs(&self.dir, &self.open_options, latest)?;
            // Copy entries to new Logs.
            for entry in self.writable_log().iter_dirty() {
                let bytes = entry?;
                new_logs[0].append(bytes)?;
            }
            self.logs = new_logs;
            self.latest = latest;
        }

        let size = self.writable_log().flush()?;

        if size >= self.open_options.max_bytes_per_log {
            // Create a new Log. Bump latest.
            let next = self.latest.wrapping_add(1);
            let log = create_empty_log(&self.dir, &self.open_options, next)?;
            if self.logs.len() as u64 >= self.open_options.max_log_count {
                self.logs.pop();
            }
            self.logs.insert(0, log);
            self.latest = next;
            self.try_remove_old_logs();
        }

        Ok(self.latest)
    }

    fn try_remove_old_logs(&self) {
        if let Ok(read_dir) = self.dir.read_dir() {
            let latest = self.latest;
            let earliest = latest.wrapping_sub(self.open_options.max_log_count - 1);
            for entry in read_dir {
                if let Ok(entry) = entry {
                    let name = entry.file_name();
                    if let Some(name) = name.to_str() {
                        if let Ok(id) = name.parse::<u64>() {
                            if (latest >= earliest && (id > latest || id < earliest))
                                || (latest < earliest && (id > latest && id < earliest))
                            {
                                // Errors are not fatal. On Windows, this can fail if
                                // other processes have files in entry.path() mmap-ed.
                                // Newly opened or flushed LogRotate will umap files.
                                // New rotation would trigger remove_dir_all to try
                                // remove old logs again.
                                let _ = fs::remove_dir_all(entry.path());
                            }
                        }
                    }
                }
            }
        }
    }

    // `writable_log` is public for advanced use-cases. Ex. if a Log is used to
    // store file contents chained with deltas. It might be desirable to make
    // sure the delta parent is within a same log. That can be done by using
    // writable_log().lookup to check the delta parent candidate.
    /// Get the writable [`Log`].
    pub fn writable_log(&mut self) -> &mut Log {
        &mut self.logs[0]
    }
}

/// Iterator over [`LogRotate`] entries selected by an index lookup.
pub struct LogRotateLookupIter<'a> {
    inner_iter: log::LogLookupIter<'a>,
    end: bool,
    log_rotate: &'a LogRotate,
    log_index: usize,
    index_id: usize,
    key: Bytes,
}

impl<'a> Iterator for LogRotateLookupIter<'a> {
    type Item = io::Result<&'a [u8]>;

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
                    self.inner_iter = match self.log_rotate.logs[self.log_index]
                        .lookup(self.index_id, &self.key)
                    {
                        Err(err) => {
                            self.end = true;
                            return Some(Err(err));
                        }
                        Ok(iter) => iter,
                    };
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

fn create_empty_log(dir: &Path, open_options: &OpenOptions, latest: u64) -> io::Result<Log> {
    let latest_path = dir.join(LATEST_FILE);
    let latest_str = format!("{}", latest);
    let log_path = dir.join(&latest_str);
    let log = open_options
        .log_open_options
        .clone()
        .create(true)
        .open(log_path)?;
    AtomicFile::new(&latest_path, AllowOverwrite).write(|f| f.write_all(latest_str.as_bytes()))?;
    Ok(log)
}

fn read_latest(dir: &Path) -> io::Result<u64> {
    fs::read_to_string(&dir.join(LATEST_FILE))?
        .parse()
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn read_logs(dir: &Path, open_options: &OpenOptions, latest: u64) -> io::Result<Vec<Log>> {
    let mut logs = Vec::new();
    let mut current = latest;
    let mut remaining = open_options.max_log_count;
    while remaining > 0 {
        let log_path = dir.join(format!("{}", current));
        if let Ok(log) = open_options.log_open_options.clone().open(&log_path) {
            logs.push(log);
            current = current.wrapping_sub(1);
            remaining -= 1;
        } else {
            break;
        }
    }

    if logs.is_empty() {
        Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("no logs are found in {:?}", &dir),
        ))
    } else {
        Ok(logs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempdir::TempDir;

    use log::IndexOutput;

    #[test]
    fn test_open() {
        let dir = TempDir::new("log").unwrap();
        let path = dir.path().join("rotate");

        assert!(OpenOptions::new().create(false).open(&path).is_err());
        assert!(OpenOptions::new().create(true).open(&path).is_ok());
        assert!(OpenOptions::new()
            .checksum_type(log::ChecksumType::None)
            .create(false)
            .open(&path)
            .is_ok());
    }

    // lookup via index 0
    fn lookup<'a>(rotate: &'a LogRotate, key: &[u8]) -> Vec<&'a [u8]> {
        rotate
            .lookup(0, key)
            .unwrap()
            .collect::<io::Result<Vec<&[u8]>>>()
            .unwrap()
    }

    #[test]
    fn test_trivial_append_lookup() {
        let dir = TempDir::new("log").unwrap();
        let mut rotate = OpenOptions::new()
            .create(true)
            .index_defs(vec![IndexDef::new("two-bytes", |_| {
                vec![IndexOutput::Reference(0..2)]
            })])
            .open(&dir)
            .unwrap();

        rotate.append(b"aaa").unwrap();
        rotate.append(b"abbb").unwrap();
        rotate.append(b"abc").unwrap();

        assert_eq!(lookup(&rotate, b"aa"), vec![b"aaa"]);
        assert_eq!(lookup(&rotate, b"ab"), vec![&b"abc"[..], b"abbb"]);
        assert_eq!(lookup(&rotate, b"ac"), Vec::<&[u8]>::new());
    }

    #[test]
    fn test_simple_rotate() {
        let dir = TempDir::new("log").unwrap();
        let mut rotate = OpenOptions::new()
            .create(true)
            .max_bytes_per_log(100)
            .max_log_count(2)
            .index_defs(vec![IndexDef::new("first-byte", |_| {
                vec![IndexOutput::Reference(0..1)]
            })])
            .open(&dir)
            .unwrap();

        // No rotate.
        rotate.append(b"a").unwrap();
        assert_eq!(rotate.flush().unwrap(), 0);
        rotate.append(b"a").unwrap();
        assert_eq!(rotate.flush().unwrap(), 0);

        // Trigger rotate. "a" is still accessible.
        rotate.append(vec![b'b'; 100]).unwrap();
        assert_eq!(rotate.flush().unwrap(), 1);
        assert_eq!(lookup(&rotate, b"a").len(), 2);

        // Trigger rotate again. Only new entries are accessible.
        // Older directories should be deleted automatically.
        rotate.append(vec![b'c'; 50]).unwrap();
        assert_eq!(rotate.flush().unwrap(), 1);
        rotate.append(vec![b'd'; 50]).unwrap();
        assert_eq!(rotate.flush().unwrap(), 2);
        assert_eq!(lookup(&rotate, b"a").len(), 0);
        assert_eq!(lookup(&rotate, b"b").len(), 0);
        assert_eq!(lookup(&rotate, b"c").len(), 1);
        assert_eq!(lookup(&rotate, b"d").len(), 1);
        assert!(!dir.path().join("0").exists());
    }

    #[test]
    fn test_concurrent_writes() {
        let dir = TempDir::new("log").unwrap();
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
        assert_eq!(rotate1.flush().unwrap(), 1);

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
        assert_eq!(rotate2.flush().unwrap(), 2);

        #[cfg(unix)]
        {
            assert!(!dir.path().join("0").exists());
        }
        assert!(size(1) > size1 + 100);
        assert!(size(2) > 0);
    }
}
