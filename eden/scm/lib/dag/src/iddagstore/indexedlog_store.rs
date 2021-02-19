/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::IdDagStore;
use crate::errors::bug;
use crate::id::{Group, Id};
use crate::ops::Persist;
use crate::segment::Segment;
use crate::segment::SegmentFlags;
use crate::Level;
use crate::Result;
use byteorder::{BigEndian, WriteBytesExt};
use fs2::FileExt;
use indexedlog::log;
use minibytes::Bytes;
use std::fs::{self, File};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicU8;
use std::sync::atomic::Ordering::AcqRel;
use std::sync::atomic::Ordering::Acquire;
use std::sync::atomic::Ordering::Release;

pub struct IndexedLogStore {
    log: log::Log,
    path: PathBuf,
    cached_max_level: AtomicU8,
}

const MAX_LEVEL_UNKNOWN: u8 = 0;

// Required functionality
impl IdDagStore for IndexedLogStore {
    fn max_level(&self) -> Result<Level> {
        let max_level = self.cached_max_level.load(Acquire);
        if max_level != MAX_LEVEL_UNKNOWN {
            return Ok(max_level);
        }
        let max_level = match self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, ..)?
            .rev()
            .nth(0)
        {
            None => 0,
            Some(key) => key?.0.get(0).cloned().unwrap_or(0),
        };
        self.cached_max_level.store(max_level, Release);
        Ok(max_level)
    }

    fn find_segment_by_head_and_level(&self, head: Id, level: u8) -> Result<Option<Segment>> {
        let key = Self::serialize_head_level_lookup_key(head, level);
        match self.log.lookup(Self::INDEX_LEVEL_HEAD, &key)?.nth(0) {
            None => Ok(None),
            Some(bytes) => Ok(Some(self.segment_from_slice(bytes?))),
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
                let seg = self.segment_from_slice(entry);
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
        let level = segment.level()?;
        self.cached_max_level.fetch_max(level, AcqRel);
        // When inserting a new flat segment, consider merging it with the last
        // flat segment on disk.
        //
        // Turn:
        //
        //   [last segment] [(new) segment]
        //
        // Into:
        //
        //   [------------]
        //    (removed)
        //   [(new, merged) segment       ]
        //    (in memory)
        if level == 0 {
            if self.maybe_insert_merged_flat_segment(&segment)? {
                return Ok(());
            }
        }
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
                    let seg = self.segment_from_slice(bytes?);
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
                result.push(self.segment_from_slice(value?));
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
        let iter = iter.flat_map(move |entry| {
            match entry {
                Ok((_key, values)) => values
                    .into_iter()
                    .map(|value| {
                        let value = value?;
                        Ok(self.segment_from_slice(value))
                    })
                    .collect(),
                Err(err) => vec![Err(err.into())],
            }
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
        let iter = iter.flat_map(move |entry| {
            match entry {
                Ok((_key, values)) => values
                    .map(|value| {
                        let value = value?;
                        Ok(self.segment_from_slice(value))
                    })
                    .collect(),
                Err(err) => vec![Err(err.into())],
            }
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
        let iter = iter.map(move |result| {
            match result {
                Ok(bytes) => Ok(self.segment_from_slice(bytes)),
                Err(err) => Err(err.into()),
            }
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
            let iter = iter.map(move |result| {
                match result {
                    Ok(bytes) => Ok(self.segment_from_slice(bytes)),
                    Err(err) => Err(err.into()),
                }
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

impl Persist for IndexedLogStore {
    type Lock = File;

    fn lock(&mut self) -> Result<File> {
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

    fn reload(&mut self, _lock: &Self::Lock) -> Result<()> {
        self.log.clear_dirty()?;
        self.log.sync()?;
        Ok(())
    }

    fn persist(&mut self, _lock: &Self::Lock) -> Result<()> {
        self.log.sync()?;
        Ok(())
    }
}

impl IndexedLogStore {
    /// Attempt to merge the flat `segment` with the last flat segment to reduce
    /// fragmentation.
    ///
    /// ```plain,ignore
    /// [---last segment---] [---segment---]
    ///                    ^---- the only parent of segment
    /// [---merged segment-----------------]
    /// ```
    ///
    /// Return true if the merged segment was inserted.
    fn maybe_insert_merged_flat_segment(&mut self, segment: &Segment) -> Result<bool> {
        let level = segment.level()?;
        if level != 0 {
            // Only applies to flat segments.
            return Ok(false);
        }
        if segment.has_root()? {
            // Cannot merge if segment has roots (implies no parent for a flat segment).
            return Ok(false);
        }
        let span = segment.span()?;
        let group = span.low.group();
        if group != Group::MASTER {
            // Do not merge non-master groups for simplicity.
            return Ok(false);
        }
        let parents = segment.parents()?;
        if parents.len() != 1 || parents[0] + 1 != span.low {
            // Cannot merge - span.low dos not have parent [low-1] (non linear).
            return Ok(false);
        }
        let last_segment = match self.iter_segments_descending(group.max_id(), 0)?.next() {
            Some(Ok(s)) => s,
            _ => return Ok(false), // Cannot merge - No last flat segment.
        };
        let last_span = last_segment.span()?;
        if last_span.high + 1 != span.low {
            // Cannot merge - Two spans are not connected.
            return Ok(false);
        }

        // Can merge!

        // Sanity check: No high-level segments should cover "last_span".
        for lv in 1..=self.max_level()? {
            if self
                .find_segment_by_head_and_level(last_span.high, lv)?
                .is_some()
            {
                return bug(format!(
                    "lv{} segment should not cover last flat segment {:?}! ({})",
                    lv, last_span, "check build_high_level_segments"
                ));
            }
        }

        // Calculate the merged segment.
        let merged = {
            let last_parents = last_segment.parents()?;
            let flags = {
                let last_flags = last_segment.flags()?;
                let flags = segment.flags()?;
                (flags & SegmentFlags::ONLY_HEAD) | (last_flags & SegmentFlags::HAS_ROOT)
            };
            Segment::new(flags, level, last_span.low, span.high, &last_parents)
        };

        tracing::debug!(
            "merge flat segments {:?} + {:?} => {:?}",
            &last_segment,
            &segment,
            &merged
        );

        let mut bytes = Vec::with_capacity(merged.0.len() + 10);
        bytes.extend_from_slice(IndexedLogStore::MAGIC_REWRITE_LAST_FLAT);
        bytes.extend_from_slice(&Self::serialize_head_level_lookup_key(
            last_span.high,
            level,
        ));
        bytes.extend_from_slice(&merged.0);
        self.log.append(&bytes)?;

        Ok(true)
    }

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

    fn segment_from_slice(&self, bytes: &[u8]) -> Segment {
        let bytes = if bytes.starts_with(IndexedLogStore::MAGIC_REWRITE_LAST_FLAT) {
            let start = Self::MAGIC_REWRITE_LAST_FLAT.len() + Self::KEY_LEVEL_HEAD_LEN;
            &bytes[start..]
        } else {
            bytes
        };
        Segment(self.log.slice_to_bytes(bytes))
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

    /// Magic bytes in `Log` that this entry replaces a previous flat segment.
    ///
    /// Format:
    ///
    /// ```plain,ignore
    /// MAGIC_CLEAR_NON_MASTER + LEVEL (0u8) + PREVIOUS_HEAD (u64) + SEGMENT
    /// ```
    ///
    /// The `LEVEL + PREVIOUS_HEAD` part is used to remove the segment from the
    /// `(level, head)` index.
    const MAGIC_REWRITE_LAST_FLAT: &'static [u8] = &[0xf0];

    pub fn log_open_options() -> log::OpenOptions {
        log::OpenOptions::new()
            .create(true)
            .index("level-head", |data| {
                // (level, high)
                assert!(Self::MAGIC_CLEAR_NON_MASTER.len() < Segment::OFFSET_DELTA);
                assert!(Group::BITS == 8);
                assert_ne!(
                    SegmentFlags::all().bits()
                        & Self::MAGIC_REWRITE_LAST_FLAT[Segment::OFFSET_FLAGS],
                    Self::MAGIC_REWRITE_LAST_FLAT[Segment::OFFSET_FLAGS],
                    "MAGIC_REWRITE_LAST_FLAT should not conflict with possible flags"
                );
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
                } else if data.starts_with(Self::MAGIC_REWRITE_LAST_FLAT) {
                    // See MAGIC_REWRITE_LAST_FLAT for format.
                    let start = Self::MAGIC_REWRITE_LAST_FLAT.len();
                    let end = start + Segment::OFFSET_DELTA - Segment::OFFSET_LEVEL;
                    let previous_index = &data[start..end];
                    vec![
                        log::IndexOutput::Remove(previous_index.to_vec().into_boxed_slice()),
                        log::IndexOutput::Reference(
                            (end + Segment::OFFSET_LEVEL) as u64
                                ..(end + Segment::OFFSET_DELTA) as u64,
                        ),
                    ]
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
                if data == Self::MAGIC_CLEAR_NON_MASTER {
                    // Invalidate child-group == 1 entries
                    return vec![log::IndexOutput::RemovePrefix(Box::new([
                        Group::NON_MASTER.0 as u8,
                    ]))];
                }

                if data.starts_with(Self::MAGIC_REWRITE_LAST_FLAT) {
                    // No need to create new indexes. The existing parent -> child
                    // indexes for the old segment is applicable for the new segment.
                    return Vec::new();
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
        Ok(Self {
            log,
            path,
            cached_max_level: AtomicU8::new(MAX_LEVEL_UNKNOWN),
        })
    }

    pub fn open_from_log(log: log::Log) -> Self {
        let path = log.path().as_opt_path().unwrap().to_path_buf();
        Self {
            log,
            path,
            cached_max_level: AtomicU8::new(MAX_LEVEL_UNKNOWN),
        }
    }

    pub fn try_clone(&self) -> Result<IndexedLogStore> {
        let log = self.log.try_clone()?;
        let store = IndexedLogStore {
            log,
            path: self.path.clone(),
            cached_max_level: AtomicU8::new(self.cached_max_level.load(Acquire)),
        };
        Ok(store)
    }

    pub fn try_clone_without_dirty(&self) -> Result<IndexedLogStore> {
        let log = self.log.try_clone_without_dirty()?;
        let store = IndexedLogStore {
            log,
            path: self.path.clone(),
            cached_max_level: AtomicU8::new(MAX_LEVEL_UNKNOWN),
        };
        Ok(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_persisted_segments() -> Result<()> {
        // Test that a persisted segment can still be mutable and merged.
        //
        // Persisted                    | Not persisted
        // [0..=5] [6..=10, parents=[3]] [11..=20, parents=[10]]
        //         [6..                       =20, parents=[3] ] <- merged
        let tmp = tempfile::tempdir()?;
        let mut iddag = IndexedLogStore::open(tmp.path())?;
        let seg1 = Segment::new(SegmentFlags::HAS_ROOT, 0, Id(0), Id(5), &[]);
        let seg2 = Segment::new(SegmentFlags::empty(), 0, Id(6), Id(10), &[Id(3)]);
        iddag.insert_segment(seg1)?;
        iddag.insert_segment(seg2)?;
        let locked = iddag.lock()?;
        iddag.persist(&locked)?;

        let seg3 = Segment::new(SegmentFlags::ONLY_HEAD, 0, Id(11), Id(20), &[Id(10)]);
        iddag.insert_segment(seg3)?;
        iddag.persist(&locked)?;

        // Reload.
        let iddag2 = IndexedLogStore::open(tmp.path())?;

        // Check the merged segments.
        assert_eq!(
            dbg_iter(iddag2.iter_segments_descending(Id(20), 0)?),
            "[H6-20[3], R0-5[]]"
        );

        // Check parent -> child index.
        // 10 -> 11 parent index wasn't inserted.
        assert_eq!(
            dbg_iter(iddag2.iter_flat_segments_with_parent(Id(10))?),
            "[]"
        );
        // 3 -> 6 parent index only returns the new segment.
        assert_eq!(
            dbg_iter(iddag2.iter_flat_segments_with_parent(Id(3))?),
            "[6-10[3]]"
        );

        // Check (level, head) -> segment index.
        // Check lookup by "including_id". Should all return the new merged segment.
        assert_eq!(
            dbg(iddag2.find_flat_segment_including_id(Id(7))?),
            "Some(H6-20[3])"
        );
        assert_eq!(
            dbg(iddag2.find_flat_segment_including_id(Id(13))?),
            "Some(H6-20[3])"
        );
        assert_eq!(
            dbg(iddag2.find_flat_segment_including_id(Id(20))?),
            "Some(H6-20[3])"
        );
        // Check lookup by head.
        // By head 20 returns the new merged segment.
        assert_eq!(
            dbg(iddag2.find_segment_by_head_and_level(Id(20), 0)?),
            "Some(H6-20[3])"
        );
        // By head 10 does not return the old segment.
        assert_eq!(
            dbg(iddag2.find_segment_by_head_and_level(Id(10), 0)?),
            "None"
        );

        Ok(())
    }

    fn dbg_iter<'a>(iter: Box<dyn Iterator<Item = Result<Segment>> + 'a>) -> String {
        let v = iter.map(|s| s.unwrap()).collect::<Vec<_>>();
        dbg(v)
    }

    fn dbg<T: std::fmt::Debug>(t: T) -> String {
        format!("{:?}", t)
    }
}
