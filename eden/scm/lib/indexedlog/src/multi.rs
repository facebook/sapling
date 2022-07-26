/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Atomic `sync` support for multiple [`Log`]s.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::io;
use std::mem;
use std::ops;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use vlqencoding::VLQDecode;
use vlqencoding::VLQEncode;

use crate::errors::IoResultExt;
use crate::errors::ResultExt;
use crate::lock::ScopedDirLock;
use crate::lock::READER_LOCK_OPTS;
use crate::log;
use crate::log::GenericPath;
use crate::log::LogMetadata;
use crate::repair::OpenOptionsOutput;
use crate::repair::OpenOptionsRepair;
use crate::repair::RepairMessage;
use crate::utils;
use crate::utils::rand_u64;

/// Options used to configure how a [`MultiLog`] is opened.
#[derive(Clone, Default)]
pub struct OpenOptions {
    /// Name (subdir) of the Log and its OpenOptions.
    name_open_options: Vec<(&'static str, log::OpenOptions)>,

    /// Whether to use legacy MultiMeta source.
    /// true: use "multimeta" file; false: use "multimeta_log" Log.
    /// For testing purpose only.
    leacy_multimeta_source: bool,
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

    /// Log used for `MultiMeta`. For data recovery.
    multimeta_log: log::Log,

    /// Whether to use legacy MultiMeta source.
    /// true: use "multimeta" file; false: use "multimeta_log" Log.
    /// For testing purpose only.
    leacy_multimeta_source: bool,

    /// Indicate an active reader. Destrictive writes (repair) are unsafe.
    reader_lock: ScopedDirLock,
}

/// Constant for the reverse index of multimeta log.
const INDEX_REVERSE_KEY: &[u8] = b"r";

/// The reverse index is the first index. See [`multi_meta_log_open_options`].
const INDEX_REVERSE: usize = 0;

#[derive(Debug)]
pub struct MultiMeta {
    metas: BTreeMap<String, Arc<Mutex<LogMetadata>>>,

