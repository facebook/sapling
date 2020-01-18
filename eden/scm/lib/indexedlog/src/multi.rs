/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Atomic `sync` support for multiple [`Log`]s.

use crate::errors::{IoResultExt, ResultExt};
use crate::lock::ScopedDirLock;
use crate::log::{self, GenericPath, LogMetadata};
use crate::utils;
use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Read};
use std::mem;
use std::ops;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use vlqencoding::{VLQDecode, VLQEncode};

/// Options used to configure how a [`MultiLog`] is opened.
#[derive(Clone, Default)]
pub struct OpenOptions {
    /// Name (subdir) of the Log and its OpenOptions.
    name_open_options: Vec<(&'static str, log::OpenOptions)>,
}

/// A [`MultiLog`] contains multiple [`Log`]s with a centric metadata file.
///
/// Metadata is "frozen" and changes to the metadata on the filesystem are not
/// visible to Logs until [`MultiLog::lock`] gets called.  The only way to write
/// the centric metadata back to the filesystem is [`MultiLog::write_meta`].
/// Note: [`MultiLog::sync`] calls the above functions and is another way to
/// exchange data with the filesystem.
///
/// [`Log`]s will be accessible via indexing. For example, `multilog[0]`
/// accesses the first [`Log`]. [`Log`]s can also be moved out of this
/// struct by [`MultiLog::detach_logs`].
///
/// [`MultiLog`] makes sure the data consistency on disk but not always
/// in memory. In case [`MultiLog::write_meta`] is not called or is not
/// successful, but [`Log::sync`] was called. The data in [`Log`] might
/// be rewritten by other processes, breaking the [`Log`]!
pub struct MultiLog {
    /// Directory containing all the Logs.
    /// Used to write metadata.
    path: PathBuf,

    /// Combined metadata from logs.
    multimeta: MultiMeta,

    /// Logs loaded by MultiLog.
    logs: Vec<log::Log>,
}

#[derive(Default)]
pub struct MultiMeta {
    metas: BTreeMap<String, Arc<Mutex<LogMetadata>>>,
}

impl OpenOptions {
    /// Create [`OpenOptions`] from names and OpenOptions of [`Log`].
    pub fn from_name_opts(name_opts: Vec<(&'static str, log::OpenOptions)>) -> Self {
        // Sanity check.
        for (name, _) in &name_opts {
            if name == &"multimeta" {
                panic!("MultiLog: cannot use 'multimeta' as Log name");
            } else if name.contains('/') || name.contains('\\') {
                panic!("MultiLog: cannot use '/' or '\\' in Log name");
            }
        }
        Self {
            name_open_options: name_opts,
        }
    }

    /// Open [`MultiLog`] at the given directory.
    ///
    /// This ignores the `create` option per [`Log`]. [`Log`] and their metadata
    /// are created on demand.
    pub fn open(&self, path: &Path) -> crate::Result<MultiLog> {
        let result: crate::Result<_> = (|| {
            let meta_path = multi_meta_path(path);
            let mut multimeta = MultiMeta::default();
            match multimeta.read_file(&meta_path) {
                Err(e) => match e.kind() {
                    io::ErrorKind::NotFound => (), // not fatal.
                    _ => return Err(e).context(&meta_path, "when opening MultiLog"),
                },
                Ok(_) => (),
            };

            let locked = if self
                .name_open_options
                .iter()
                .all(|(name, _)| multimeta.metas.contains_key(AsRef::<str>::as_ref(name)))
            {
                // All keys exist. No need to write files on disk.
                None
            } else {
                // Need to create some Logs and rewrite the multimeta.
                utils::mkdir_p(path)?;
                Some(ScopedDirLock::new(path)?)
            };

            let mut logs = Vec::with_capacity(self.name_open_options.len());
            for (name, opts) in self.name_open_options.iter() {
                let fspath = path.join(name);
                let name_ref: &str = name.as_ref();
                if !multimeta.metas.contains_key(name_ref) {
                    // Create a new Log if it does not exist in MultiMeta.
                    utils::mkdir_p(&fspath)?;
                    let meta = log::Log::load_or_create_shared_meta(&fspath.as_path().into())?;
                    let meta = Arc::new(Mutex::new(meta));
                    multimeta.metas.insert(name.to_string(), meta);
                }
                let path = GenericPath::SharedMeta {
                    path: Box::new(fspath.as_path().into()),
                    meta: multimeta.metas[name_ref].clone(),
                };
                let log = opts.open(path)?;
                logs.push(log);
            }

            if locked.is_some() {
                multimeta.write_file(&meta_path)?;
            }

            Ok(MultiLog {
                path: path.to_path_buf(),
                logs,
                multimeta,
            })
        })();

        result.context("in multi::OpenOptions::open")
    }
}

impl MultiLog {
    /// Lock the MultiLog directory for writing.
    ///
    /// After taking the lock, metadata will be reloaded so [`Log`]s can see the
    /// latest metadata on disk and do `sync()` accordingly.
    ///
    /// Once everything is done, use [`MultiLog::write_meta`] to persistent the
    /// changed metadata.
    pub fn lock(&mut self) -> crate::Result<LockGuard> {
        let result: crate::Result<_> = (|| {
            let lock = LockGuard(ScopedDirLock::new(&self.path)?);
            self.read_meta(&lock)?;
            Ok(lock)
        })();
        result.context("in MultiLog::lock")
    }

