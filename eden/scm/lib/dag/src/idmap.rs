/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # idmap
//!
//! See [`IdMap`] for the main structure.

use crate::errors::bug;
use crate::errors::programming;
use crate::id::{Group, Id, VertexName};
use crate::ops::IdConvert;
use crate::ops::PrefixLookup;
use crate::Result;
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use fs2::FileExt;
use indexedlog::log;
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fs::{self, File};
use std::io::{Cursor, Read, Write};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{self, AtomicU64};

/// Bi-directional mapping between an integer id and a name (`[u8]`).
///
/// Backed by the filesystem.
pub struct IdMap {
    log: log::Log,
    path: PathBuf,
    cached_next_free_ids: [AtomicU64; Group::COUNT],
    pub(crate) need_rebuild_non_master: bool,
}

/// Bi-directional mapping between an integer id and a name (`[u8]`).
///
/// Private. Stored in memory.
#[derive(Default)]
pub struct MemIdMap {
    id2name: HashMap<Id, VertexName>,
    name2id: BTreeMap<VertexName, Id>,
    cached_next_free_ids: [AtomicU64; Group::COUNT],
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
    const INDEX_ID_TO_NAME: usize = 0;
    const INDEX_NAME_TO_ID: usize = 1;

    /// Magic bytes in `Log` that indicates "remove all non-master id->name
    /// mappings". A valid entry has at least 8 bytes so does not conflict
    /// with this.
    const MAGIC_CLEAR_NON_MASTER: &'static [u8] = b"CLRNM";

