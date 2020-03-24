/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::id::{Group, Id};
use crate::segment::{Segment, SegmentFlags};
use crate::Level;
use anyhow::{bail, ensure, Result};
use byteorder::{BigEndian, WriteBytesExt};
use fs2::FileExt;
use indexedlog::log;
use minibytes::Bytes;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::Cursor;
use std::iter;
use std::path::{Path, PathBuf};
use vlqencoding::VLQEncode;

pub trait IdDagStore {
    /// Maximum level segment in the store
    fn max_level(&self) -> Result<Level>;

    /// Find segment by level and head.
    fn find_segment_by_head_and_level(&self, head: Id, level: u8) -> Result<Option<Segment>>;

    /// Find flat segment containing the given id.
    fn find_flat_segment_including_id(&self, id: Id) -> Result<Option<Segment>>;

    /// Add a new segment.
    ///
    /// For simplicity, it does not check if the new segment overlaps with
    /// an existing segment (which is a logic error). Those checks can be
    /// offline.
    fn insert(
        &mut self,
        flags: SegmentFlags,
        level: Level,
        low: Id,
        high: Id,
        parents: &[Id],
    ) -> Result<()> {
        let segment = Segment::new(flags, level, low, high, parents);
        self.insert_segment(segment)
    }

    fn insert_segment(&mut self, segment: Segment) -> Result<()>;

    /// Return the next unused id for segments of the specified level.
    ///
    /// Useful for building segments incrementally.
    fn next_free_id(&self, level: Level, group: Group) -> Result<Id>;

    /// Find segments that covers `id..` range at the given level, within a same group.
    fn next_segments(&self, id: Id, level: Level) -> Result<Vec<Segment>>;

    /// Iterate through segments at the given level in descending order.
    fn iter_segments_descending<'a>(
        &'a self,
        max_high_id: Id,
        level: Level,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>>;

    /// Iterate through master flat segments that have the given parent.
    fn iter_master_flat_segments_with_parent<'a>(
        &'a self,
        parent: Id,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>>;

    /// Remove all non master Group identifiers from the DAG.
    fn remove_non_master(&mut self) -> Result<()>;

    /// Reload from the source of truth. Discard pending changes.
    fn reload(&mut self) -> Result<()>;

    /// Reload from the source of truth (without discarding pending changes).
    fn sync(&mut self) -> Result<()>;
}

pub trait GetLock {
    type LockT;
    fn get_lock(&self) -> Result<Self::LockT>;
}

pub struct IndexedLogStore {
    log: log::Log,
    path: PathBuf,
}

