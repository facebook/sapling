/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::ErrorKind;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic;
use std::sync::atomic::AtomicU64;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use configmodel::convert::ByteCount;
use indexedlog::OpenWithRepair;
use indexedlog::Result as IndexedlogResult;
use indexedlog::log;
use indexedlog::log::ExtendWrite;
use indexedlog::log::IndexDef;
use indexedlog::log::IndexOutput;
use indexedlog::log::Log;
use indexedlog::log::LogLookupIter;
use indexedlog::rotate;
use indexedlog::rotate::ConsistentReadGuard;
use indexedlog::rotate::RotateLog;
use indexedlog::rotate::RotateLogLookupIter;
use minibytes::Bytes;
use parking_lot::RwLock;
use parking_lot::RwLockReadGuard;
use parking_lot::RwLockUpgradableReadGuard;
use parking_lot::RwLockWriteGuard;
use tracing::debug;

/// Simple wrapper around either an `IndexedLog` or a `RotateLog`. This abstracts whether a store
/// is permanent (`IndexedLog`) or rotated (`RotateLog`) so that higher level stores don't have to deal
/// with the subtle differences.
pub struct Store {
    inner: RwLock<Inner>,
    auto_sync_count: AtomicU64,
    // Configured by scmstore.sync-logs-if-changed-on-disk (defaults to disabled if not configured).
    sync_if_changed_on_disk: bool,
    should_compress: bool,
}

pub enum Inner {
    Permanent(Log),
    Rotated(RotateLog),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreType {
    Permanent,
    Rotated,
}

impl Store {
    pub fn read(&self) -> RwLockReadGuard<'_, Inner> {
        self.sync_if_changed_on_disk()
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner.write()
    }

    pub fn is_permanent(&self) -> bool {
        self.read().is_permanent()
    }

    /// Add the buffer to the store.
    pub fn append(&self, buf: impl AsRef<[u8]>) -> Result<()> {
        self.write().append(buf)
    }

    /// Write to the store directly.
    pub fn append_direct(&self, cb: impl Fn(&mut dyn ExtendWrite) -> Result<()>) -> Result<()> {
        self.write().append_direct(cb)
    }

    /// Attempt to make slice backed by the mmap buffer to avoid heap allocation.
    pub fn slice_to_bytes(&self, slice: &[u8]) -> Bytes {
        self.read().slice_to_bytes(slice)
    }

    pub fn should_compress(&self) -> bool {
        self.should_compress
    }

    /// Append a batch of items to the store. This is optimized to reduce lock churn, which helps a
    /// lot when there is multi-threaded contention.
    pub fn append_batch<K: AsRef<[u8]> + Copy, V>(
        &self,
        mut items: Vec<(K, V)>,
        serialize: impl Fn(K, &V, &mut dyn Write) -> Result<()>,
        // Filter out items already present in the store before inserting.
        read_before_write: bool,
    ) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }

        // If requested, filter out items that are already in the store.
        if read_before_write {
            let mut insert_idx = 0;
            let log = self.read();
            for read_idx in 0..items.len() {
                if log.lookup(0, items[read_idx].0.as_ref())?.is_empty()? {
                    items.swap(insert_idx, read_idx);
                    insert_idx += 1;
                }
            }
            if insert_idx == 0 {
                return Ok(());
            }
            items.truncate(insert_idx);
        }

        let mut log = self.write();

        for (k, v) in items {
            log.append_direct(|buf| serialize(k, &v, buf))?;
        }

        Ok(())
    }

    pub fn flush(&self) -> Result<()> {
        self.write().flush()
    }

    fn sync_if_changed_on_disk(&self) -> RwLockReadGuard<'_, Inner> {
        let log = self.inner.read();

        if !self.sync_if_changed_on_disk {
            return log;
        }

        if log.is_changed_on_disk() {
            drop(log);

            let mut log = self.inner.upgradable_read();
            if log.is_changed_on_disk() {
                tracing::debug!("auto-syncing indexedlog because it changed on disk");
                self.auto_sync_count.fetch_add(1, atomic::Ordering::Relaxed);
                log.with_upgraded(|log| {
                    if let Err(err) = log.flush() {
                        tracing::warn!(?err, "error auto-syncing indexedlog store");
                    }
                })
            }

            RwLockUpgradableReadGuard::downgrade(log)
        } else {
            log
        }
    }
}

impl Inner {
    pub fn is_permanent(&self) -> bool {
        match self {
            Self::Permanent(_) => true,
            _ => false,
        }
    }

