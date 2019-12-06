/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # idmap
//!
//! See [`IdMap`] for the main structure.

use crate::id::{GroupId, Id};
use anyhow::{bail, ensure, format_err, Result};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use fs2::FileExt;
use indexedlog::log;
use std::fs::{self, File};
use std::io::{Cursor, Write};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{self, AtomicU64};

/// Bi-directional mapping between an integer id and `[u8]`.
pub struct IdMap {
    log: log::Log,
    path: PathBuf,
    cached_next_free_ids: [AtomicU64; GroupId::MAX],
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
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
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
        Ok(Self {
            log,
            path,
            cached_next_free_ids: Default::default(),
        })
    }

    /// Return a [`SyncableIdMap`] instance that provides race-free
    /// filesytem read and write access by taking an exclusive lock.
    ///
    /// The [`SyncableIdMap`] instance provides a `sync` method that
    /// actually writes changes to disk.
    ///
    /// Block if another instance is taking the lock.
    pub fn prepare_filesystem_sync(&mut self) -> Result<SyncableIdMap> {
        ensure!(
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
        self.reload()?;

        Ok(SyncableIdMap {
            map: self,
            lock_file,
        })
    }

    /// Reload from the filesystem. Discard pending changes.
    pub fn reload(&mut self) -> Result<()> {
        self.log.clear_dirty()?;
        self.log.sync()?;
        // Invalidate the next free id cache.
        self.cached_next_free_ids = Default::default();
        Ok(())
    }

    /// Find slice by a specified integer id.
    pub fn find_slice_by_id(&self, id: Id) -> Result<Option<&[u8]>> {
        let mut key = Vec::with_capacity(8);
        key.write_u64::<BigEndian>(id.0).unwrap();
        let key = self.log.lookup(Self::INDEX_ID_TO_SLICE, key)?.nth(0);
        match key {
            Some(Ok(entry)) => {
                ensure!(entry.len() >= 8, "index key should have 8 bytes at least");
                Ok(Some(&entry[8..]))
            }
            None => Ok(None),
            Some(Err(err)) => Err(err.into()),
        }
    }

    /// Find the integer id matching the given slice.
    pub fn find_id_by_slice(&self, slice: &[u8]) -> Result<Option<Id>> {
        let key = self.log.lookup(Self::INDEX_SLICE_TO_ID, slice)?.nth(0);
        match key {
            Some(Ok(mut entry)) => {
                ensure!(entry.len() >= 8, "index key should have 8 bytes at least");
                Ok(Some(Id(entry.read_u64::<BigEndian>().unwrap())))
            }
            None => Ok(None),
            Some(Err(err)) => Err(err.into()),
        }
    }

    /// Similar to `find_slice_by_id`, but returns None if group > `max_group`.
    pub fn find_id_by_slice_with_max_group(
        &self,
        slice: &[u8],
        max_group: GroupId,
    ) -> Result<Option<Id>> {
        Ok(self.find_id_by_slice(slice)?.and_then(|id| {
            if id.group_id() <= max_group {
                Some(id)
            } else {
                None
            }
        }))
    }

    /// Insert a new entry mapping from a slice to an id.
    ///
    /// Errors if the new entry conflicts with existing entries.
    pub fn insert(&mut self, id: Id, slice: &[u8]) -> Result<()> {
        let group = id.group_id();
        if id < self.next_free_id(group)? {
            let existing_slice = self.find_slice_by_id(id)?;
            if let Some(existing_slice) = existing_slice {
                if existing_slice != slice {
                    bail!(
                        "logic error: new entry {} = {:?} conflicts with an existing entry {} = {:?}",
                        id,
                        slice,
                        id,
                        existing_slice
                    );
                }
            }
        }
        let existing_id = self.find_id_by_slice(slice)?;
        if let Some(existing_id) = existing_id {
            // Allow re-assigning Ids from a higher group to a lower group.
            // For example, when a non-master commit gets merged into the
            // master branch, the id is re-assigned to master. But, the
            // ids in the master group will never be re-assigned to
            // non-master groups.
            if existing_id != id && existing_id.group_id() <= group {
                bail!(
                    "logic error: new entry {} = {:?} conflicts with an existing entry {} = {:?}",
                    id,
                    slice,
                    existing_id,
                    slice
                );
            }
        }

        let mut data = Vec::with_capacity(8 + slice.len());
        data.write_u64::<BigEndian>(id.0).unwrap();
        data.write_all(slice).unwrap();
        self.log.append(data)?;
        let next_free_id = self.cached_next_free_ids[group.0].get_mut();
        if id.0 >= *next_free_id {
            *next_free_id = id.0 + 1;
        }
        Ok(())
    }

    /// Return the next unused id in the given group.
    pub fn next_free_id(&self, group: GroupId) -> Result<Id> {
        let cached = self.cached_next_free_ids[group.0].load(atomic::Ordering::SeqCst);
        let id = if cached == 0 {
            let id = Self::get_next_free_id(&self.log, group)?;
            self.cached_next_free_ids[group.0].store(id.0, atomic::Ordering::SeqCst);
            id
        } else {
            Id(cached)
        };
        Ok(id)
    }

    // Find an unused id that is bigger than existing ids.
    // Used internally. It should match `next_free_id`.
    fn get_next_free_id(log: &log::Log, group: GroupId) -> Result<Id> {
        // Checks should have been done at callsite.
        let lower_bound_id = group.min_id();
        let upper_bound_id = group.max_id();
        let lower_bound = lower_bound_id.to_bytearray();
        let upper_bound = upper_bound_id.to_bytearray();
        let range = &lower_bound[..]..=&upper_bound[..];
        let mut iter = log.lookup_range(Self::INDEX_ID_TO_SLICE, range)?.rev();
        let id = match iter.nth(0) {
            None => lower_bound_id,
            Some(Ok((key, _))) => Id(Cursor::new(key).read_u64::<BigEndian>()? + 1),
            _ => bail!("cannot read next_free_id for group {}", group),
        };
        debug_assert!(id >= lower_bound_id);
        debug_assert!(id <= upper_bound_id);
        Ok(id)
    }
}