// Required functionality
impl IdDagStore for IndexedLogStore {
    fn max_level(&self) -> Result<Level> {
        let max_level = match self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, ..)?
            .rev()
            .nth(0)
        {
            None => 0,
            Some(key) => key?.0.get(0).cloned().unwrap_or(0),
        };
        Ok(max_level)
    }

    fn find_segment_by_head_and_level(&self, head: Id, level: u8) -> Result<Option<Segment>> {
        let key = Self::serialize_head_level_lookup_key(head, level);
        match self.log.lookup(Self::INDEX_LEVEL_HEAD, &key)?.nth(0) {
            None => Ok(None),
            Some(bytes) => Ok(Some(Segment(self.log.slice_to_bytes(bytes?)))),
        }
    }

    fn find_flat_segment_including_id(&self, id: Id) -> Result<Option<Segment>> {
        let level = 0;
        let low = Self::serialize_head_level_lookup_key(id, level);
        let high = [level + 1];
        let iter = self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, &low[..]..&high[..])?;
        for entry in iter {
            let (_, entries) = entry?;
            for entry in entries {
                let entry = entry?;
                let seg = Segment(self.log.slice_to_bytes(entry));
                if seg.span()?.low > id {
                    return Ok(None);
                }
                // low <= rev
                debug_assert!(seg.high()? >= id); // by range query
                return Ok(Some(seg));
            }
        }
        Ok(None)
    }

    fn insert_segment(&mut self, segment: Segment) -> Result<()> {
        self.log.append(&segment.0)?;
        Ok(())
    }

    fn next_free_id(&self, level: Level, group: Group) -> Result<Id> {
        let lower_bound = group.min_id().to_prefixed_bytearray(level);
        let upper_bound = group.max_id().to_prefixed_bytearray(level);
        let range = &lower_bound[..]..=&upper_bound[..];
        match self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, range)?
            .rev()
            .nth(0)
        {
            None => Ok(group.min_id()),
            Some(result) => {
                let (key, mut values) = result?;
                // PERF: The "next id" information can be also extracted from
                // `key` without going through values. Right now the code path
                // goes through values so `Segment` format changes wouldn't
                // break the logic here. If perf is really needed, we can change
                // logic here to not checking values.
                if let Some(bytes) = values.next() {
                    let seg = Segment(self.log.slice_to_bytes(bytes?));
                    Ok(seg.high()? + 1)
                } else {
                    bail!("key {:?} should have some values", key);
                }
            }
        }
    }

    fn next_segments(&self, id: Id, level: Level) -> Result<Vec<Segment>> {
        let lower_bound = Self::serialize_head_level_lookup_key(id, level);
        let upper_bound = Self::serialize_head_level_lookup_key(id.group().max_id(), level);
        let mut result = Vec::new();
        for entry in self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, &lower_bound[..]..=&upper_bound)?
        {
            let (_, values) = entry?;
            for value in values {
                result.push(Segment(self.log.slice_to_bytes(value?)));
            }
        }
        Ok(result)
    }

    fn iter_segments_descending<'a>(
        &'a self,
        max_high_id: Id,
        level: Level,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>> {
        let lower_bound = Self::serialize_head_level_lookup_key(Id::MIN, level);
        let upper_bound = Self::serialize_head_level_lookup_key(max_high_id, level);
        let iter = self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, &lower_bound[..]..=&upper_bound[..])?
            .rev();
        let iter = iter.flat_map(move |entry| match entry {
            Ok((_key, values)) => values
                .into_iter()
                .map(|value| {
                    let value = value?;
                    Ok(Segment(self.log.slice_to_bytes(value)))
                })
                .collect(),
            Err(err) => vec![Err(err.into())],
        });
        Ok(Box::new(iter))
    }

    fn iter_master_flat_segments_with_parent<'a>(
        &'a self,
        parent: Id,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>> {
        let mut key = Vec::with_capacity(8);
        key.write_vlq(parent.0)
            .expect("write to Vec should not fail");
        let iter = self.log.lookup(Self::INDEX_PARENT, &key)?;
        let iter = iter.map(move |result| match result {
            Ok(bytes) => Ok(Segment(self.log.slice_to_bytes(bytes))),
            Err(err) => Err(err.into()),
        });
        Ok(Box::new(iter))
    }

    /// Mark non-master ids as "removed".
    fn remove_non_master(&mut self) -> Result<()> {
        self.log.append(Self::MAGIC_CLEAR_NON_MASTER)?;
        // As an optimization, we could pass a max_level hint from iddag.
        // Doesn't seem necessary though.
        for level in 0..=self.max_level()? {
            ensure!(
                self.next_free_id(level, Group::NON_MASTER)? == Group::NON_MASTER.min_id(),
                "bug: remove_non_master did not take effect"
            );
        }
        Ok(())
    }

    fn reload(&mut self) -> Result<()> {
        self.log.clear_dirty()?;
        self.log.sync()?;
        Ok(())
    }

    fn sync(&mut self) -> Result<()> {
        self.log.sync()?;
        Ok(())
    }
}

impl GetLock for IndexedLogStore {
    type LockT = File;

    fn get_lock(&self) -> Result<File> {
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
        Ok(lock_file)
    }
}

impl IndexedLogStore {
    // Used internally to generate the index key for lookup
    fn serialize_head_level_lookup_key(value: Id, level: u8) -> [u8; Self::KEY_LEVEL_HEAD_LEN] {
        let mut buf = [0u8; Self::KEY_LEVEL_HEAD_LEN];
        {
            let mut cur = Cursor::new(&mut buf[..]);
            cur.write_u8(level).unwrap();
            cur.write_u64::<BigEndian>(value.0).unwrap();
            debug_assert_eq!(cur.position(), Self::KEY_LEVEL_HEAD_LEN as u64);
        }
        buf
    }
}