    /// Find the key in the store. Returns an `Iterator` over all the values that this store
    /// contains for the key.
    pub fn lookup(&self, index_id: usize, key: impl AsRef<[u8]>) -> Result<LookupIter<'_>> {
        let key = key.as_ref();
        match self {
            Self::Permanent(log) => Ok(LookupIter::Permanent(log.lookup(index_id, key)?)),
            Self::Rotated(log) => Ok(LookupIter::Rotated(
                log.lookup(index_id, Bytes::copy_from_slice(key))?,
            )),
        }
    }

    /// Return whether `key` exists in specified index, without reading log data.
    pub fn contains(&self, index_id: usize, key: impl AsRef<[u8]>) -> Result<bool> {
        Ok(!self.lookup(index_id, key)?.is_empty()?)
    }

    /// Add the buffer to the store.
    pub fn append(&mut self, buf: impl AsRef<[u8]>) -> Result<()> {
        match self {
            Self::Permanent(log) => Ok(log.append(buf)?),
            Self::Rotated(log) => Ok(log.append(buf)?),
        }
    }

    pub fn append_direct(&mut self, cb: impl Fn(&mut dyn ExtendWrite) -> Result<()>) -> Result<()> {
        match self {
            Self::Permanent(log) => Ok(log.append_direct(cb)?),
            Self::Rotated(log) => Ok(log.append_direct(cb)?),
        }
    }

    pub fn iter(&self) -> Box<dyn Iterator<Item = IndexedlogResult<&[u8]>> + '_> {
        match self {
            Self::Permanent(log) => Box::new(log.iter()),
            Self::Rotated(log) => Box::new(log.iter()),
        }
    }

    /// Attempt to make slice backed by the mmap buffer to avoid heap allocation.
    pub fn slice_to_bytes(&self, slice: &[u8]) -> Bytes {
        match self {
            Self::Permanent(log) => log.slice_to_bytes(slice),
            Self::Rotated(log) => log.slice_to_bytes(slice),
        }
    }

    pub fn flush(&mut self) -> Result<()> {
        match self {
            Self::Permanent(log) => {
                log.sync()?;
            }
            Self::Rotated(log) => {
                if let Err(err) = log.sync() {
                    if !err.is_corruption() && err.io_error_kind() == ErrorKind::NotFound {
                        // File-not-found errors can happen when the hg cache
                        // was blown away during command execution. Ignore the
                        // error since failed cache writes won't cause incorrect
                        // behavior and do not have to abort the command.
                        tracing::warn!(%err, "ignoring error flushing rotated indexedlog");
                    } else {
                        return Err(err.into());
                    }
                }
            }
        };
        Ok(())
    }

    fn is_changed_on_disk(&self) -> bool {
        match self {
            Self::Permanent(log) => log.is_changed_on_disk(),
            Self::Rotated(log) => log.is_changed_on_disk(),
        }
    }

    pub(crate) fn with_consistent_reads(&mut self) -> Option<ConsistentReadGuard> {
        match self {
            Inner::Permanent(_log) => None,
            Inner::Rotated(rotate_log) => Some(rotate_log.with_consistent_reads()),
        }
    }
}

/// Iterator returned from `Self::lookup`.
pub enum LookupIter<'a> {
    Permanent(LogLookupIter<'a>),
    Rotated(RotateLogLookupIter<'a>),
}

impl<'a> Iterator for LookupIter<'a> {
    type Item = Result<&'a [u8]>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            LookupIter::Permanent(iter) => iter.next().map(|res| res.map_err(Into::into)),
            LookupIter::Rotated(iter) => iter.next().map(|res| res.map_err(Into::into)),
        }
    }
}

impl<'a> LookupIter<'a> {
    pub fn is_empty(self) -> Result<bool> {
        match self {
            LookupIter::Permanent(log) => Ok(log.is_empty()),
            LookupIter::Rotated(log) => Ok(log.is_empty()?),
        }
    }
}

pub struct StoreOpenOptions {
    auto_sync_threshold: Option<u64>,
    sync_if_changed_on_disk: bool,
    pub max_log_count: Option<u8>,
    pub max_bytes_per_log: Option<u64>,
    indexes: Vec<IndexDef>,
    create: bool,
    btrfs_compression: bool,
}

impl StoreOpenOptions {
    pub fn new(config: &dyn Config) -> Self {
        Self {
            auto_sync_threshold: None,
            max_log_count: None,
            max_bytes_per_log: None,
            indexes: Vec::new(),
            create: true,
            sync_if_changed_on_disk: config
                .must_get("scmstore", "sync-logs-if-changed-on-disk")
                .unwrap_or_default(),
            btrfs_compression: false,
        }
    }