    /// Create an [`IdMap`] backed by the given directory.
    ///
    /// By default, only read-only operations are allowed. For writing
    /// access, call [`IdMap::make_writable`] to get a writable instance.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let log = Self::log_open_options().open(path)?;
        Self::open_from_log(log)
    }

    pub(crate) fn try_clone(&self) -> Result<Self> {
        let result = Self {
            log: self.log.try_clone()?,
            path: self.path.clone(),
            cached_next_free_ids: Default::default(),
            need_rebuild_non_master: self.need_rebuild_non_master,
        };
        Ok(result)
    }

    pub(crate) fn open_from_log(log: log::Log) -> Result<Self> {
        let path = log.path().as_opt_path().unwrap().to_path_buf();
        Ok(Self {
            log,
            path,
            cached_next_free_ids: Default::default(),
            need_rebuild_non_master: false,
        })
    }

    pub(crate) fn log_open_options() -> log::OpenOptions {
        log::OpenOptions::new()
            .create(true)
            .index("id", |data| {
                assert!(Self::MAGIC_CLEAR_NON_MASTER.len() < 8);
                assert!(Group::BITS == 8);
                if data.len() < 8 {
                    if data == Self::MAGIC_CLEAR_NON_MASTER {
                        vec![log::IndexOutput::RemovePrefix(Box::new([
                            Group::NON_MASTER.0 as u8,
                        ]))]
                    } else {
                        panic!("bug: invalid segment {:?}", &data);
                    }
                } else {
                    vec![log::IndexOutput::Reference(0..8)]
                }
            })
            .index("name", |data| {
                if data.len() >= 8 {
                    vec![log::IndexOutput::Reference(8..data.len() as u64)]
                } else {
                    Vec::new()
                }
            })
            .flush_filter(Some(|_, _| {
                panic!("programming error: idmap changed by other process")
            }))
    }

    /// Return a [`SyncableIdMap`] instance that provides race-free
    /// filesytem read and write access by taking an exclusive lock.
    ///
    /// The [`SyncableIdMap`] instance provides a `sync` method that
    /// actually writes changes to disk.
    ///
    /// Block if another instance is taking the lock.
    pub fn prepare_filesystem_sync(&mut self) -> Result<SyncableIdMap> {
        if self.log.iter_dirty().next().is_some() {
            return programming(
                "prepare_filesystem_sync must be called without dirty in-memory entries",
            );
        }

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

    /// Find name by a specified integer id.
    pub fn find_name_by_id(&self, id: Id) -> Result<Option<&[u8]>> {
        let mut key = Vec::with_capacity(8);
        key.write_u64::<BigEndian>(id.0).unwrap();
        let key = self.log.lookup(Self::INDEX_ID_TO_NAME, key)?.nth(0);
        match key {
            Some(Ok(entry)) => {
                if entry.len() < 8 {
                    return bug("index key should have 8 bytes at least");
                }
                Ok(Some(&entry[8..]))
            }
            None => Ok(None),
            Some(Err(err)) => Err(err.into()),
        }
    }

    /// Find VertexName by a specified integer id.
    pub fn find_vertex_name_by_id(&self, id: Id) -> Result<Option<VertexName>> {
        self.find_name_by_id(id)
            .map(|v| v.map(|n| VertexName(self.log.slice_to_bytes(n))))
    }

    /// Find the integer id matching the given name.
    pub fn find_id_by_name(&self, name: &[u8]) -> Result<Option<Id>> {
        let key = self.log.lookup(Self::INDEX_NAME_TO_ID, name)?.nth(0);
        match key {
            Some(Ok(mut entry)) => {
                if entry.len() < 8 {
                    return bug("index key should have 8 bytes at least");
                }
                let id = Id(entry.read_u64::<BigEndian>().unwrap());
                // Double check. Id should <= next_free_id. This is useful for 'remove_non_master'
                // and re-insert ids.
                // This is because 'remove_non_master' only removes the id->name index, not
                // the name->id index.
                let group = id.group();
                if group != Group::MASTER && self.next_free_id(group)? <= id {
                    Ok(None)
                } else {
                    Ok(Some(id))
                }
            }
            None => Ok(None),
            Some(Err(err)) => Err(err.into()),
        }
    }

    /// Similar to `find_name_by_id`, but returns None if group > `max_group`.
    pub fn find_id_by_name_with_max_group(
        &self,
        name: &[u8],
        max_group: Group,
    ) -> Result<Option<Id>> {
        Ok(self.find_id_by_name(name)?.and_then(|id| {
            if id.group() <= max_group {
                Some(id)
            } else {
                None
            }
        }))
    }

    /// Insert a new entry mapping from a name to an id.
    ///
    /// Errors if the new entry conflicts with existing entries.
    pub fn insert(&mut self, id: Id, name: &[u8]) -> Result<()> {
        let group = id.group();
        if id < self.next_free_id(group)? {
            let existing_name = self.find_name_by_id(id)?;
            if let Some(existing_name) = existing_name {
                if existing_name == name {
                    return Ok(());
                } else {
                    return bug(format!(
                        "new entry {} = {:?} conflicts with an existing entry {} = {:?}",
                        id, name, id, existing_name
                    ));
                }
            }
        }
        let existing_id = self.find_id_by_name(name)?;
        if let Some(existing_id) = existing_id {
            // Allow re-assigning Ids from a higher group to a lower group.
            // For example, when a non-master commit gets merged into the
            // master branch, the id is re-assigned to master. But, the
            // ids in the master group will never be re-assigned to
            // non-master groups.
            if existing_id == id {
                return Ok(());
            } else if existing_id.group() <= group {
                return bug(format!(
                    "new entry {} = {:?} conflicts with an existing entry {} = {:?}",
                    id, name, existing_id, name
                ));
            }
            // Mark "need_rebuild_non_master". This prevents "sync" until
            // the callsite uses "remove_non_master" to remove and re-insert
            // non-master ids.
            self.need_rebuild_non_master = true;
        }

        let mut data = Vec::with_capacity(8 + name.len());
        data.write_u64::<BigEndian>(id.0).unwrap();
        data.write_all(name).unwrap();
        self.log.append(data)?;
        let next_free_id = self.cached_next_free_ids[group.0].get_mut();
        if id.0 >= *next_free_id {
            *next_free_id = id.0 + 1;
        }
        Ok(())
    }

    /// Return the next unused id in the given group.
    pub fn next_free_id(&self, group: Group) -> Result<Id> {
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

    /// Lookup names by hex prefix.
    fn find_names_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> Result<Vec<VertexName>> {
        self.log
            .lookup_prefix_hex(Self::INDEX_NAME_TO_ID, hex_prefix)?
            .take(limit)
            .map(|entry| {
                let (k, _v) = entry?;
                let vertex = self.log.slice_to_bytes(&k);
                Ok(VertexName(vertex))
            })
            .collect::<Result<_>>()
    }

    // Find an unused id that is bigger than existing ids.
    // Used internally. It should match `next_free_id`.
    fn get_next_free_id(log: &log::Log, group: Group) -> Result<Id> {
        // Checks should have been done at callsite.
        let lower_bound_id = group.min_id();
        let upper_bound_id = group.max_id();
        let lower_bound = lower_bound_id.to_bytearray();
        let upper_bound = upper_bound_id.to_bytearray();
        let range = &lower_bound[..]..=&upper_bound[..];
        let mut iter = log.lookup_range(Self::INDEX_ID_TO_NAME, range)?.rev();
        let id = match iter.nth(0) {
            None => lower_bound_id,
            Some(Ok((key, _))) => Id(Cursor::new(key).read_u64::<BigEndian>()? + 1),
            _ => return bug(format!("cannot read next_free_id for group {}", group)),
        };
        debug_assert!(id >= lower_bound_id);
        debug_assert!(id <= upper_bound_id);
        Ok(id)
    }
}

impl MemIdMap {
    /// Create an empty [`MemIdMap`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl Clone for MemIdMap {
    fn clone(&self) -> Self {
        Self {
            id2name: self.id2name.clone(),
            name2id: self.name2id.clone(),
            cached_next_free_ids: [
                AtomicU64::new(self.cached_next_free_ids[0].load(atomic::Ordering::SeqCst)),
                AtomicU64::new(self.cached_next_free_ids[1].load(atomic::Ordering::SeqCst)),
            ],
        }
    }
}

/// Return value of `assign_head`.
#[derive(Debug, Default)]
pub struct AssignHeadOutcome {
    /// New flat segments.
    pub segments: Vec<FlatSegment>,
}

impl AssignHeadOutcome {
    /// The id of the head.
    pub fn head_id(&self) -> Option<Id> {
        self.segments.last().map(|s| s.high)
    }

    /// Merge with another (newer) `AssignHeadOutcome`.
    pub fn merge(&mut self, rhs: Self) {
        if rhs.segments.is_empty() {
            return;
        }
        if self.segments.is_empty() {
            *self = rhs;
            return;
        }

        // sanity check: should be easy to verify - next_free_id provides
        // incremental ids.
        debug_assert!(self.segments.last().unwrap().high < rhs.segments[0].low);

        // NOTE: Consider merging segments for slightly better perf.
        self.segments.extend(rhs.segments);
    }

    /// Add graph edges: id -> parent_ids. Used by `assign_head`.
    fn push_edge(&mut self, id: Id, parent_ids: &[Id]) {
        let new_seg = || FlatSegment {
            low: id,
            high: id,
            parents: parent_ids.to_vec(),
        };

        // sanity check: this should be easy to verify - assign_head gets new ids
        // by `next_free_id()`, which should be incremental.
        debug_assert!(
            self.segments.last().map_or(Id(0), |s| s.high + 1) < id + 1,
            "push_edge(id={}, parent_ids={:?}) called out of order ({:?})",
            id,
            parent_ids,
            self
        );

        if parent_ids.len() != 1 || parent_ids[0] + 1 != id {
            // Start a new segment.
            self.segments.push(new_seg());
        } else {
            // Try to reuse the existing last segment.
            if let Some(seg) = self.segments.last_mut() {
                if seg.high + 1 == id {
                    seg.high = id;
                } else {
                    self.segments.push(new_seg());
                }
            } else {
                self.segments.push(new_seg());
            }
        }
    }

    #[cfg(test)]
    /// Verify against a parent function. For testing only.
    pub fn verify(&self, parent_func: impl Fn(Id) -> Result<Vec<Id>>) {
        for seg in &self.segments {
            assert_eq!(
                parent_func(seg.low).unwrap(),
                seg.parents,
                "parents mismtach for {} ({:?})",
                seg.low,
                &self
            );
            for id in (seg.low + 1).0..=seg.high.0 {
                let id = Id(id);
                assert_eq!(
                    parent_func(id).unwrap(),
                    vec![id - 1],
                    "parents mismatch for {} ({:?})",
                    id,
                    &self
                );
            }
        }
    }
}

/// Used as part of `AssignedIds`.
#[derive(Debug)]
pub struct FlatSegment {
    pub low: Id,
    pub high: Id,
    pub parents: Vec<Id>,
}

/// DAG-aware write operations.
pub trait IdMapAssignHead: IdConvert + IdMapWrite {
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
    fn assign_head<F>(
        &mut self,
        head: VertexName,
        parents_by_name: F,
        group: Group,
    ) -> Result<AssignHeadOutcome>
    where
        F: Fn(VertexName) -> Result<Vec<VertexName>>,
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
        let mut outcome = AssignHeadOutcome::default();

        // Emulate the stack in heap to avoid overflow.
        #[derive(Debug)]
        enum Todo {
            /// Visit parents. Finally assign self. This will eventually turn into AssignedId.
            Visit(VertexName),

            /// Assign a number if not assigned. Parents are visited.
            /// The `usize` provides the length of parents.
            Assign(VertexName, usize),

            /// Assigned Id. Will be picked by and pushed to the current `parent_ids` stack.
            AssignedId(Id),
        }
        use Todo::{Assign, AssignedId, Visit};
        let mut parent_ids: Vec<Id> = Vec::new();

        let mut todo_stack: Vec<Todo> = vec![Visit(head.clone())];
        while let Some(todo) = todo_stack.pop() {
            match todo {
                Visit(head) => {
                    // If the id was not assigned, or was assigned to a higher group,
                    // (re-)assign it to this group.
                    match self.vertex_id_with_max_group(&head, group)? {
                        None => {
                            let parents = parents_by_name(head.clone())?;
                            todo_stack.push(Todo::Assign(head, parents.len()));
                            // If the parent was not assigned, or was assigned to a higher group,
                            // (re-)assign the parent to this group.
                            // "rev" is the "optimization"
                            for p in parents.into_iter().rev() {
                                match self.vertex_id_with_max_group(&p, group) {
                                    Ok(Some(id)) => todo_stack.push(Todo::AssignedId(id)),
                                    _ => todo_stack.push(Todo::Visit(p)),
                                }
                            }
                        }
                        Some(id) => {
                            // Inlined Assign(id, ...) -> AssignedId(id)
                            parent_ids.push(id);
                        }
                    }
                }
                Assign(head, parent_len) => {
                    let parent_start = parent_ids.len() - parent_len;
                    let id = match self.vertex_id_with_max_group(&head, group)? {
                        Some(id) => id,
                        None => {
                            let id = self.next_free_id(group)?;
                            self.insert(id, head.as_ref())?;
                            let parents = &parent_ids[parent_start..];
                            outcome.push_edge(id, parents);
                            id
                        }
                    };
                    parent_ids.truncate(parent_start);
                    // Inlined AssignId(id);
                    parent_ids.push(id);
                }
                AssignedId(id) => {
                    parent_ids.push(id);
                }
            }
        }

        Ok(outcome)
    }
}