    /// The version. Updated on flush.
    /// `(a, b)` is backwards compatible (only has appended content) with
    /// `(c, d)` if `a == c` and `b >= d`.
    version: (u64, u64),
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
            leacy_multimeta_source: false,
        }
    }

    /// Open [`MultiLog`] at the given directory.
    ///
    /// This ignores the `create` option per [`Log`]. [`Log`] and their metadata
    /// are created on demand.
    pub fn open(&self, path: &Path) -> crate::Result<MultiLog> {
        let result: crate::Result<_> = (|| {
            let reader_lock = ScopedDirLock::new_with_options(path, &READER_LOCK_OPTS)?;

            // The multimeta log contains the "MultiMeta" metadata about how to load other
            // logs.
            let meta_log_path = multi_meta_log_path(&path);
            let meta_path = multi_meta_path(path);
            let mut multimeta_log = multi_meta_log_open_options().open(&meta_log_path)?;
            let multimeta_log_is_empty = multimeta_log.iter().next().is_none();

            // Read meltimeta from the multimeta log.
            let mut multimeta = MultiMeta::default();
            if multimeta_log_is_empty || self.leacy_multimeta_source {
                // Previous versions of MultiLog uses the "multimeta" file. Read it for
                // compatibility.
                multimeta.read_file(&meta_path)?;
            } else {
                // New version uses a Log for the "multimeta" data. It enables "repair()".
                multimeta.read_log(&multimeta_log)?;
                apply_legacy_meta_if_it_is_newer(&meta_path, &mut multimeta);
            }

            let locked = if !multimeta_log_is_empty
                && self
                    .name_open_options
                    .iter()
                    .all(|(name, _)| multimeta.metas.contains_key(AsRef::<str>::as_ref(name)))
            {
                // Not using legacy format. All keys exist. No need to write files on disk.
                None
            } else {
                // Need to create some Logs and rewrite the multimeta.
                utils::mkdir_p(path)?;
                Some(LockGuard(ScopedDirLock::new(path)?))
            };

            let mut logs = Vec::with_capacity(self.name_open_options.len());
            for (name, opts) in self.name_open_options.iter() {
                let fspath = path.join(name);
                let name_ref: &str = name;
                if !multimeta.metas.contains_key(name_ref) {
                    // Create a new Log if it does not exist in MultiMeta.
                    utils::mkdir_p(&fspath)?;
                    let meta = log::Log::load_or_create_meta(&fspath.as_path().into(), true)?;
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

            if let Some(locked) = locked.as_ref() {
                if !self.leacy_multimeta_source {
                    multimeta.write_log(&mut multimeta_log, locked)?;
                }
                multimeta.write_file(&meta_path)?;
            }

            Ok(MultiLog {
                path: path.to_path_buf(),
                logs,
                multimeta,
                multimeta_log,
                leacy_multimeta_source: self.leacy_multimeta_source,
                reader_lock,
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
        if lock.0.path() != self.path {
            let msg = format!(
                "Invalid lock used to write_meta (Lock path = {:?}, MultiLog path = {:?})",
                lock.0.path(),
                &self.path
            );
            return Err(crate::Error::programming(msg));
        }
        let result: crate::Result<_> = (|| {
            self.multimeta.bump_version();
            if !self.leacy_multimeta_source {
                // New MultiLog uses multimeta_log to track MultiMeta.
                self.multimeta.write_log(&mut self.multimeta_log, lock)?;
            }

            // Legacy MultiLog uses multimeta file to track MultiMeta.
            let meta_path = multi_meta_path(&self.path);
            self.multimeta.write_file(&meta_path)?;

            Ok(())
        })();
        result.context("in MultiLog::write_meta")
    }

    /// Return the version in `(a, b)` form.
    ///
    /// Version `(a, b)` only has append-only data than version `(c, d)`, if
    /// `a == c` and `b > d`.
    ///
    /// Version `(a, _)` is incompatible with version `(b, _)` if `a != b`.
    ///
    /// Version gets updated on `write_meta`.
    pub fn version(&self) -> (u64, u64) {
        self.multimeta.version
    }

    /// Reload meta from disk so they become visible to Logs.
    ///
    /// This is called automatically by `lock` so it's not part of the
    /// public interface.
    fn read_meta(&mut self, lock: &LockGuard) -> crate::Result<()> {
        debug_assert_eq!(lock.0.path(), &self.path);
        (|| -> crate::Result<()> {
            let meta_path = multi_meta_path(&self.path);
            if self.leacy_multimeta_source {
                self.multimeta.read_file(&meta_path)?;
            } else {
                self.multimeta_log.clear_dirty()?;
                self.multimeta_log.sync()?;
                self.multimeta.read_log(&self.multimeta_log)?;
                apply_legacy_meta_if_it_is_newer(&meta_path, &mut self.multimeta);
            }
            Ok(())
        })()
        .context("reloading multimeta")
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
    /// This function should not be called if logs were detached.
    /// This does not seem very useful practically. So it is private.
    fn sync(&mut self) -> crate::Result<()> {
        let lock = self.lock()?;
        for log in self.logs.iter_mut() {
            log.sync()?;
        }
        self.write_meta(&lock)?;
        Ok(())
    }
}

fn apply_legacy_meta_if_it_is_newer(meta_path: &Path, multimeta: &mut MultiMeta) {
    // For safe migration. Also check the "multimeta" file.
    // It can contain newer data if written by an older version.
    let mut maybe_new_multimeta = MultiMeta::default();
    if maybe_new_multimeta.read_file(meta_path).is_ok() {
        if maybe_new_multimeta.metas.iter().all(|(k, v)| {
            v.lock().unwrap().primary_len
                >= match multimeta.metas.get(k) {
                    None => 0,
                    Some(v) => v.lock().unwrap().primary_len,
                }
        }) {
            // Only update "primary_len" and "indexes" metadata in place.
            // The "epoch" might contain changes that need to be preserved.
            for (k, v) in multimeta.metas.iter() {
                let mut current = v.lock().unwrap();
                if let Some(newer) = maybe_new_multimeta.metas.remove(k) {
                    let newer = newer.lock().unwrap();
                    current.primary_len = newer.primary_len;
                    current.indexes = newer.indexes.clone();
                }
            }
        }
    }
}

fn multi_meta_log_open_options() -> log::OpenOptions {
    log::OpenOptions::new()
        .index("reverse", |_data| -> Vec<_> {
            // Reverse index so we can find the last entries quickly.
            vec![log::IndexOutput::Owned(
                INDEX_REVERSE_KEY.to_vec().into_boxed_slice(),
            )]
        })
        .create(true)
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

impl OpenOptionsRepair for OpenOptions {
    fn open_options_repair(&self, path: impl AsRef<Path>) -> crate::Result<String> {
        let path = path.as_ref();
        let lock = LockGuard(ScopedDirLock::new(path)?);
        let mut out = RepairMessage::new(path);

        // First, repair the MultiMeta log.
        let mpath = multi_meta_log_path(path);
        out += "Repairing MultiMeta Log:\n";
        out += &indent(&multi_meta_log_open_options().open_options_repair(&mpath)?);

        // Then, repair each logs.
        let mut repaired_log_metas = HashMap::new();
        for (name, opts) in self.name_open_options.iter() {
            let fspath = path.join(name);
            if !fspath.exists() {
                out += &format!("Skipping non-existed Log {}\n", name);
                continue;
            }
            out += &format!("Repairing Log {}\n", name);
            out += &indent(&opts.open_options_repair(&fspath)?);
            let log = opts.open(&fspath)?;
            let len = log.meta.primary_len;
            out += &format!("Log {} has valid length {} after repair\n", name, len);
            repaired_log_metas.insert(*name, log.meta);
        }

        // Finally, figure out a good "multimeta" from the multimeta log.
        let mut mlog = multi_meta_log_open_options()
            .open(&mpath)
            .context("repair cannot open MultiMeta Log after repairing it")?;
        let mut selected_meta = None;
        let mut invalid_count = 0;
        for entry in mlog.lookup(INDEX_REVERSE, INDEX_REVERSE_KEY)? {
            // The linked list in the index is in the reversed order.
            // So the first entry contains the last root id.
            if let Ok(data) = entry {
                let mut mmeta = MultiMeta::default();
                if mmeta.read(data).is_ok() {
                    // Check if everything is okay.
                    if mmeta.metas.iter().all(|(name, meta)| {
                        let len_required = meta.lock().unwrap().primary_len;
                        let len_provided = repaired_log_metas
                            .get(name.as_str())
                            .map(|m| m.primary_len)
                            .unwrap_or_default();
                        len_required <= len_provided
                    }) {
                        if invalid_count > 0 {
                            // Write repair log.
                            let mmeta_desc = mmeta
                                .metas
                                .iter()
                                .map(|(name, meta)| {
                                    format!("{}: {}", name, meta.lock().unwrap().primary_len)
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            out += &format!(
                                "Found valid MultiMeta after {} invalid entries: {}\n",
                                invalid_count, mmeta_desc
                            );
                        }
                        selected_meta = Some(mmeta);
                        break;
                    } else {
                        invalid_count += 1;
                    }
                }
            }
        }

        if selected_meta.is_none() {
            // For legacy MultiLog, the MultiMeta is stored in the file.
            let mut mmeta = MultiMeta::default();
            if mmeta.read_file(&multi_meta_path(path)).is_ok() {
                selected_meta = Some(mmeta);
            }
        }

        let selected_meta = match selected_meta {
            None => {
                return Err(crate::Error::corruption(
                    &mpath,
                    "repair cannot find valid MultiMeta",
                ))
                .context(|| format!("Repair log:\n{}", indent(out.as_str())));
            }
            Some(meta) => meta,
        };

        let mut should_write_new_meta_entry = invalid_count > 0;
        for (name, log_meta) in selected_meta.metas.iter() {
            let mut log_meta = log_meta.lock().unwrap();
            let should_invalidate_indexes = match repaired_log_metas.get(name.as_str()) {
                None => true,
                Some(repaired_log_meta) => &*log_meta != repaired_log_meta,
            };
            if should_invalidate_indexes {
                out += &format!("Invalidated indexes in log '{}'\n", name);
                log_meta.indexes.clear();
                should_write_new_meta_entry = true;
            }
        }

        if should_write_new_meta_entry {
            selected_meta
                .write_log(&mut mlog, &lock)
                .context("repair cannot write MultiMeta log")?;
            selected_meta
                .write_file(multi_meta_path(path))
                .context("repair cannot write valid MultiMeta file")?;
            out += "Write valid MultiMeta\n";
        } else {
            out += "MultiMeta is valid\n";
        }

        Ok(out.into_string())
    }
}

impl OpenOptionsOutput for OpenOptions {
    type Output = MultiLog;

    fn open_path(&self, path: &Path) -> crate::Result<Self::Output> {
        self.open(path)
    }
}

fn multi_meta_path(dir: &Path) -> PathBuf {
    dir.join("multimeta")
}

fn multi_meta_log_path(dir: &Path) -> PathBuf {
    dir.join("multimetalog")
}

/// Indent lines by 2 spaces.
fn indent(s: &str) -> String {
    s.lines()
        .map(|l| format!("  {}\n", l))
        .collect::<Vec<_>>()
        .concat()
}

impl Default for MultiMeta {
    fn default() -> Self {
        Self {
            metas: Default::default(),
            version: (rand_u64(), 0),
        }
    }
}

impl MultiMeta {
    /// Update self with content from a reader.
    /// Metadata with existing keys are mutated in-place.
    fn read(&mut self, mut reader: impl io::Read) -> io::Result<()> {
        let format_version: usize = reader.read_vlq()?;
        if format_version != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                format!("MultiMeta format {} is unsupported", format_version),
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
        let version_major: u64 = reader.read_vlq().unwrap_or_else(|_| rand_u64());
        let version_minor: u64 = reader.read_vlq().unwrap_or_default();
        self.version = (version_major, version_minor);
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
        writer.write_vlq(self.version.0)?;
        writer.write_vlq(self.version.1)?;
        Ok(())
    }

    /// Update self with metadata from a file (legacy, for backwards compatibility).
    /// If the file does not exist, self is not updated.
    fn read_file<P: AsRef<Path>>(&mut self, path: P) -> crate::Result<()> {
        let path = path.as_ref();
        match utils::atomic_read(path) {
            Ok(buf) => self.read(&buf[..]),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => Err(e),
        }
        .context(path, "when decoding MultiMeta")
    }

    /// Atomically write metadata to a file (legacy, for backwards compatibility).
    fn write_file<P: AsRef<Path>>(&self, path: P) -> crate::Result<()> {
        let mut buf = Vec::new();
        self.write(&mut buf).infallible()?;
        utils::atomic_write(path, &buf, false)?;
        Ok(())
    }

    /// Update self from a [`log::Log`].
    fn read_log(&mut self, log: &log::Log) -> crate::Result<()> {
        if let Some(last_entry) = log.lookup(INDEX_REVERSE, INDEX_REVERSE_KEY)?.next() {
            let data = last_entry?;
            self.read(data).context(
                log.path().as_opt_path().unwrap_or_else(|| Path::new("")),
                "when decoding MutltiMeta",
            )?;
        }
        Ok(())
    }

    /// Write metadata to a [`log::Log`] and persist to disk.
    fn write_log(&self, log: &mut log::Log, _lock: &LockGuard) -> crate::Result<()> {
        let mut data = Vec::new();
        self.write(&mut data).infallible()?;
        // Reload to check if the last entry is already up-to-date.
        log.clear_dirty()?;
        log.sync()?;
        if let Some(Ok(last_data)) = log.lookup(INDEX_REVERSE, INDEX_REVERSE_KEY)?.next() {
            if last_data == &data {
                // log does not change. Do not write redundant data.
                return Ok(());
            }
        }
        log.append(&data)?;
        log.sync()?;
        Ok(())
    }

    /// Bump the version recorded in this [`MultiMeta`].
    fn bump_version(&mut self) {
        self.version.1 += 1;
    }
}

#[cfg(test)]
mod tests {
    use log::tests::pwrite;
    use quickcheck::quickcheck;

    use super::*;

    fn simple_open_opts() -> OpenOptions {
        OpenOptions::from_name_opts(vec![
            ("a", log::OpenOptions::new()),
            ("b", log::OpenOptions::new()),
        ])
    }

    /// Create a simple MultiLog containing Log 'a' and 'b' for testing.
    fn simple_multilog(path: &Path) -> MultiLog {
        let mopts = simple_open_opts();
        mopts.open(path).unwrap()
    }

    fn index_open_opts() -> OpenOptions {
        fn index_func(bytes: &[u8]) -> Vec<log::IndexOutput> {
            (0..bytes.len() as u64)
                .map(|i| log::IndexOutput::Reference(i..i + 1))
                .collect()
        }
        let index_def = log::IndexDef::new("x", index_func).lag_threshold(0);
        OpenOptions::from_name_opts(vec![(
            "a",
            log::OpenOptions::new().index_defs(vec![index_def]),
        )])
    }

    #[test]
    fn test_individual_log_can_be_opened_directly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let mut mlog = simple_multilog(path);

        log::OpenOptions::new().open(path.join("a")).unwrap();
        log::OpenOptions::new().open(path.join("b")).unwrap();

        // After flush - still readable.
        mlog[0].append(b"1").unwrap();
        mlog[0].flush().unwrap();
        log::OpenOptions::new().open(path.join("a")).unwrap();
    }

    #[test]
    fn test_individual_log_flushes_are_invisible() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let mut mlog = simple_multilog(path);

        // This is not a proper use of Log::sync, since
        // it's not protected by a lock. But it demonstrates
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
    fn test_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let mut mlog1 = simple_multilog(&path.join("1"));
        let mut mlog2 = simple_multilog(&path.join("2"));

        // Different logs have different versions.
        let v1 = mlog1.version();
        let v2 = mlog2.version();
        assert!(v1.1 == 0);
        assert!(v2.1 == 0);
        assert_ne!(v1, v2);

        // The second number of the version gets bumped on flush.
        mlog1.sync().unwrap();
        mlog2.sync().unwrap();
        let v3 = mlog1.version();
        let v4 = mlog2.version();
        assert_eq!(v3.0, v1.0);
        assert_eq!(v4.0, v2.0);
        assert!(v3 > v1);
        assert!(v4 > v2);

        // Reopen preserves the versions.
        let mlog1 = simple_multilog(&path.join("1"));
        let mlog2 = simple_multilog(&path.join("2"));
        let v5 = mlog1.version();
        let v6 = mlog2.version();
        assert_eq!(v5, v3);
        assert_eq!(v6, v4);
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
        drop(lock);

        let mlog2 = simple_multilog(path);
        assert_eq!(mlog2[0].iter().count(), 1);
        assert_eq!(mlog2[1].iter().count(), 1);
    }

    #[test]
    fn test_new_index_built_only_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let mopts = OpenOptions::from_name_opts(vec![("a", log::OpenOptions::new())]);
        let mut mlog = mopts.open(path).unwrap();
        mlog[0].append(b"0").unwrap();
        mlog.sync().unwrap();

        // Reopen with an index newly defined.
        let index_def =
            log::IndexDef::new("i", |_| vec![log::IndexOutput::Reference(0..1)]).lag_threshold(0);
        let mopts = OpenOptions::from_name_opts(vec![(
            "a",
            log::OpenOptions::new().index_defs(vec![index_def.clone()]),
        )]);
        let index_size = || {
            path.join("a")
                .join(index_def.filename())
                .metadata()
                .map(|m| m.len())
                .unwrap_or_default()
        };

        assert_eq!(index_size(), 0);

        // Open one time, index is built on demand.
        let _mlog = mopts.open(path).unwrap();
        assert_eq!(index_size(), 36);

        // Open another time, index is reused.
        let mut mlog = mopts.open(path).unwrap();
        assert_eq!(index_size(), 36);

        // Force updating epoch to make multimeta and per-log meta incompatible.
        let lock = LockGuard(ScopedDirLock::new(path).unwrap());
        mlog.multimeta.metas["a"].lock().unwrap().epoch ^= 1;
        mlog.multimeta
            .write_log(&mut mlog.multimeta_log, &lock)
            .unwrap();
        mlog.multimeta.write_file(&multi_meta_path(path)).unwrap();
        drop(lock);

        // The index is rebuilt (appended) at open time because of incompatible meta.
        let _mlog = mopts.open(path).unwrap();
        assert_eq!(index_size(), 71);
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

    fn repair_output(opts: &OpenOptions, path: &Path) -> String {
        let out = opts.open_options_repair(path).unwrap();
        filter_repair_output(out)
    }

    fn filter_repair_output(out: String) -> String {
        // Filter out dynamic content.
        out.lines()
            .filter(|l| {
                !l.contains("bytes in log")
                    && !l.contains("Backed up")
                    && !l.contains("Processing")
                    && !l.contains("date -d")
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn test_repair() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let opts = simple_open_opts();
        let mut mlog = opts.open(&path).unwrap();
        let mut logs = mlog.detach_logs();

        // Create 10 "multimeta"s. Each MultiMeta contains N entires for each log.
        const N: usize = 12;
        for i in 0..10u32 {
            let lock = mlog.lock().unwrap();
            for _ in 0..N {
                logs[0].append(i.to_be_bytes()).unwrap();
                logs[1].append(i.to_be_bytes()).unwrap();
                logs[0].sync().unwrap();
            }
            logs[1].sync().unwrap();
            mlog.write_meta(&lock).unwrap();
        }

        let repair = || repair_output(&opts, path);

        // Check that both logs only have a multiple of N entries.
        let verify = || {
            let mlog = opts.open(&path).unwrap();
            assert_eq!(mlog.logs[0].iter().count() % N, 0);
            assert_eq!(mlog.logs[1].iter().count() % N, 0);
        };

        // Valid MultiLog.
        let s1 = repair();
        assert_eq!(
            &s1,
            r#"Repairing MultiMeta Log:
  Index "reverse" passed integrity check
Repairing Log a
Log a has valid length 1212 after repair
Repairing Log b
Log b has valid length 1212 after repair
MultiMeta is valid"#
        );

        // Repair output is also written to "repair.log" file.
        let s2 = filter_repair_output(std::fs::read_to_string(path.join("repair.log")).unwrap());
        assert_eq!(&s1, s2.trim_end());

        // Put bad data in the first log. The repair will pick a recent MultiMeta point and
        // dropping some entries.
        pwrite(&path.join("a").join("log"), 1000, b"ff");
        assert_eq!(
            repair(),
            r#"Repairing MultiMeta Log:
  Index "reverse" passed integrity check
Repairing Log a
  Reset log size to 992
Log a has valid length 992 after repair
Repairing Log b
Log b has valid length 1212 after repair
Found valid MultiMeta after 2 invalid entries: a: 972, b: 972
Invalidated indexes in log 'a'
Invalidated indexes in log 'b'
Write valid MultiMeta"#
        );
        verify();

        assert_eq!(
            repair(),
            r#"Repairing MultiMeta Log:
  Index "reverse" passed integrity check
Repairing Log a
Log a has valid length 992 after repair
Repairing Log b
Log b has valid length 1212 after repair
Invalidated indexes in log 'a'
Invalidated indexes in log 'b'
Write valid MultiMeta"#
        );
    }

    #[test]
    fn test_repair_broken_index() {
        // Test repair where the logs are fine but the indexes are broken.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        let opts = index_open_opts();
        let mut mlog = opts.open(&path).unwrap();
        let mut logs = mlog.detach_logs();

        let repair = || repair_output(&opts, path);
        let file_size = |path| std::fs::metadata(path).unwrap().len();

        let meta_path = multi_meta_path(path);
        let meta_log_path = multi_meta_log_path(path).join("log");
        let index_path = path.join("a").join("index2-x");

        // Write some data. Flush the "log" multiple times to cause index
        // fragmentation so the rebuilt index would be shorter.
        let mut meta_log_sizes = Vec::new();
        let mut index_sizes = Vec::new();
        for data in [b"abcd", b"abce", b"acde", b"bcde"] {
            let lock = mlog.lock().unwrap();
            logs[0].append(data).unwrap();
            logs[0].sync().unwrap();
            mlog.write_meta(&lock).unwrap();
            meta_log_sizes.push(file_size(&meta_log_path));
            index_sizes.push(file_size(&index_path));
        }
        drop(mlog);
        drop(logs);

        // Corrupt the index and the multimeta log so the repair
        // logic would revert to a previous MultiMeta, and rebuild
        // index. If it's not careful, MultiMeta can contain offsets
        // to the index file that is no longer valid.
        pwrite(&index_path, -4, b"ffff");
        pwrite(&meta_log_path, (meta_log_sizes[1] - 5) as _, b"xxxxx");
        std::fs::remove_file(&meta_path).unwrap();

        let index_len_before = file_size(&index_path);
        assert_eq!(
            repair(),
            r#"Repairing MultiMeta Log:
  Reset log size to 111
  Rebuilt index "reverse"
Repairing Log a
  Rebuilt index "x"
Log a has valid length 52 after repair
Invalidated indexes in log 'a'
Write valid MultiMeta"#
        );

        // Index should be rebuilt (shorter).
        let index_len_after = file_size(&index_path);
        assert!(index_len_before > index_len_after);

        // The MultiLog can be opened fine.
        opts.open(path).map(|_| 1).unwrap();
    }

    #[test]
    fn test_mixed_old_new_read_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();

        let mut mlog_new = simple_open_opts().open(&path).unwrap();
        let mut logs_new = mlog_new.detach_logs();

        let mut mlog_old = {
            let mut opts = simple_open_opts();
            opts.leacy_multimeta_source = true;
            opts.open(&path).unwrap()
        };
        let mut logs_old = mlog_old.detach_logs();

        // Mixed writes from old and new mlogs.
        const N: usize = 2;
        for i in 0..N {
            for (mlog, logs, j) in [
                (&mut mlog_new, &mut logs_new, 0u8),
                (&mut mlog_old, &mut logs_old, 1u8),
            ] {
                let lock = mlog.lock().unwrap();
                logs[0].append(&[i as u8, j]).unwrap();
                logs[0].sync().unwrap();
                mlog.write_meta(&lock).unwrap();
            }
        }

        // Reading the log. It should contain N * 2 entries.
        let mlog = simple_open_opts().open(&path).unwrap();
        assert_eq!(
            mlog.logs[0].iter().map(|e| e.unwrap()).collect::<Vec<_>>(),
            [[0, 0], [0, 1], [1, 0], [1, 1]],
        );
    }

    quickcheck! {
        fn test_roundtrip_multimeta(name_len_list: Vec<(String, u64)>, version: (u64, u64)) -> bool {
            let metas = name_len_list
                .into_iter()
                .map(|(name, len)| {
                    let meta = LogMetadata::new_with_primary_len(len);
                    (name, Arc::new(Mutex::new(meta)))
                })
                .collect();
            let meta = MultiMeta { metas, version, ..Default::default() };
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