    /// Load specific configs `indexedlog.{prefix}-...`.
    pub(crate) fn load_specific_config(mut self, config: &dyn Config, prefix: &str) -> Self {
        if let Ok(Some(v)) =
            config.get_opt::<ByteCount>("indexedlog", &format!("{prefix}.auto-sync-threshold"))
        {
            self.auto_sync_threshold = Some(v.value());
        }
        if let Ok(Some(v)) =
            config.get_opt::<ByteCount>("indexedlog", &format!("{prefix}.max-bytes-per-log"))
        {
            self.max_bytes_per_log = Some(v.value());
        }
        if let Ok(Some(v)) = config.get_opt::<u8>("indexedlog", &format!("{prefix}.max-log-count"))
        {
            self.max_log_count = Some(v)
        }

        self
    }

    /// When the store is rotated, control how many logs will be kept in the `RotateLog`.
    pub fn max_log_count(mut self, count: u8) -> Self {
        self.max_log_count = Some(count);
        self
    }

    /// When the store is rotated, control how big each of the individual logs in the `RotateLog`
    /// will be.
    pub fn max_bytes_per_log(mut self, bytes: u64) -> Self {
        self.max_bytes_per_log = Some(bytes);
        self
    }

    /// Add an `IndexedLog` index function.
    pub fn index(mut self, name: &'static str, func: fn(&[u8]) -> Vec<IndexOutput>) -> Self {
        self.indexes.push(IndexDef::new(name, func));
        self
    }

    /// When the in-memory buffer exceeds `threshold`, it's automatically flushed to disk.
    pub fn auto_sync_threshold(mut self, threshold: u64) -> Self {
        self.auto_sync_threshold = Some(threshold);
        self
    }

    /// Rely on btrfs compression.
    pub fn btrfs_compression(mut self, btrfs: bool) -> Self {
        self.btrfs_compression = btrfs;
        self
    }

    pub fn create(mut self, create: bool) -> Self {
        self.create = create;
        self
    }

    fn into_permanent_open_options(self) -> log::OpenOptions {
        log::OpenOptions::new()
            .create(self.create)
            .index_defs(self.indexes)
            .auto_sync_threshold(self.auto_sync_threshold)
    }

    /// Create a permanent `Store`.
    ///
    /// Data added to the store will never be rotated out, and `fsync(2)` is used to guarantee
    /// data consistency.
    pub fn permanent(self, path: impl AsRef<Path>) -> Result<Store> {
        let sync_if_changed_on_disk = self.sync_if_changed_on_disk;
        let should_compress = self.should_compress(path.as_ref())?;
        Ok(Store {
            inner: RwLock::new(Inner::Permanent(
                self.into_permanent_open_options()
                    .btrfs_compression(!should_compress)
                    .open_with_repair(path.as_ref())?,
            )),
            auto_sync_count: AtomicU64::new(0),
            sync_if_changed_on_disk,
            should_compress,
        })
    }

    /// Convert a `StoreOpenOptions` to a `rotate::OpenOptions`.
    ///
    /// Should only be used to implement `indexedlog::DefaultOpenOptions`
    pub fn into_rotated_open_options(self) -> rotate::OpenOptions {
        let mut opts = rotate::OpenOptions::new()
            .create(self.create)
            .auto_sync_threshold(self.auto_sync_threshold)
            .index_defs(self.indexes);

        if let Some(max_log_count) = self.max_log_count {
            opts = opts.max_log_count(max_log_count);
        }

        if let Some(max_bytes_per_log) = self.max_bytes_per_log {
            opts = opts.max_bytes_per_log(max_bytes_per_log);
        }

        opts
    }

    /// Create a rotated `Store`
    ///
    /// Data added to a rotated store will be rotated out depending on the values of `max_log_count`
    /// and `max_bytes_per_log`.
    pub fn rotated(self, path: impl AsRef<Path>) -> Result<Store> {
        let sync_if_changed_on_disk = self.sync_if_changed_on_disk;
        let should_compress = self.should_compress(path.as_ref())?;
        let opts = self
            .into_rotated_open_options()
            .btrfs_compression(!should_compress);
        let mut rotate_log = opts.open_with_repair(path.as_ref())?;
        // Attempt to clean up old logs that might be left around. On Windows, other
        // Mercurial processes that have the store opened might prevent their removal.
        let res = rotate_log.remove_old_logs();
        if let Err(err) = res {
            debug!("Unable to remove old indexedlogutil logs: {:?}", err);
        }
        Ok(Store {
            inner: RwLock::new(Inner::Rotated(rotate_log)),
            auto_sync_count: AtomicU64::new(0),
            sync_if_changed_on_disk,
            should_compress,
        })
    }

    /// Attempts to repair corruption in a permanent indexedlog store.
    ///
    /// Note, this may delete data, though it should only delete data that is unreadable.
    #[allow(dead_code)]
    pub fn repair_permanent(self, path: PathBuf) -> Result<String> {
        self.into_permanent_open_options()
            .repair(path)
            .map_err(|e| e.into())
    }