impl<T> IdMapAssignHead for T where T: IdConvert + IdMapWrite {}

pub trait IdMapBuildParents: IdConvert {
    /// Translate `get_parents` from taking names to taking `Id`s.
    fn build_get_parents_by_id<'a>(
        &'a self,
        get_parents_by_name: &'a dyn Fn(VertexName) -> Result<Vec<VertexName>>,
    ) -> Box<dyn Fn(Id) -> Result<Vec<Id>> + 'a> {
        let func = move |id: Id| -> Result<Vec<Id>> {
            let name = self.vertex_name(id)?;
            let parent_names: Vec<VertexName> = get_parents_by_name(name.clone())?;
            let mut result = Vec::with_capacity(parent_names.len());
            for parent_name in parent_names {
                let parent_id = self.vertex_id(parent_name)?;
                if parent_id >= id {
                    return programming(format!(
                        "parent {} {:?} should <= {} {:?}",
                        parent_id,
                        self.vertex_name(parent_id)?,
                        id,
                        &name
                    ));
                };
                result.push(parent_id);
            }
            Ok(result)
        };
        Box::new(func)
    }
}

impl<T> IdMapBuildParents for T where T: IdConvert {}

// Remove data.
impl IdMap {
    /// Mark non-master ids as "removed".
    pub fn remove_non_master(&mut self) -> Result<()> {
        self.log.append(IdMap::MAGIC_CLEAR_NON_MASTER)?;
        self.need_rebuild_non_master = false;
        // Invalidate the next free id cache.
        self.cached_next_free_ids = Default::default();
        if self.next_free_id(Group::NON_MASTER)? != Group::NON_MASTER.min_id() {
            return bug("remove_non_master did not take effect");
        }
        Ok(())
    }
}

