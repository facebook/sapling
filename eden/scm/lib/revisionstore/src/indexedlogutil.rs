/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::io::ErrorKind;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic;
use std::sync::atomic::AtomicU64;

use anyhow::Result;
use configmodel::Config;
use configmodel::ConfigExt;
use indexedlog::log;
use indexedlog::log::IndexDef;
use indexedlog::log::IndexOutput;
use indexedlog::log::Log;
use indexedlog::log::LogLookupIter;
use indexedlog::rotate;
use indexedlog::rotate::RotateLog;
use indexedlog::rotate::RotateLogLookupIter;
use indexedlog::OpenWithRepair;
use indexedlog::Result as IndexedlogResult;
use minibytes::Bytes;
use parking_lot::RwLock;
use parking_lot::RwLockReadGuard;
use parking_lot::RwLockUpgradableReadGuard;
use parking_lot::RwLockWriteGuard;
use tracing::debug;

/// Simple wrapper around either an `IndexedLog` or a `RotateLog`. This abstracts whether a store
/// is local (`IndexedLog`) or shared (`RotateLog`) so that higher level stores don't have to deal
/// with the subtle differences.
pub struct Store {
    inner: RwLock<Inner>,
    auto_sync_count: AtomicU64,
    // Configured by scmstore.sync-logs-if-changed-on-disk (defaults to disabled if not configured).
    sync_if_changed_on_disk: bool,
}

pub enum Inner {
    Local(Log),
    Shared(RotateLog),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreType {
    Local,
    Shared,
}

impl Store {
    pub fn read(&self) -> RwLockReadGuard<'_, Inner> {
        self.sync_if_changed_on_disk()
    }