    /// Attempts to repair corruption in a rotated rotatelog store.
    ///
    /// Note, this may delete data, though that should be fine since a rotatelog is free to delete
    /// data already.
    #[allow(dead_code)]
    pub fn repair_rotated(self, path: PathBuf) -> Result<String> {
        self.into_rotated_open_options()
            .repair(path)
            .map_err(|e| e.into())
    }

    fn should_compress(&self, path: &Path) -> Result<bool> {
        Ok(!self.btrfs_compression || !is_btrfs(path))
    }
}

fn is_btrfs(path: &Path) -> bool {
    if cfg!(target_os = "linux") {
        match fsinfo::fstype(path) {
            Ok(fstype) => fstype == fsinfo::FsType::BTRFS,
            Err(err) => {
                tracing::error!(?err, "error detecting filesystem type for btrfs decision");
                false
            }
        }
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_permanent() -> Result<()> {
        let dir = TempDir::new()?;

        let store = StoreOpenOptions::new(&BTreeMap::<&str, &str>::new())
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .permanent(&dir)?;

        store.append(b"aabcd")?;

        assert_eq!(
            store.read().lookup(0, b"aa")?.collect::<Result<Vec<_>>>()?,
            vec![b"aabcd"]
        );
        Ok(())
    }

    #[test]
    fn test_rotated() -> Result<()> {
        let dir = TempDir::new()?;

        let store = StoreOpenOptions::new(&BTreeMap::<&str, &str>::new())
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .rotated(&dir)?;

        store.append(b"aabcd")?;

        assert_eq!(
            store.read().lookup(0, b"aa")?.collect::<Result<Vec<_>>>()?,
            vec![b"aabcd"]
        );
        Ok(())
    }

    #[test]
    fn test_permanent_no_rotate() -> Result<()> {
        let dir = TempDir::new()?;

        let store = StoreOpenOptions::new(&BTreeMap::<&str, &str>::new())
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .max_log_count(1)
            .max_bytes_per_log(10)
            .permanent(&dir)?;

        store.append(b"aabcd")?;
        store.append(b"abbcd")?;
        store.flush()?;
        store.append(b"acbcd")?;
        store.append(b"adbcd")?;
        store.flush()?;

        assert_eq!(store.read().lookup(0, b"aa")?.count(), 1);
        Ok(())
    }

    #[test]
    fn test_rotated_rotate() -> Result<()> {
        let dir = TempDir::new()?;

        let store = StoreOpenOptions::new(&BTreeMap::<&str, &str>::new())
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .max_log_count(1)
            .max_bytes_per_log(10)
            .rotated(&dir)?;

        store.append(b"aabcd")?;
        store.append(b"abbcd")?;
        store.flush()?;
        store.append(b"acbcd")?;
        store.append(b"adbcd")?;
        store.flush()?;

        assert_eq!(store.read().lookup(0, b"aa")?.count(), 0);
        Ok(())
    }

    #[test]
    fn test_transparent_sync() -> Result<()> {
        let dir = TempDir::new()?;

        let mut config = BTreeMap::<&str, &str>::new();
        config.insert("scmstore.sync-logs-if-changed-on-disk", "true");

        let store1 = StoreOpenOptions::new(&config)
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .permanent(&dir)?;

        let store2 = StoreOpenOptions::new(&config)
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .permanent(&dir)?;

        store1.append(b"aabcd")?;
        store1.flush()?;

        // store2 sees the write immediately
        assert_eq!(
            store2
                .read()
                .lookup(0, b"aa")?
                .collect::<Result<Vec<_>>>()?,
            vec![b"aabcd"]
        );

        store2.append(b"abcd")?;
        assert_eq!(
            store2
                .read()
                .lookup(0, b"ab")?
                .collect::<Result<Vec<_>>>()?,
            vec![b"abcd"]
        );

        // Make sure we only synced once:
        assert_eq!(store2.auto_sync_count.load(atomic::Ordering::Relaxed), 1);

        Ok(())
    }

    #[test]
    fn test_transparent_sync_disabled() -> Result<()> {
        let dir = TempDir::new()?;

        let store1 = StoreOpenOptions::new(&BTreeMap::<&str, &str>::new())
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .permanent(&dir)?;

        let store2 = StoreOpenOptions::new(&BTreeMap::<&str, &str>::new())
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .permanent(&dir)?;

        store1.append(b"aabcd")?;
        store1.flush()?;

        // store2 doesn't see the write.
        assert!(
            store2
                .read()
                .lookup(0, b"aa")?
                .collect::<Result<Vec<_>>>()?
                .is_empty()
        );

        Ok(())
    }
}