impl<'a> SyncableIdMap<'a> {
    /// Write pending changes to disk.
    pub fn sync(&mut self) -> Result<()> {
        if self.need_rebuild_non_master {
            return bug("cannot sync with re-assigned ids unresolved");
        }
        self.map.log.sync()?;
        Ok(())
    }
}

impl fmt::Debug for IdMap {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "IdMap {{\n")?;
        for data in self.log.iter() {
            if let Ok(mut data) = data {
                let id = data.read_u64::<BigEndian>().unwrap();
                let mut name = Vec::with_capacity(20);
                data.read_to_end(&mut name).unwrap();
                let name = if name.len() >= 20 {
                    VertexName::from(name).to_hex()
                } else {
                    String::from_utf8_lossy(&name).to_string()
                };
                let id = Id(id);
                write!(f, "  {}: {},\n", name, id)?;
            }
        }
        write!(f, "}}\n")?;
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

/// Minimal write operations for IdMap.
pub trait IdMapWrite {
    fn insert(&mut self, id: Id, name: &[u8]) -> Result<()>;
    fn next_free_id(&self, group: Group) -> Result<Id>;
}

impl IdConvert for IdMap {
    fn vertex_id(&self, name: VertexName) -> Result<Id> {
        self.find_id_by_name(name.as_ref())?
            .ok_or_else(|| name.not_found_error())
    }
    fn vertex_id_with_max_group(&self, name: &VertexName, max_group: Group) -> Result<Option<Id>> {
        self.find_id_by_name_with_max_group(name.as_ref(), max_group)
    }
    fn vertex_name(&self, id: Id) -> Result<VertexName> {
        self.find_vertex_name_by_id(id)?
            .ok_or_else(|| id.not_found_error())
    }
    fn contains_vertex_name(&self, name: &VertexName) -> Result<bool> {
        Ok(self.find_id_by_name(name.as_ref())?.is_some())
    }
}