// Interaction with a DAG.
impl IdMap {
    /// Assign an id for a head in a DAG. This implies ancestors of the
    /// head will also have ids assigned.
    ///
    /// This function is incremental. If the head or any of its ancestors
    /// already have an id stored in this map, the existing ids will be
    /// reused.
    ///
    /// This function needs roughly `O(N)` heap memory. `N` is the number of
    /// ids to assign. When `N` is very large, try assigning ids to a known
    /// ancestor first.
    ///
    /// New `id`s inserted by this function will have the specified `group`.
    /// Existing `id`s that are ancestors of `head` will get re-assigned
    /// if they have a higher `group`.
    pub fn assign_head<F>(&mut self, head: &[u8], parents_by_name: &F, group: GroupId) -> Result<Id>
    where
        F: Fn(&[u8]) -> Result<Vec<Box<[u8]>>>,
    {
        // There are some interesting cases to optimize the numbers:
        //
        // C     For a merge C, it has choice to assign numbers to A or B
        // |\    first (A and B are abstract branches that have many nodes).
        // A B   Suppose branch A is linear and B have merges, and D is
        // |/    (::A & ::B). Then:
        // D
        //
        // - If `D` is empty or already assigned, it's better to assign A last.
        //   This is because (A+C) can then always form a segment regardless of
        //   the complexity of B:
        //
        //      B   A   C       vs.        A   B   C
        //     ~~~  ^^^^^                     ~~~
        //     xxxxxx                          *****
        //                                 xxxxx
        //
        //   [~]: Might be complex (ex. many segments)
        //   [^]: Can always form a segment. (better)
        //   [*]: Can only be a segment if segment size is large enough.
        //   [x]: Cannot form a segment.
        //
        // - If `D` is not empty (and not assigned), it _might_ be better to
        //   assign D and A first. This provides benefits for A and D to be
        //   continuous, with the downside that A and C are not continuous.
        //
        //   A typical pattern is one branch continuously merges into the other
        //   (see also segmented-changelog.pdf, page 19):
        //
        //        B---D---F
        //         \   \   \
        //      A---C---E---G
        //
        // The code below is optimized for cases where p1 branch is linear,
        // but p2 branch is not.

        // Emulate the stack in heap to avoid overflow.
        enum Todo {
            /// Visit parents. Finally assign self.
            Visit(Box<[u8]>),

            /// Assign a number if not assigned. Parents are visited.
            Assign(Box<[u8]>),
        }
        use Todo::{Assign, Visit};

        let mut todo_stack: Vec<Todo> = vec![Visit(head.to_vec().into_boxed_slice())];
        while let Some(todo) = todo_stack.pop() {
            match todo {
                Visit(head) => {
                    // If the id was not assigned, or was assigned to a higher group,
                    // (re-)assign it to this group.
                    if let None = self.find_id_by_slice_with_max_group(&head, group)? {
                        todo_stack.push(Todo::Assign(head.clone()));
                        // If the parent was not assigned, or was assigned to a higher group,
                        // (re-)assign the parent to this group.
                        for unassigned_parent in parents_by_name(&head)?
                            .into_iter()
                            .filter(|p| match self.find_id_by_slice_with_max_group(p, group) {
                                Ok(Some(_)) => false,
                                _ => true,
                            })
                            // "rev" is the "optimization"
                            .rev()
                        {
                            todo_stack.push(Todo::Visit(unassigned_parent));
                        }
                    }
                }
                Assign(head) => {
                    if let None = self.find_id_by_slice_with_max_group(&head, group)? {
                        let id = self.next_free_id(group)?;
                        self.insert(id, &head)?;
                    }
                }
            }
        }

        self.find_id_by_slice(head)
            .map(|v| v.expect("head should be assigned now"))
    }