// Implementation details
impl IndexedLogStore {
    const INDEX_LEVEL_HEAD: usize = 0;
    const INDEX_PARENT: usize = 1;
    const KEY_LEVEL_HEAD_LEN: usize = Segment::OFFSET_DELTA - Segment::OFFSET_LEVEL;

    /// Magic bytes in `Log` that indicates "remove all non-master segments".
    /// A Segment entry has at least KEY_LEVEL_HEAD_LEN (9) bytes so it does
    /// not conflict with this.
    const MAGIC_CLEAR_NON_MASTER: &'static [u8] = b"CLRNM";

    pub fn log_open_options() -> log::OpenOptions {
        log::OpenOptions::new()
            .create(true)
            .index("level-head", |data| {
                // (level, high)
                assert!(Self::MAGIC_CLEAR_NON_MASTER.len() < Segment::OFFSET_DELTA);
                assert!(Group::BITS == 8);
                if data.len() < Segment::OFFSET_DELTA {
                    if data == Self::MAGIC_CLEAR_NON_MASTER {
                        let max_level = 255;
                        (0..=max_level)
                            .map(|level| {
                                log::IndexOutput::RemovePrefix(Box::new([
                                    level,
                                    Group::NON_MASTER.0 as u8,
                                ]))
                            })
                            .collect()
                    } else {
                        panic!("bug: invalid segment {:?}", &data);
                    }
                } else {
                    vec![log::IndexOutput::Reference(
                        Segment::OFFSET_LEVEL as u64..Segment::OFFSET_DELTA as u64,
                    )]
                }
            })
            .index("parent", |data| {
                // parent -> child for flat segments
                let seg = Segment(Bytes::copy_from_slice(data));
                let mut result = Vec::new();
                if seg.level().ok() == Some(0)
                    && seg.high().map(|id| id.group()).ok() == Some(Group::MASTER)
                {
                    // This should never pass since MAGIC_CLEAR_NON_MASTER[0] != 0.
                    assert_ne!(
                        data,
                        Self::MAGIC_CLEAR_NON_MASTER,
                        "bug: MAGIC_CLEAR_NON_MASTER conflicts with data"
                    );
                    if let Ok(parents) = seg.parents() {
                        for id in parents {
                            let mut bytes = Vec::with_capacity(8);
                            bytes.write_vlq(id.0).expect("write to Vec should not fail");
                            // Attempt to use IndexOutput::Reference instead of
                            // IndexOutput::Owned to reduce index size.
                            match data.windows(bytes.len()).position(|w| w == &bytes[..]) {
                                Some(pos) => result.push(log::IndexOutput::Reference(
                                    pos as u64..(pos + bytes.len()) as u64,
                                )),
                                None => panic!("bug: {:?} should contain {:?}", &data, &bytes),
                            }
                        }
                    }
                }
                result
            })
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let log = Self::log_open_options().open(path.clone())?;
        Ok(Self { log, path })
    }

    pub fn open_from_log(log: log::Log) -> Self {
        let path = log.path().as_opt_path().unwrap().to_path_buf();
        Self { log, path }
    }

    pub fn try_clone_without_dirty(&self) -> Result<IndexedLogStore> {
        let log = self.log.try_clone_without_dirty()?;
        let store = IndexedLogStore {
            log,
            path: self.path.clone(),
        };
        Ok(store)
    }
}

#[derive(Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum StoreId {
    Master(usize),
    NonMaster(usize),
}

pub struct InProcessStore {
    master_segments: Vec<Segment>,
    non_master_segments: Vec<Segment>,
    // level -> head -> serialized Segment
    level_head_index: Vec<BTreeMap<Id, StoreId>>,
    // parent -> serialized Segment
    parent_index: BTreeMap<Id, BTreeSet<StoreId>>,
}

impl IdDagStore for InProcessStore {
    fn max_level(&self) -> Result<Level> {
        Ok((self.level_head_index.len().max(1) - 1) as Level)
    }

    fn find_segment_by_head_and_level(&self, head: Id, level: Level) -> Result<Option<Segment>> {
        let answer = self
            .get_head_index(level)
            .and_then(|head_index| head_index.get(&head))
            .map(|store_id| self.get_segment(store_id));
        Ok(answer)
    }