impl IdMapWrite for IdMap {
    fn insert(&mut self, id: Id, name: &[u8]) -> Result<()> {
        IdMap::insert(self, id, name)
    }
    fn next_free_id(&self, group: Group) -> Result<Id> {
        IdMap::next_free_id(self, group)
    }
}

impl IdConvert for MemIdMap {
    fn vertex_id(&self, name: VertexName) -> Result<Id> {
        let id = self
            .name2id
            .get(&name)
            .ok_or_else(|| name.not_found_error())?;
        Ok(*id)
    }
    fn vertex_id_with_max_group(&self, name: &VertexName, max_group: Group) -> Result<Option<Id>> {
        let optional_id = self.name2id.get(name).and_then(|id| {
            if id.group() <= max_group {
                Some(*id)
            } else {
                None
            }
        });
        Ok(optional_id)
    }
    fn vertex_name(&self, id: Id) -> Result<VertexName> {
        let name = self.id2name.get(&id).ok_or_else(|| id.not_found_error())?;
        Ok(name.clone())
    }
    fn contains_vertex_name(&self, name: &VertexName) -> Result<bool> {
        Ok(self.name2id.contains_key(name))
    }
}

impl IdMapWrite for MemIdMap {
    fn insert(&mut self, id: Id, name: &[u8]) -> Result<()> {
        let vertex_name = VertexName::copy_from(name);
        self.name2id.insert(vertex_name.clone(), id);
        self.id2name.insert(id, vertex_name);
        let group = id.group();
        // TODO: use fetch_max once stabilized.
        // (https://github.com/rust-lang/rust/issues/4865)
        let cached = self.cached_next_free_ids[group.0].load(atomic::Ordering::SeqCst);
        if id.0 >= cached {
            self.cached_next_free_ids[group.0].store(id.0 + 1, atomic::Ordering::SeqCst);
        }
        Ok(())
    }
    fn next_free_id(&self, group: Group) -> Result<Id> {
        let cached = self.cached_next_free_ids[group.0].load(atomic::Ordering::SeqCst);
        let id = Id(cached);
        Ok(id)
    }
}

