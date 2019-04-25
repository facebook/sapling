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
