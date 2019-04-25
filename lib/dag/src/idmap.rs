// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! # idmap
//!
//! See [`IdMap`] for the main structure.

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use failure::{bail, ensure, Fallible};
use fs2::FileExt;
use indexedlog::log;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{Cursor, Write};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};

pub type Id = u64;

/// Bi-directional mapping between an integer id and `[u8]`.
pub struct IdMap {
    log: log::Log,
    path: PathBuf,
    next_free_id: Id,
}

/// Guard to make sure [`IdMap`] on-disk writes are race-free.
///
/// Constructing this struct will take a filesystem lock and reload
/// the content from the filesystem. Dropping this struct will write
/// down changes to the filesystem and release the lock.
pub struct SyncableIdMap<'a> {
    map: &'a mut IdMap,
    lock_file: File,
}

impl IdMap {
    const INDEX_ID_TO_SLICE: usize = 0;
    const INDEX_SLICE_TO_ID: usize = 1;

    /// Create an [`IdMap`] backed by the given directory.
    ///
    /// By default, only read-only operations are allowed. For writing
    /// access, call [`IdMap::make_writable`] to get a writable instance.
    pub fn open(path: impl AsRef<Path>) -> Fallible<Self> {
        let path = path.as_ref();
        let log = log::OpenOptions::new()
            .create(true)
            .index("id", |_| vec![log::IndexOutput::Reference(0..8)])
            .index("slice", |data| {
                vec![log::IndexOutput::Reference(8..data.len() as u64)]
            })
            .flush_filter(Some(|_, _| {
                panic!("programming error: idmap changed by other process")
            }))
            .open(path)?;
        let path = path.to_path_buf();
        let next_free_id = Self::get_next_free_id(&log)?;
        Ok(Self {
            log,
            path,
            next_free_id,
        })
    }

    /// Return a [`SyncableIdMap`] instance that provides race-free
    /// filesytem read and write access by taking an exclusive lock.
    ///
    /// The [`SyncableIdMap`] instance provides a `sync` method that
    /// actually writes changes to disk.
    ///
    /// Block if another instance is taking the lock.
    ///
    /// Panic if there are pending in-memory writes.
    pub fn prepare_filesystem_sync(&mut self) -> Fallible<SyncableIdMap> {
        assert!(
            self.log.iter_dirty().next().is_none(),
            "programming error: prepare_filesystem_sync must be called without dirty in-memory entries",
        );

        // Take a filesystem lock. The file name 'lock' is taken by indexedlog
        // running on Windows, so we choose another file name here.
        let lock_file = {
            let mut path = self.path.clone();
            path.push("wlock");
            File::open(&path).or_else(|_| {
                fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&path)
            })?
        };
        lock_file.lock_exclusive()?;

        // Reload. So we get latest data.
        self.log.sync()?;
        self.next_free_id = Self::get_next_free_id(&self.log)?;

