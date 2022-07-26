/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
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
use tracing::debug;

/// Simple wrapper around either an `IndexedLog` or a `RotateLog`. This abstracts whether a store
/// is local (`IndexedLog`) or shared (`RotateLog`) so that higher level stores don't have to deal
/// with the subtle differences.
pub enum Store {
    Local(Log),
    Shared(RotateLog),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreType {
    Local,
    Shared,
}

impl Store {
    pub fn is_local(&self) -> bool {
        match self {
            Store::Local(_) => true,
            _ => false,
        }
    }

    /// Find the key in the store. Returns an `Iterator` over all the values that this store
    /// contains for the key.
    pub fn lookup(&self, index_id: usize, key: impl AsRef<[u8]>) -> Result<LookupIter> {
        let key = key.as_ref();
        match self {
            Store::Local(log) => Ok(LookupIter::Local(log.lookup(index_id, key)?)),
            Store::Shared(log) => Ok(LookupIter::Shared(
                log.lookup(index_id, Bytes::copy_from_slice(key))?,
            )),
        }
    }

    /// Add the buffer to the store.
    pub fn append(&mut self, buf: impl AsRef<[u8]>) -> Result<()> {
        match self {
            Store::Local(log) => Ok(log.append(buf)?),
            Store::Shared(log) => Ok(log.append(buf)?),
        }
    }

    pub fn iter(&self) -> Box<dyn Iterator<Item = IndexedlogResult<&[u8]>> + '_> {
        match self {
            Store::Local(log) => Box::new(log.iter()),
            Store::Shared(log) => Box::new(log.iter()),
        }
    }

    /// Attempt to make slice backed by the mmap buffer to avoid heap allocation.
    pub fn slice_to_bytes(&self, slice: &[u8]) -> Bytes {
        match self {
            Store::Local(log) => log.slice_to_bytes(slice),
            Store::Shared(log) => log.slice_to_bytes(slice),
        }
    }

    pub fn flush(&mut self) -> Result<()> {
        match self {
            Store::Local(log) => {
                log.flush()?;
            }
            Store::Shared(log) => {
                log.flush()?;
            }
        };
        Ok(())
    }
}

/// Iterator returned from `Store::lookup`.
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

pub struct StoreOpenOptions {
    auto_sync_threshold: Option<u64>,
    pub max_log_count: Option<u8>,
    pub max_bytes_per_log: Option<u64>,
    indexes: Vec<IndexDef>,
    create: bool,
}

impl StoreOpenOptions {
    pub fn new() -> Self {
        Self {
            auto_sync_threshold: None,
            max_log_count: None,
            max_bytes_per_log: None,
            indexes: Vec::new(),
            create: true,
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
        Ok(Store::Local(
            self.into_local_open_options()
                .open_with_repair(path.as_ref())?,
        ))
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
        let opts = self.into_shared_open_options();
        let mut rotate_log = opts.open_with_repair(path.as_ref())?;
        // Attempt to clean up old logs that might be left around. On Windows, other
        // Mercurial processes that have the store opened might prevent their removal.
        let res = rotate_log.remove_old_logs();
        if let Err(err) = res {
            debug!("Unable to remove old indexedlogutil logs: {:?}", err);
        }
        Ok(Store::Shared(rotate_log))
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
    use tempfile::TempDir;

    use super::*;

    #[test]
    fn test_local() -> Result<()> {
        let dir = TempDir::new()?;

        let mut store = StoreOpenOptions::new()
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .local(&dir)?;

        store.append(b"aabcd")?;

        assert_eq!(
            store.lookup(0, b"aa")?.collect::<Result<Vec<_>>>()?,
            vec![b"aabcd"]
        );
        Ok(())
    }

    #[test]
    fn test_shared() -> Result<()> {
        let dir = TempDir::new()?;

        let mut store = StoreOpenOptions::new()
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .shared(&dir)?;

        store.append(b"aabcd")?;

        assert_eq!(
            store.lookup(0, b"aa")?.collect::<Result<Vec<_>>>()?,
            vec![b"aabcd"]
        );
        Ok(())
    }

    #[test]
    fn test_local_no_rotate() -> Result<()> {
        let dir = TempDir::new()?;

        let mut store = StoreOpenOptions::new()
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

        assert_eq!(store.lookup(0, b"aa")?.count(), 1);
        Ok(())
    }

    #[test]
    fn test_shared_rotate() -> Result<()> {
        let dir = TempDir::new()?;

        let mut store = StoreOpenOptions::new()
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

        assert_eq!(store.lookup(0, b"aa")?.count(), 0);
        Ok(())
    }
}