    fn find_flat_segment_including_id(&self, id: Id) -> Result<Option<Segment>> {
        let level = 0;
        let answer = self
            .get_head_index(level)
            .and_then(|head_index| head_index.range(id..).next())
            .map(|(_, store_id)| self.get_segment(store_id));
        Ok(answer)
    }

    fn insert_segment(&mut self, segment: Segment) -> Result<()> {
        let high = segment.high()?;
        let level = segment.level()?;
        let parents = segment.parents()?;

        let store_id = match high.group() {
            Group::MASTER => {
                self.master_segments.push(segment);
                StoreId::Master(self.master_segments.len() - 1)
            }
            _ => {
                self.non_master_segments.push(segment);
                StoreId::NonMaster(self.non_master_segments.len() - 1)
            }
        };
        if level == 0 && high.group() == Group::MASTER {
            for parent in parents {
                let children = self.parent_index.entry(parent).or_insert(BTreeSet::new());
                children.insert(store_id);
            }
        }
        self.get_head_index_mut(level).insert(high, store_id);
        Ok(())
    }

    fn remove_non_master(&mut self) -> Result<()> {
        // Note. The parent index should not contain any non master entries.
        for segment in self.non_master_segments.iter() {
            let level = segment.level()?;
            let head = segment.head()?;
            self.level_head_index
                .get_mut(level as usize)
                .map(|head_index| head_index.remove(&head));
        }
        self.non_master_segments = Vec::new();
        Ok(())
    }

    fn next_free_id(&self, level: Level, group: Group) -> Result<Id> {
        match self.get_head_index(level).and_then(|head_index| {
            head_index
                .range(group.min_id()..=group.max_id())
                .rev()
                .next()
        }) {
            None => Ok(group.min_id()),
            Some((_, store_id)) => {
                let segment = self.get_segment(store_id);
                Ok(segment.high()? + 1)
            }
        }
    }

    fn next_segments(&self, id: Id, level: Level) -> Result<Vec<Segment>> {
        match self.get_head_index(level) {
            None => Ok(vec![]),
            Some(head_index) => {
                let segments = head_index
                    .range(id..id.group().max_id())
                    .map(|(_, store_id)| self.get_segment(store_id))
                    .collect();
                Ok(segments)
            }
        }
    }

    fn iter_segments_descending<'a>(
        &'a self,
        max_high_id: Id,
        level: Level,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>> {
        match self.get_head_index(level) {
            None => Ok(Box::new(iter::empty())),
            Some(head_index) => {
                let iter = head_index
                    .range(Id::MIN..=max_high_id)
                    .rev()
                    .map(move |(_, store_id)| Ok(self.get_segment(store_id)));
                Ok(Box::new(iter))
            }
        }
    }

    fn iter_master_flat_segments_with_parent<'a>(
        &'a self,
        parent: Id,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>> {
        match self.parent_index.get(&parent) {
            None => Ok(Box::new(iter::empty())),
            Some(children) => {
                let iter = children
                    .iter()
                    .map(move |store_id| Ok(self.get_segment(store_id)));
                Ok(Box::new(iter))
            }
        }
    }

    fn reload(&mut self) -> Result<()> {
        Ok(())
    }

    fn sync(&mut self) -> Result<()> {
        Ok(())
    }
}

impl InProcessStore {
    fn get_head_index(&self, level: Level) -> Option<&BTreeMap<Id, StoreId>> {
        self.level_head_index.get(level as usize)
    }

    fn get_head_index_mut(&mut self, level: Level) -> &mut BTreeMap<Id, StoreId> {
        if self.level_head_index.len() <= level as usize {
            self.level_head_index
                .resize(level as usize + 1, BTreeMap::new());
        }
        &mut self.level_head_index[level as usize]
    }

    fn get_segment(&self, store_id: &StoreId) -> Segment {
        match store_id {
            &StoreId::Master(offset) => self.master_segments[offset].clone(),
            &StoreId::NonMaster(offset) => self.non_master_segments[offset].clone(),
        }
    }
}