    /// Write meta to disk so they become visible to other processes.
    ///
    /// A lock must be provided to prove that there is no race condition.
    /// The lock is usually obtained via `lock()`.
    pub fn write_meta(&mut self, lock: &LockGuard) -> crate::Result<()> {
        if lock.0.path() != &self.path {
            let msg = format!(
                "Invalid lock used to write_meta (Lock path = {:?}, MultiLog path = {:?})",
                lock.0.path(),
                &self.path
            );
            return Err(crate::Error::programming(msg));
        }
        let result: crate::Result<_> = (|| {
            let meta_path = multi_meta_path(&self.path);
            self.multimeta.write_file(&meta_path)?;
            Ok(())
        })();
        result.context("in MultiLog::write_meta")
    }

    /// Reload meta from disk so they become visible to Logs.
    ///
    /// This is called automatically by `lock` so it's not part of the
    /// public interface.
    fn read_meta(&mut self, lock: &LockGuard) -> crate::Result<()> {
        debug_assert_eq!(lock.0.path(), &self.path);
        let meta_path = multi_meta_path(&self.path);
        match self.multimeta.read_file(&meta_path) {
            Err(err) => {
                if err.kind() == io::ErrorKind::NotFound {
                    Ok(())
                } else {
                    Err(err).context(&meta_path, "reloading meta")
                }
            }
            Ok(()) => Ok(()),
        }
    }

    /// Detach [`Log`]s from this [`MultiLog`].
    ///
    /// Once detached, [`Log`]s will no longer be available via indexing
    /// like `multilog[0]`.
    ///
    /// This is useful for places where [`Log`]s are owned by other
    /// structured, instead of being accessed via [`MultiLog`].
    pub fn detach_logs(&mut self) -> Vec<log::Log> {
        let mut result = Vec::new();
        mem::swap(&mut result, &mut self.logs);
        result
    }