    /// Translate `get_parents` from taking slices to taking `Id`s.
    pub fn build_get_parents_by_id<'a>(
        &'a self,
        get_parents_by_name: &'a dyn Fn(&[u8]) -> Result<Vec<Box<[u8]>>>,
    ) -> impl Fn(Id) -> Result<Vec<Id>> + 'a {
        let func = move |id: Id| -> Result<Vec<Id>> {
            let name = match self.find_slice_by_id(id)? {
                Some(name) => name,
                None => {
                    let name = match self.find_slice_by_id(id) {
                        Ok(Some(name)) => format!("{} ({:?})", id, name),
                        _ => format!("{}", id),
                    };
                    bail!("logic error: {} is referred but not assigned", name)
                }
            };
            let parent_names = get_parents_by_name(&name)?;
            let mut result = Vec::with_capacity(parent_names.len());
            for parent_name in parent_names {
                if let Some(parent_id) = self.find_id_by_slice(&parent_name)? {
                    ensure!(
                        parent_id < id,
                        "parent {} {:?} should <= {} {:?}",
                        parent_id,
                        &parent_name,
                        id,
                        &name
                    );
                    result.push(parent_id);
                } else {
                    bail!("logic error: ancestor ids must be available");
                }
            }
            Ok(result)
        };
        func
    }
}

impl<'a> SyncableIdMap<'a> {
    /// Write pending changes to disk.
    pub fn sync(&mut self) -> Result<()> {
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

/// Minimal APIs for converting between Id and slice.
pub trait IdMapLike {
    fn id(&self, slice: &[u8]) -> Result<Id>;
    fn slice(&self, id: Id) -> Result<Box<[u8]>>;
}

impl IdMapLike for IdMap {
    fn id(&self, slice: &[u8]) -> Result<Id> {
        self.find_id_by_slice(slice)?
            .ok_or_else(|| format_err!("{:?} not found", slice))
    }
    fn slice(&self, id: Id) -> Result<Box<[u8]>> {
        Ok(self
            .find_slice_by_id(id)?
            .ok_or_else(|| format_err!("{} not found", id))?
            .to_vec()
            .into_boxed_slice())
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
        assert_eq!(map.next_free_id(GroupId::MASTER).unwrap().0, 0);
        map.insert(Id(1), b"abc").unwrap();
        assert_eq!(map.next_free_id(GroupId::MASTER).unwrap().0, 2);
        map.insert(Id(2), b"def").unwrap();
        assert_eq!(map.next_free_id(GroupId::MASTER).unwrap().0, 3);
        map.insert(Id(10), b"ghi").unwrap();
        assert_eq!(map.next_free_id(GroupId::MASTER).unwrap().0, 11);
        map.insert(Id(11), b"ghi").unwrap_err(); // ghi maps to 10
        map.insert(Id(10), b"ghi2").unwrap_err(); // 10 maps to ghi

        // Test another group.
        let id = map.next_free_id(GroupId::NON_MASTER).unwrap();
        map.insert(id, b"jkl").unwrap();
        map.insert(id, b"jkl").unwrap();
        map.insert(id, b"jkl2").unwrap_err(); // id maps to jkl
        map.insert(id + 1, b"jkl2").unwrap();
        map.insert(id + 2, b"jkl2").unwrap_err(); // jkl2 maps to id + 1
        map.insert(Id(15), b"jkl2").unwrap(); // reassign jkl2 to master group - ok.
        map.insert(id + 3, b"abc").unwrap_err(); // reassign abc to non-master group - error.
        assert_eq!(map.next_free_id(GroupId::NON_MASTER).unwrap(), id + 2);

        for _ in 0..=1 {
            assert_eq!(map.find_slice_by_id(Id(1)).unwrap().unwrap(), b"abc");
            assert_eq!(map.find_slice_by_id(Id(2)).unwrap().unwrap(), b"def");
            assert!(map.find_slice_by_id(Id(3)).unwrap().is_none());
            assert_eq!(map.find_slice_by_id(Id(10)).unwrap().unwrap(), b"ghi");

            assert_eq!(map.find_id_by_slice(b"abc").unwrap().unwrap().0, 1);
            assert_eq!(map.find_id_by_slice(b"def").unwrap().unwrap().0, 2);
            assert_eq!(map.find_id_by_slice(b"ghi").unwrap().unwrap().0, 10);
            assert_eq!(map.find_id_by_slice(b"jkl").unwrap().unwrap(), id);
            assert_eq!(map.find_id_by_slice(b"jkl2").unwrap().unwrap().0, 15);
            assert!(map.find_id_by_slice(b"jkl3").unwrap().is_none());
            map.sync().unwrap();
        }
    }
}
