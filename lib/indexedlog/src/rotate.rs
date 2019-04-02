// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Rotation support for a set of [Log]s.

use atomicwrites::{AllowOverwrite, AtomicFile};
use bytes::Bytes;
use lock::ScopedFileLock;
use log::{self, IndexDef, Log};
use std::fs;
use std::io;
use std::io::Write;
use std::path::{Path, PathBuf};
use utils::open_dir;

/// A collection of [Log]s that get rotated or deleted automatically when they
/// exceed size or count limits.
///
/// Writes go to the active [Log]. Reads scan through all [Log]s.
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

/// Options used to configure how a [LogRotate] is opened.
pub struct OpenOptions {
    max_bytes_per_log: u64,
    max_log_count: u64,
    create: bool,
    index_defs: Vec<IndexDef>,
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
            index_defs: Vec::new(),
            create: false,
        }
    }

    /// Set the maximum [Log] count.
    pub fn max_log_count(mut self, count: u64) -> Self {
        assert!(count >= 1);
        self.max_log_count = count;
        self
    }

    /// Set the maximum bytes per [Log].
    pub fn max_bytes_per_log(mut self, bytes: u64) -> Self {
        assert!(bytes > 0);
        self.max_bytes_per_log = bytes;
        self
    }

    /// Set whether create the [LogRotate] structure if it does not exist.
    pub fn create(mut self, create: bool) -> Self {
        self.create = create;
        self
    }

    /// Set the index definitions.
    ///
    /// See [IndexDef] for details.
    pub fn index_defs(mut self, index_defs: Vec<IndexDef>) -> Self {
        self.index_defs = index_defs;
        self
    }

    /// Open [LogRotate] at given location.
    pub fn open(self, dir: impl AsRef<Path>) -> io::Result<LogRotate> {
        let dir = dir.as_ref();

        let latest_path = dir.join(LATEST_FILE);
        let (latest, logs) = if !self.create || latest_path.exists() {
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
    /// Append data to the writable [Log].
    pub fn append(&mut self, data: impl AsRef<[u8]>) -> io::Result<()> {
        self.writable_log().append(data)
    }

    /// Look up an entry using the given index. The `index_id` is the index of
    /// `index_defs` stored in [OpenOptions].
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

    // `writable_log` is public for advanced use-cases. Ex. if a Log is used to
    // store file contents chained with deltas. It might be desirable to make
    // sure the delta parent is within a same log. That can be done by using
    // writable_log().lookup to check the delta parent candidate.
    /// Get the writable [Log].
    pub fn writable_log(&mut self) -> &mut Log {
        &mut self.logs[0]
    }
}

/// Iterator over [LogRotate] entries selected by an index lookup.
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
    let log = log::OpenOptions::new()
        .create(true)
        .index_defs(open_options.index_defs.clone())
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
        if let Ok(log) = log::OpenOptions::new()
            .create(false)
            .index_defs(open_options.index_defs.clone())
            .open(&log_path)
        {
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
        assert!(OpenOptions::new().create(false).open(&path).is_ok());
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

}