impl PrefixLookup for IdMap {
    fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> Result<Vec<VertexName>> {
        self.find_names_by_hex_prefix(hex_prefix, limit)
    }
}

impl PrefixLookup for MemIdMap {
    fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> Result<Vec<VertexName>> {
        let start = VertexName::from_hex(hex_prefix)?;
        let mut result = Vec::new();
        for (vertex, _) in self.name2id.range(start..) {
            if !vertex.to_hex().as_bytes().starts_with(hex_prefix) {
                break;
            }
            result.push(vertex.clone());
            if result.len() >= limit {
                break;
            }
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_basic_operations() {
        let dir = tempdir().unwrap();
        let mut map = IdMap::open(dir.path()).unwrap();
        let mut map = map.prepare_filesystem_sync().unwrap();
        assert_eq!(map.next_free_id(Group::MASTER).unwrap().0, 0);
        map.insert(Id(1), b"abc").unwrap();
        assert_eq!(map.next_free_id(Group::MASTER).unwrap().0, 2);
        map.insert(Id(2), b"def").unwrap();
        assert_eq!(map.next_free_id(Group::MASTER).unwrap().0, 3);
        map.insert(Id(10), b"ghi").unwrap();
        assert_eq!(map.next_free_id(Group::MASTER).unwrap().0, 11);
        map.insert(Id(11), b"ghi").unwrap_err(); // ghi maps to 10
        map.insert(Id(10), b"ghi2").unwrap_err(); // 10 maps to ghi

        // Test another group.
        let id = map.next_free_id(Group::NON_MASTER).unwrap();
        map.insert(id, b"jkl").unwrap();
        map.insert(id, b"jkl").unwrap();
        map.insert(id, b"jkl2").unwrap_err(); // id maps to jkl
        map.insert(id + 1, b"jkl2").unwrap();
        map.insert(id + 2, b"jkl2").unwrap_err(); // jkl2 maps to id + 1
        map.insert(Id(15), b"jkl2").unwrap(); // reassign jkl2 to master group - ok.
        map.insert(id + 3, b"abc").unwrap_err(); // reassign abc to non-master group - error.
        assert_eq!(map.next_free_id(Group::NON_MASTER).unwrap(), id + 2);

        // Test hex lookup.
        assert_eq!(0x6a, b'j');
        assert_eq!(
            map.vertexes_by_hex_prefix(b"6a", 3).unwrap(),
            [
                VertexName::from(&b"jkl"[..]),
                VertexName::from(&b"jkl2"[..])
            ]
        );
        assert_eq!(
            map.vertexes_by_hex_prefix(b"6a", 1).unwrap(),
            [VertexName::from(&b"jkl"[..])]
        );
        assert!(map.vertexes_by_hex_prefix(b"6b", 1).unwrap().is_empty());

        for _ in 0..=1 {
            assert_eq!(map.find_name_by_id(Id(1)).unwrap().unwrap(), b"abc");
            assert_eq!(map.find_name_by_id(Id(2)).unwrap().unwrap(), b"def");
            assert!(map.find_name_by_id(Id(3)).unwrap().is_none());
            assert_eq!(map.find_name_by_id(Id(10)).unwrap().unwrap(), b"ghi");

            assert_eq!(map.find_id_by_name(b"abc").unwrap().unwrap().0, 1);
            assert_eq!(map.find_id_by_name(b"def").unwrap().unwrap().0, 2);
            assert_eq!(map.find_id_by_name(b"ghi").unwrap().unwrap().0, 10);
            assert_eq!(map.find_id_by_name(b"jkl").unwrap().unwrap(), id);
            assert_eq!(map.find_id_by_name(b"jkl2").unwrap().unwrap().0, 15);
            assert!(map.find_id_by_name(b"jkl3").unwrap().is_none());
            // HACK: allow sync with re-assigned ids.
            map.need_rebuild_non_master = false;
            map.sync().unwrap();
        }

        // Test Debug
        assert_eq!(
            format!("{:?}", map.deref()),
            r#"IdMap {
  abc: 1,
  def: 2,
  ghi: 10,
  jkl: N0,
  jkl2: N1,
  jkl2: 15,
}
"#
        );
    }
}