impl InProcessStore {
    pub fn new() -> Self {
        InProcessStore {
            master_segments: Vec::new(),
            non_master_segments: Vec::new(),
            level_head_index: Vec::new(),
            parent_index: BTreeMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use once_cell::sync::Lazy;
    use std::ops::Deref;

    fn nid(id: u64) -> Id {
        Group::NON_MASTER.min_id() + id
    }
    //  0---2--3--4--5--10--11--13--N0--N1--N2--N5--N6
    //   \-1 \-6--8--9-/      \-12   \-N3--N4--/
    //          \-7
    static LEVEL0_HEAD2: Lazy<Segment> =
        Lazy::new(|| Segment::new(SegmentFlags::HAS_ROOT, 0 as Level, Id(0), Id(2), &[]));
    static LEVEL0_HEAD5: Lazy<Segment> =
        Lazy::new(|| Segment::new(SegmentFlags::ONLY_HEAD, 0 as Level, Id(3), Id(5), &[Id(2)]));
    static LEVEL0_HEAD9: Lazy<Segment> =
        Lazy::new(|| Segment::new(SegmentFlags::empty(), 0 as Level, Id(6), Id(9), &[Id(2)]));
    static LEVEL0_HEAD13: Lazy<Segment> = Lazy::new(|| {
        Segment::new(
            SegmentFlags::empty(),
            0 as Level,
            Id(10),
            Id(13),
            &[Id(5), Id(9)],
        )
    });

    static LEVEL0_HEADN2: Lazy<Segment> =
        Lazy::new(|| Segment::new(SegmentFlags::empty(), 0 as Level, nid(0), nid(2), &[Id(13)]));
    static LEVEL0_HEADN4: Lazy<Segment> =
        Lazy::new(|| Segment::new(SegmentFlags::empty(), 0 as Level, nid(3), nid(4), &[nid(0)]));
    static LEVEL0_HEADN6: Lazy<Segment> = Lazy::new(|| {
        Segment::new(
            SegmentFlags::empty(),
            0 as Level,
            nid(5),
            nid(6),
            &[nid(2), nid(4)],
        )
    });

    static LEVEL1_HEAD13: Lazy<Segment> =
        Lazy::new(|| Segment::new(SegmentFlags::HAS_ROOT, 1 as Level, Id(0), Id(13), &[]));
    static LEVEL1_HEADN6: Lazy<Segment> = Lazy::new(|| {
        Segment::new(
            SegmentFlags::HAS_ROOT,
            1 as Level,
            nid(0),
            nid(6),
            &[Id(13)],
        )
    });

    fn init_store(segments: Vec<&Segment>) -> InProcessStore {
        let mut store = InProcessStore::new();
        for segment in segments {
            store.insert_segment(segment.clone()).unwrap();
        }
        store
    }

    fn get_in_process_store() -> InProcessStore {
        let segments: Vec<&Segment> = vec![
            &LEVEL0_HEAD2,
            &LEVEL0_HEAD5,
            &LEVEL0_HEAD9,
            &LEVEL0_HEAD13,
            &LEVEL1_HEAD13,
            &LEVEL0_HEADN2,
            &LEVEL0_HEADN4,
            &LEVEL0_HEADN6,
            &LEVEL1_HEADN6,
        ];
        init_store(segments)
    }

    fn segments_to_owned(segments: &[&Segment]) -> Vec<Segment> {
        segments.into_iter().cloned().cloned().collect()
    }

    #[test]
    fn test_in_process_store_insert() {
        let _store = get_in_process_store();
        // `get_in_process_stores` does inserts, we care that nothings panics.
    }

    #[test]
    fn test_in_process_store_find_segment_by_head_and_level() {
        let store = get_in_process_store();
        let segment = store
            .find_segment_by_head_and_level(Id(13), 1 as Level)
            .unwrap()
            .unwrap();
        assert_eq!(&segment, LEVEL1_HEAD13.deref());

        let segment = store
            .find_segment_by_head_and_level(Id(5), 0 as Level)
            .unwrap()
            .unwrap();
        assert_eq!(&segment, LEVEL0_HEAD5.deref());

        let segment = store
            .find_segment_by_head_and_level(nid(2), 0 as Level)
            .unwrap()
            .unwrap();
        assert_eq!(&segment, LEVEL0_HEADN2.deref());
    }

    #[test]
    fn test_in_process_store_find_flat_segment_including_id() {
        let store = get_in_process_store();
        let segment = store
            .find_flat_segment_including_id(Id(10))
            .unwrap()
            .unwrap();
        assert_eq!(&segment, LEVEL0_HEAD13.deref());

        let segment = store
            .find_flat_segment_including_id(Id(0))
            .unwrap()
            .unwrap();
        assert_eq!(&segment, LEVEL0_HEAD2.deref());

        let segment = store
            .find_flat_segment_including_id(nid(1))
            .unwrap()
            .unwrap();
        assert_eq!(&segment, LEVEL0_HEADN2.deref());
    }

    #[test]
    fn test_in_process_store_next_free_id() {
        let store = get_in_process_store();
        assert_eq!(
            store.next_free_id(0 as Level, Group::MASTER).unwrap(),
            Id(14)
        );
        assert_eq!(
            store.next_free_id(0 as Level, Group::NON_MASTER).unwrap(),
            nid(7)
        );
        assert_eq!(
            store.next_free_id(1 as Level, Group::MASTER).unwrap(),
            Id(14)
        );
        assert_eq!(
            store.next_free_id(2 as Level, Group::MASTER).unwrap(),
            Group::MASTER.min_id()
        );
    }

    #[test]
    fn test_in_process_store_next_segments() {
        let store = get_in_process_store();

        let segments = store.next_segments(Id(4), 0 as Level).unwrap();
        let expected = segments_to_owned(&[&LEVEL0_HEAD5, &LEVEL0_HEAD9, &LEVEL0_HEAD13]);
        assert_eq!(segments, expected);

        let segments = store.next_segments(Id(14), 0 as Level).unwrap();
        assert!(segments.is_empty());

        let segments = store.next_segments(Id(0), 1 as Level).unwrap();
        let expected = segments_to_owned(&[&LEVEL1_HEAD13]);
        assert_eq!(segments, expected);

        let segments = store.next_segments(Id(0), 2 as Level).unwrap();
        assert!(segments.is_empty());
    }

    #[test]
    fn test_in_process_store_max_level() {
        let store = get_in_process_store();
        assert_eq!(store.max_level().unwrap(), 1);

        let store = InProcessStore::new();
        assert_eq!(store.max_level().unwrap(), 0);
    }

    #[test]
    fn test_in_process_store_iter_segments_descending() {
        let store = get_in_process_store();

        let answer = store
            .iter_segments_descending(Id(12), 0)
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        let expected = segments_to_owned(&[&LEVEL0_HEAD9, &LEVEL0_HEAD5, &LEVEL0_HEAD2]);
        assert_eq!(answer, expected);

        let mut answer = store.iter_segments_descending(Id(1), 0).unwrap();
        assert!(answer.next().is_none());

        let answer = store
            .iter_segments_descending(Id(13), 1)
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        let expected = segments_to_owned(&[&LEVEL1_HEAD13]);
        assert_eq!(answer, expected);

        let mut answer = store.iter_segments_descending(Id(5), 2).unwrap();
        assert!(answer.next().is_none());
    }

    #[test]
    fn test_in_process_store_iter_master_flat_segments_with_parent() {
        let store = get_in_process_store();

        let answer = store
            .iter_master_flat_segments_with_parent(Id(2))
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap();
        let expected = segments_to_owned(&[&LEVEL0_HEAD5, &LEVEL0_HEAD9]);
        assert_eq!(answer, expected);

        let mut answer = store.iter_master_flat_segments_with_parent(Id(13)).unwrap();
        assert!(answer.next().is_none());

        let mut answer = store.iter_master_flat_segments_with_parent(Id(4)).unwrap();
        assert!(answer.next().is_none());

        let mut answer = store.iter_master_flat_segments_with_parent(nid(2)).unwrap();
        assert!(answer.next().is_none());
    }

    #[test]
    fn test_in_process_store_remove_non_master() {
        let mut store = get_in_process_store();

        store.remove_non_master().unwrap();

        assert!(store
            .find_segment_by_head_and_level(nid(2), 0 as Level)
            .unwrap()
            .is_none());
        assert!(store
            .find_flat_segment_including_id(nid(1))
            .unwrap()
            .is_none());
        assert_eq!(
            store.next_free_id(0 as Level, Group::NON_MASTER).unwrap(),
            nid(0)
        );
        assert!(store
            .iter_master_flat_segments_with_parent(nid(2))
            .unwrap()
            .next()
            .is_none());
    }
}