        Ok(SyncableIdMap {
            map: self,
            lock_file,
        })
    }

    /// Find slice by a specified integer id.
    pub fn find_slice_by_id(&self, id: Id) -> Fallible<Option<&[u8]>> {
        let mut key = Vec::with_capacity(8);
        key.write_u64::<BigEndian>(id).unwrap();
        let key = self.log.lookup(Self::INDEX_ID_TO_SLICE, key)?.nth(0);
        match key {
            Some(Ok(entry)) => {
                ensure!(entry.len() >= 8, "index key should have 8 bytes at least");
                Ok(Some(&entry[8..]))
            }
            None => Ok(None),
            Some(Err(err)) => Err(err),
        }
    }

    /// Find the integer id matching the given slice.
    pub fn find_id_by_slice(&self, slice: &[u8]) -> Fallible<Option<Id>> {
        let key = self.log.lookup(Self::INDEX_SLICE_TO_ID, slice)?.nth(0);
        match key {
            Some(Ok(mut entry)) => {
                ensure!(entry.len() >= 8, "index key should have 8 bytes at least");
                Ok(Some(entry.read_u64::<BigEndian>().unwrap()))
            }
            None => Ok(None),
            Some(Err(err)) => Err(err),
        }
    }

    /// Insert a new entry mapping from a slice to an id.
    ///
    /// Panic if the new entry conflicts with existing entries.
    pub fn insert(&mut self, id: Id, slice: &[u8]) -> Fallible<()> {
        if id < self.next_free_id {
            let existing_slice = self.find_slice_by_id(id)?;
            if let Some(existing_slice) = existing_slice {
                assert_eq!(
                    existing_slice, slice,
                    "logic error: new entry conflicts with an existing entry"
                );
            }
        }
        let existing_id = self.find_id_by_slice(slice)?;
        if let Some(existing_id) = existing_id {
            assert_eq!(
                existing_id, id,
                "logic error: new entry conflicts with an existing entry"
            );
        }

        let mut data = Vec::with_capacity(8 + slice.len());
        data.write_u64::<BigEndian>(id).unwrap();
        data.write_all(slice).unwrap();
        self.log.append(data)?;
        if id >= self.next_free_id {
            self.next_free_id = id + 1;
        }
        Ok(())
    }

    /// Return the next unused id.
    pub fn next_free_id(&self) -> Id {
        self.next_free_id
    }

    // Find an unused id that is bigger than existing ids.
    // Used internally. It should match `next_free_id`.
    fn get_next_free_id(log: &log::Log) -> Fallible<Id> {
        let mut iter = log.lookup_range(Self::INDEX_ID_TO_SLICE, ..)?.rev();
        match iter.nth(0) {
            None => Ok(0),
            Some(Ok((key, _))) => Ok(Cursor::new(key).read_u64::<BigEndian>()? + 1),
            _ => bail!("cannot read next_free_id"),
        }
    }
}

impl<'a> SyncableIdMap<'a> {
    /// Write pending changes to disk.
    ///
    /// This method must be called if there are new entries inserted.
    /// Otherwise [`SyncableIdMap`] will panic once it gets dropped.
    pub fn sync(&mut self) -> Fallible<()> {
        self.map.log.sync()?;
        Ok(())
    }
}

impl<'a> Deref for SyncableIdMap<'a> {
    type Target = IdMap;

    fn deref(&self) -> &Self::Target {
        self.map
    }
}

impl<'a> DerefMut for SyncableIdMap<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.map
    }
}

impl<'a> Drop for SyncableIdMap<'a> {
    fn drop(&mut self) {
        // TODO: handles `sync` failures gracefully.
        assert!(
            self.map.log.iter_dirty().next().is_none(),
            "programming error: sync must be called before dropping WritableIdMap"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_simple_lookups() {
        let dir = tempdir().unwrap();
        let mut map = IdMap::open(dir.path()).unwrap();
        let mut map = map.prepare_filesystem_sync().unwrap();
        assert_eq!(map.next_free_id(), 0);
        map.insert(1, b"abc").unwrap();
        assert_eq!(map.next_free_id(), 2);
        map.insert(2, b"def").unwrap();
        assert_eq!(map.next_free_id(), 3);
        map.insert(10, b"ghi").unwrap();
        assert_eq!(map.next_free_id(), 11);

        for _ in 0..=1 {
            assert_eq!(map.find_slice_by_id(1).unwrap().unwrap(), b"abc");
            assert_eq!(map.find_slice_by_id(2).unwrap().unwrap(), b"def");
            assert!(map.find_slice_by_id(3).unwrap().is_none());
            assert_eq!(map.find_slice_by_id(10).unwrap().unwrap(), b"ghi");

            assert_eq!(map.find_id_by_slice(b"abc").unwrap().unwrap(), 1);
            assert_eq!(map.find_id_by_slice(b"def").unwrap().unwrap(), 2);
            assert_eq!(map.find_id_by_slice(b"ghi").unwrap().unwrap(), 10);
            assert!(map.find_id_by_slice(b"jkl").unwrap().is_none());
            map.sync().unwrap();
        }
    }

    #[test]
    #[should_panic]
    fn test_panic_with_dirty_changes() {
        let dir = tempdir().unwrap();
        let mut map = IdMap::open(dir.path()).unwrap();
        let mut map = map.prepare_filesystem_sync().unwrap();
        map.insert(0, b"abc").unwrap();
    }
}