    pub fn write(&self) -> RwLockWriteGuard<'_, Inner> {
        self.inner.write()
    }

    pub fn is_local(&self) -> bool {
        self.read().is_local()
    }

    /// Add the buffer to the store.
    pub fn append(&self, buf: impl AsRef<[u8]>) -> Result<()> {
        self.write().append(buf)
    }

    /// Attempt to make slice backed by the mmap buffer to avoid heap allocation.
    pub fn slice_to_bytes(&self, slice: &[u8]) -> Bytes {
        self.read().slice_to_bytes(slice)
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
    pub fn is_local(&self) -> bool {
        match self {
            Self::Local(_) => true,
            _ => false,
        }
    }

    /// Find the key in the store. Returns an `Iterator` over all the values that this store
    /// contains for the key.
    pub fn lookup(&self, index_id: usize, key: impl AsRef<[u8]>) -> Result<LookupIter> {
        let key = key.as_ref();
        match self {
            Self::Local(log) => Ok(LookupIter::Local(log.lookup(index_id, key)?)),
            Self::Shared(log) => Ok(LookupIter::Shared(
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
            Self::Local(log) => Ok(log.append(buf)?),
            Self::Shared(log) => Ok(log.append(buf)?),
        }
    }

    pub fn iter(&self) -> Box<dyn Iterator<Item = IndexedlogResult<&[u8]>> + '_> {
        match self {
            Self::Local(log) => Box::new(log.iter()),
            Self::Shared(log) => Box::new(log.iter()),
        }
    }

    /// Attempt to make slice backed by the mmap buffer to avoid heap allocation.
    pub fn slice_to_bytes(&self, slice: &[u8]) -> Bytes {
        match self {
            Self::Local(log) => log.slice_to_bytes(slice),
            Self::Shared(log) => log.slice_to_bytes(slice),
        }
    }

    pub fn flush(&mut self) -> Result<()> {
        match self {
            Self::Local(log) => {
                log.sync()?;
            }
            Self::Shared(log) => {
                if let Err(err) = log.sync() {
                    if !err.is_corruption() && err.io_error_kind() == ErrorKind::NotFound {
                        // File-not-found errors can happen when the hg cache
                        // was blown away during command execution. Ignore the
                        // error since failed cache writes won't cause incorrect
                        // behavior and do not have to abort the command.
                        tracing::warn!(%err, "ignoring error flushing shared indexedlog");
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
            Self::Local(log) => log.is_changed_on_disk(),
            Self::Shared(log) => log.is_changed_on_disk(),
        }
    }
}

/// Iterator returned from `Self::lookup`.
pub enum LookupIter<'a> {
    Local(LogLookupIter<'a>),
    Shared(RotateLogLookupIter<'a>),
}

impl<'a> Iterator for LookupIter<'a> {
    type Item = Result<&'a [u8]>;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            LookupIter::Local(iter) => iter.next().map(|res| res.map_err(Into::into)),
            LookupIter::Shared(iter) => iter.next().map(|res| res.map_err(Into::into)),
        }
    }
}

impl<'a> LookupIter<'a> {
    pub fn is_empty(self) -> Result<bool> {
        match self {
            LookupIter::Local(log) => Ok(log.is_empty()),
            LookupIter::Shared(log) => Ok(log.is_empty()?),
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
        }
    }

    /// When the store is local, control how many logs will be kept in the `RotateLog`.
    pub fn max_log_count(mut self, count: u8) -> Self {
        self.max_log_count = Some(count);
        self
    }

    /// When the store is local, control how big each of the individual logs in the `RotateLog`
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

    pub fn create(mut self, create: bool) -> Self {
        self.create = create;
        self
    }

    fn into_local_open_options(self) -> log::OpenOptions {
        log::OpenOptions::new()
            .create(self.create)
            .index_defs(self.indexes)
            .auto_sync_threshold(self.auto_sync_threshold)
    }

    /// Create a local `Store`.
    ///
    /// Data added to a local store will never be rotated out, and `fsync(2)` is used to guarantee
    /// data consistency.
    pub fn local(self, path: impl AsRef<Path>) -> Result<Store> {
        let sync_if_changed_on_disk = self.sync_if_changed_on_disk;
        Ok(Store {
            inner: RwLock::new(Inner::Local(
                self.into_local_open_options()
                    .open_with_repair(path.as_ref())?,
            )),
            auto_sync_count: AtomicU64::new(0),
            sync_if_changed_on_disk,
        })
    }

    /// Convert a `StoreOpenOptions` to a `rotate::OpenOptions`.
    ///
    /// Should only be used to implement `indexedlog::DefaultOpenOptions`
    pub fn into_shared_open_options(self) -> rotate::OpenOptions {
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

    /// Create a shared `Store`
    ///
    /// Data added to a shared store will be rotated out depending on the values of `max_log_count`
    /// and `max_bytes_per_log`.
    pub fn shared(self, path: impl AsRef<Path>) -> Result<Store> {
        let sync_if_changed_on_disk = self.sync_if_changed_on_disk;
        let opts = self.into_shared_open_options();
        let mut rotate_log = opts.open_with_repair(path.as_ref())?;
        // Attempt to clean up old logs that might be left around. On Windows, other
        // Mercurial processes that have the store opened might prevent their removal.
        let res = rotate_log.remove_old_logs();
        if let Err(err) = res {
            debug!("Unable to remove old indexedlogutil logs: {:?}", err);
        }
        Ok(Store {
            inner: RwLock::new(Inner::Shared(rotate_log)),
            auto_sync_count: AtomicU64::new(0),
            sync_if_changed_on_disk,
        })
    }

    /// Attempts to repair corruption in a local indexedlog store.
    ///
    /// Note, this may delete data, though it should only delete data that is unreadable.
    #[allow(dead_code)]
    pub fn repair_local(self, path: PathBuf) -> Result<String> {
        self.into_local_open_options()
            .repair(path)
            .map_err(|e| e.into())
    }

    /// Attempts to repair corruption in a shared rotatelog store.
    ///
    /// Note, this may delete data, though that should be fine since a rotatelog is free to delete
    /// data already.
    #[allow(dead_code)]
    pub fn repair_shared(self, path: PathBuf) -> Result<String> {
        self.into_shared_open_options()
            .repair(path)
            .map_err(|e| e.into())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_local() -> Result<()> {
        let dir = TempDir::new()?;

        let store = StoreOpenOptions::new(&BTreeMap::<&str, &str>::new())
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .local(&dir)?;

        store.append(b"aabcd")?;

        assert_eq!(
            store.read().lookup(0, b"aa")?.collect::<Result<Vec<_>>>()?,
            vec![b"aabcd"]
        );
        Ok(())
    }

    #[test]
    fn test_shared() -> Result<()> {
        let dir = TempDir::new()?;

        let store = StoreOpenOptions::new(&BTreeMap::<&str, &str>::new())
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .shared(&dir)?;

        store.append(b"aabcd")?;

        assert_eq!(
            store.read().lookup(0, b"aa")?.collect::<Result<Vec<_>>>()?,
            vec![b"aabcd"]
        );
        Ok(())
    }

    #[test]
    fn test_local_no_rotate() -> Result<()> {
        let dir = TempDir::new()?;

        let store = StoreOpenOptions::new(&BTreeMap::<&str, &str>::new())
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .max_log_count(1)
            .max_bytes_per_log(10)
            .local(&dir)?;

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
    fn test_shared_rotate() -> Result<()> {
        let dir = TempDir::new()?;

        let store = StoreOpenOptions::new(&BTreeMap::<&str, &str>::new())
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .max_log_count(1)
            .max_bytes_per_log(10)
            .shared(&dir)?;

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
            .local(&dir)?;

        let store2 = StoreOpenOptions::new(&config)
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .local(&dir)?;

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
            .local(&dir)?;

        let store2 = StoreOpenOptions::new(&BTreeMap::<&str, &str>::new())
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .local(&dir)?;

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
