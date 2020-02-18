/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use anyhow::Result;

use indexedlog::{
    log::{self, IndexDef, IndexOutput, Log, LogLookupIter},
    rotate::{self, RotateLog, RotateLogLookupIter},
};
use minibytes::Bytes;

/// Simple wrapper around either an `IndexedLog` or a `RotateLog`. This abstracts whether a store
/// is local (`IndexedLog`) or shared (`RotateLog`) so that higher level stores don't have to deal
/// with the subtle differences.
pub enum Store {
    Local(Log),
    Shared(RotateLog),
}

impl Store {
    /// Find the key in the store. Returns an `Iterator` over all the values that this store
    /// contains for the key.
    pub fn lookup(&self, key: impl AsRef<[u8]>) -> Result<LookupIter> {
        let key = key.as_ref();
        match self {
            Store::Local(log) => Ok(LookupIter::Local(log.lookup(0, key)?)),
            Store::Shared(log) => Ok(LookupIter::Shared(
                log.lookup(0, Bytes::copy_from_slice(key))?,
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
    max_log_count: Option<u8>,
    max_bytes_per_log: Option<u64>,
    indexes: Vec<IndexDef>,
}

impl StoreOpenOptions {
    pub fn new() -> Self {
        Self {
            max_log_count: None,
            max_bytes_per_log: None,
            indexes: Vec::new(),
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

    /// Create a local `Store`.
    ///
    /// Data added to a local store will never be rotated out, and `fsync(2)` is used to guarantee
    /// data consistency.
    pub fn local(self, path: impl AsRef<Path>) -> Result<Store> {
        Ok(Store::Local(
            log::OpenOptions::new()
                .create(true)
                .fsync(true)
                .index_defs(self.indexes)
                .open(path.as_ref())?,
        ))
    }

    /// Create a shared `Store`
    ///
    /// Data added to a shared store will be rotated out depending on the values of `max_log_count`
    /// and `max_bytes_per_log`.
    pub fn shared(self, path: impl AsRef<Path>) -> Result<Store> {
        let mut opts = rotate::OpenOptions::new()
            .create(true)
            .index_defs(self.indexes);

        if let Some(max_log_count) = self.max_log_count {
            opts = opts.max_log_count(max_log_count);
        }

        if let Some(max_bytes_per_log) = self.max_bytes_per_log {
            opts = opts.max_bytes_per_log(max_bytes_per_log);
        }

        Ok(Store::Shared(opts.open(path.as_ref())?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    #[test]
    fn test_local() -> Result<()> {
        let dir = TempDir::new()?;

        let mut store = StoreOpenOptions::new()
            .index("hex", |_| vec![IndexOutput::Reference(0..2)])
            .local(&dir)?;

        store.append(b"aabcd")?;

        assert_eq!(
            store.lookup(b"aa")?.collect::<Result<Vec<_>>>()?,
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
            store.lookup(b"aa")?.collect::<Result<Vec<_>>>()?,
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

        assert_eq!(store.lookup(b"aa")?.count(), 1);
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

        assert_eq!(store.lookup(b"aa")?.count(), 0);
        Ok(())
    }
}