    /// Sync all [`Log`]s. This is an atomic operation.
    ///
    /// This function simply calls [`MultiLog::lock`], [`Log::sync`] and
    /// [`MultiLog::write_meta`]. For more advanced use-cases, call those
    /// functions manually.
    ///
    /// (This does not seem very useful practically. So it is private.)
    pub fn sync(&mut self) -> crate::Result<()> {
        let lock = self.lock()?;
        for log in self.logs.iter_mut() {
            log.sync()?;
        }
        self.write_meta(&lock)?;
        Ok(())
    }
}

/// Structure proving a lock was taken for [`MultiLog`].
pub struct LockGuard(ScopedDirLock);

impl ops::Index<usize> for MultiLog {
    type Output = log::Log;
    fn index(&self, index: usize) -> &Self::Output {
        &self.logs[index]
    }
}

impl ops::IndexMut<usize> for MultiLog {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.logs[index]
    }
}

fn multi_meta_path(dir: &Path) -> PathBuf {
    dir.join("multimeta")
}

impl MultiMeta {
    /// Update self with content from a reader.
    /// Metadata with existing keys are mutated in-place.
    fn read(&mut self, mut reader: impl io::Read) -> io::Result<()> {
        let version: usize = reader.read_vlq()?;
        if version != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("MultiMeta version is unsupported: {}", version),
            ));
        }
        let count: usize = reader.read_vlq()?;
        for _ in 0..count {
            let name_len = reader.read_vlq()?;
            let mut name_buf = vec![0; name_len];
            reader.read_exact(&mut name_buf)?;
            let name = String::from_utf8(name_buf)
                .map_err(|_| io::Error::new(io::ErrorKind::Other, "Log name is not utf-8"))?;
            let meta = LogMetadata::read(&mut reader)?;
            self.metas
                .entry(name.to_string())
                .and_modify(|e| {
                    let mut e = e.lock().unwrap();
                    let truncated = e.primary_len > meta.primary_len && e.epoch == meta.epoch;
                    *e = meta.clone();
                    // Force a different epoch for truncation.
                    if truncated {
                        e.epoch = e.epoch.wrapping_add(1);
                    }
                })
                .or_insert_with(|| Arc::new(Mutex::new(meta.clone())));
        }
        Ok(())
    }

    /// Write metadata to a writer.
    fn write(&self, mut writer: impl io::Write) -> io::Result<()> {
        let version = 0;
        writer.write_vlq(version)?;
        writer.write_vlq(self.metas.len())?;
        for (name, meta) in self.metas.iter() {
            writer.write_vlq(name.len())?;
            writer.write_all(name.as_bytes())?;
            meta.lock().unwrap().write(&mut writer)?;
        }
        Ok(())
    }

    /// Update self with metadata from a file.
    pub fn read_file<P: AsRef<Path>>(&mut self, path: P) -> io::Result<()> {
        let mut file = fs::OpenOptions::new().read(true).open(path)?;
        let mut buf = Vec::new();
        file.read_to_end(&mut buf)?;
        self.read(&buf[..])
    }

    /// Atomically write metadata to a file.
    pub fn write_file<P: AsRef<Path>>(&self, path: P) -> crate::Result<()> {
        let mut buf = Vec::new();
        self.write(&mut buf).infallible()?;
        utils::atomic_write(path, &buf, false)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;

    /// Create a simple MultiLog containing Log 'a' and 'b' for testing.
    fn simple_multilog(path: &Path) -> MultiLog {
        let mopts = OpenOptions::from_name_opts(vec![
            ("a", log::OpenOptions::new()),
            ("b", log::OpenOptions::new()),
        ]);
        mopts.open(path).unwrap()
    }

    #[test]
    fn test_individual_log_cannot_be_opened_directly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let mut mlog = simple_multilog(path);

        assert_eq!(
            log::OpenOptions::new()
                .open(path.join("a"))
                .unwrap_err()
                .to_string()
                .lines()
                .last()
                .unwrap(),
            "- This Log is managed by MultiLog. Direct access is forbidden!"
        );
        log::OpenOptions::new().open(path.join("b")).unwrap_err();

        // It's still an error after individual log flush.
        mlog[0].append(b"1").unwrap();
        mlog[0].flush().unwrap();
        log::OpenOptions::new().open(path.join("a")).unwrap_err();
    }

    #[test]
    fn test_individual_log_flushes_are_invisible() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let mut mlog = simple_multilog(path);

        // This is not a proper use of Log::sync, since
        // it's not protected by a lock. But it demostrates
        // the properties.
        mlog[0].append(b"2").unwrap();
        mlog[0].sync().unwrap();
        mlog[0].append(b"3").unwrap();
        mlog[0].append(b"4").unwrap();

        mlog[1].append(b"y").unwrap();
        mlog[1].sync().unwrap();
        mlog[1].append(b"z").unwrap();
        mlog[1].sync().unwrap();

        assert_eq!(mlog[0].iter().count(), 3);
        assert_eq!(mlog[1].iter().count(), 2);

        // mlog changes are not written via MultiLog::write_meta.
        // Therefore invisible to mlog2.
        let mlog2 = simple_multilog(path);
        assert_eq!(mlog2[0].iter().count(), 0);
        assert_eq!(mlog2[1].iter().count(), 0);

        // mlog.sync reloads multimeta. "Flushed" contents are dropped.
        // But in-memory content is kept and written.
        mlog.sync().unwrap();
        assert_eq!(mlog[0].iter().count(), 2);
        assert_eq!(mlog[1].iter().count(), 0);

        let mlog2 = simple_multilog(path);
        assert_eq!(mlog2[0].iter().count(), 2);
        assert_eq!(mlog2[1].iter().count(), 0);
    }

    #[test]
    fn test_detach_logs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let mut mlog = simple_multilog(path);
        let mut logs = mlog.detach_logs();
        logs[0].append(b"0").unwrap();
        logs[1].append(b"1").unwrap();

        // Although logs are detached. MultiLog can still update multimeta.
        let lock = mlog.lock().unwrap();
        logs[0].sync().unwrap();
        logs[1].sync().unwrap();
        mlog.write_meta(&lock).unwrap();

        let mlog2 = simple_multilog(path);
        assert_eq!(mlog2[0].iter().count(), 1);
        assert_eq!(mlog2[1].iter().count(), 1);
    }

    #[test]
    fn test_wrong_locks_cause_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let mut mlog1 = simple_multilog(&path.join("1"));
        let mut mlog2 = simple_multilog(&path.join("2"));

        let lock1 = mlog1.lock().unwrap();
        let lock2 = mlog2.lock().unwrap();
        assert!(mlog1.write_meta(&lock2).is_err());
        assert!(mlog2.write_meta(&lock1).is_err());
    }

    quickcheck! {
        fn test_roundtrip_multimeta(name_len_list: Vec<(String, u64)>) -> bool {
            let metas = name_len_list
                .into_iter()
                .map(|(name, len)| {
                    let meta = LogMetadata::new_with_primary_len(len);
                    (name, Arc::new(Mutex::new(meta)))
                })
                .collect();
            let meta = MultiMeta { metas, ..Default::default() };
            let mut buf = Vec::new();
            meta.write(&mut buf).unwrap();
            let mut meta2 = MultiMeta::default();
            meta2.read(&buf[..]).unwrap();
            let mut buf2 = Vec::new();
            meta2.write(&mut buf2).unwrap();
            assert_eq!(buf2, buf);
            buf2 == buf
        }

        fn test_roundtrip_multilog(list_a: Vec<Vec<u8>>, list_b: Vec<Vec<u8>>) -> bool {
            let dir = tempfile::tempdir().unwrap();
            let mut mlog = simple_multilog(dir.path());
            for a in &list_a {
                mlog[0].append(a).unwrap();
            }
            for b in &list_b {
                mlog[1].append(b).unwrap();
            }
            mlog.sync().unwrap();

            let mlog_read = simple_multilog(dir.path());
            let list_a_read: Vec<Vec<u8>> = mlog_read[0].iter().map(|e| e.unwrap().to_vec()).collect();
            let list_b_read: Vec<Vec<u8>> = mlog_read[1].iter().map(|e| e.unwrap().to_vec()).collect();

            list_a == list_a_read && list_b == list_b_read
        }
    }
}
