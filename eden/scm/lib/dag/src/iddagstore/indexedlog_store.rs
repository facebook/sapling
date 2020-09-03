/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::GetLock;
use super::IdDagStore;
use crate::errors::bug;
use crate::id::{Group, Id};
use crate::segment::Segment;
use crate::Level;
use crate::Result;
use byteorder::{BigEndian, WriteBytesExt};
use fs2::FileExt;
use indexedlog::log;
use minibytes::Bytes;
use std::fs::{self, File};
use std::io::Cursor;
use std::path::{Path, PathBuf};

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
                    bug(format!("key {:?} should have values in next_free_id", key))
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

    fn iter_segments_ascending<'a>(
        &'a self,
        min_high_id: Id,
        level: Level,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a + Send + Sync>> {
        let lower_bound = Self::serialize_head_level_lookup_key(min_high_id, level);
        let upper_bound = Self::serialize_head_level_lookup_key(Id::MAX, level);
        let iter = self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, &lower_bound[..]..=&upper_bound[..])?;
        let iter = iter.flat_map(move |entry| match entry {
            Ok((_key, values)) => values
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
        let mut key = Vec::with_capacity(9);
        // child (segment low id) is in the "master" group.
        key.write_u8(Group::MASTER.0 as u8).unwrap();
        key.write_u64::<BigEndian>(parent.0).unwrap();
        let iter = self.log.lookup(Self::INDEX_PARENT, &key)?;
        let iter = iter.map(move |result| match result {
            Ok(bytes) => Ok(Segment(self.log.slice_to_bytes(bytes))),
            Err(err) => Err(err.into()),
        });
        Ok(Box::new(iter))
    }

    fn iter_flat_segments_with_parent<'a>(
        &'a self,
        parent: Id,
    ) -> Result<Box<dyn Iterator<Item = Result<Segment>> + 'a>> {
        let get_iter = |group: Group| -> Result<_> {
            let mut key = Vec::with_capacity(9);
            key.write_u8(group.0 as u8).unwrap();
            key.write_u64::<BigEndian>(parent.0).unwrap();
            let iter = self.log.lookup(Self::INDEX_PARENT, &key)?;
            let iter = iter.map(move |result| match result {
                Ok(bytes) => Ok(Segment(self.log.slice_to_bytes(bytes))),
                Err(err) => Err(err.into()),
            });
            Ok(iter)
        };
        let iter: Box<dyn Iterator<Item = Result<Segment>> + 'a> =
            if parent.group() == Group::MASTER {
                Box::new(get_iter(Group::MASTER)?.chain(get_iter(Group::NON_MASTER)?))
            } else {
                Box::new(get_iter(Group::NON_MASTER)?)
            };
        Ok(iter)
    }

    /// Mark non-master ids as "removed".
    fn remove_non_master(&mut self) -> Result<()> {
        self.log.append(Self::MAGIC_CLEAR_NON_MASTER)?;
        // As an optimization, we could pass a max_level hint from iddag.
        // Doesn't seem necessary though.
        for level in 0..=self.max_level()? {
            if self.next_free_id(level, Group::NON_MASTER)? != Group::NON_MASTER.min_id() {
                return bug("remove_non_master did not take effect");
            }
        }
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

    fn reload(&mut self, _lock: &Self::LockT) -> Result<()> {
        self.log.clear_dirty()?;
        self.log.sync()?;
        Ok(())
    }

    fn persist(&mut self, _lock: &Self::LockT) -> Result<()> {
        self.log.sync()?;
        Ok(())
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
            .index("group-parent", |data| {
                //  child-group parent -> child for flat segments
                //  ^^^^^^^^^^^ ^^^^^^
                //  u8          u64 BE
                //
                //  The "child-group" prefix is used for invalidating index when
                //  non-master Ids get re-assigned.
                if data.len() < Segment::OFFSET_DELTA && data == Self::MAGIC_CLEAR_NON_MASTER {
                    // Invalidate child-group == 1 entries
                    return vec![log::IndexOutput::RemovePrefix(Box::new([
                        Group::NON_MASTER.0 as u8,
                    ]))];
                }
                let seg = Segment(Bytes::copy_from_slice(data));
                let mut result = Vec::new();
                if seg.level().ok() == Some(0) {
                    // This should never pass since MAGIC_CLEAR_NON_MASTER[0] != 0.
                    // ([0] stores level: u8).
                    assert_ne!(
                        data,
                        Self::MAGIC_CLEAR_NON_MASTER,
                        "bug: MAGIC_CLEAR_NON_MASTER conflicts with data"
                    );
                    if let (Ok(parents), Ok(span)) = (seg.parents(), seg.span()) {
                        let group = span.low.group();
                        assert_eq!(
                            span.low.group(),
                            span.high.group(),
                            "Cross-group segment is unexpected"
                        );
                        for id in parents {
                            let mut bytes = Vec::with_capacity(9);
                            bytes.write_u8(group.0 as u8).unwrap();
                            bytes.write_u64::<BigEndian>(id.0).unwrap();
                            result.push(log::IndexOutput::Owned(bytes.into()));
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

    pub fn try_clone(&self) -> Result<IndexedLogStore> {
        let log = self.log.try_clone()?;
        let store = IndexedLogStore {
            log,
            path: self.path.clone(),
        };
        Ok(store)
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
