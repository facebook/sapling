// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! # segment
//!
//! Segmented DAG. See [`Dag`] for the main structure.

use crate::spanset::Span;
use crate::spanset::SpanSet;
use byteorder::{BigEndian, ByteOrder, WriteBytesExt};
use failure::{bail, Fallible};
use fs2::FileExt;
use indexedlog::log;
use indexmap::set::IndexSet;
use std::collections::{BTreeSet, BinaryHeap};
use std::fmt::{self, Debug, Formatter};
use std::fs::{self, File};
use std::io::Cursor;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use vlqencoding::{VLQDecode, VLQDecodeAt, VLQEncode};

pub type Id = u64;
pub type Level = u8;

/// Structure to store a DAG of integers, with indexes to speed up ancestry queries.
///
/// A segment is defined as `(level: int, low: int, high: int, parents: [int])` on
/// a topo-sorted integer DAG. It covers all integers in `low..=high` range, and
/// must satisfy:
/// - `high` is the *only* head in the sub DAG covered by the segment.
/// - `parents` do not have entries within `low..=high` range.
/// - If `level` is 0, for any integer `x` in `low+1..=high` range, `x`'s parents
///   must be `x - 1`.
///
/// See `slides/201904-segmented-changelog/segmented-changelog.pdf` for pretty
/// graphs about how segments help with ancestry queries.
pub struct Dag {
    pub(crate) log: log::Log,
    path: PathBuf,
    max_level: Level,
}

/// Guard to make sure [`Dag`] on-disk writes are race-free.
pub struct SyncableDag<'a> {
    dag: &'a mut Dag,
    lock_file: File,
}

/// [`Segment`] provides access to fields of a node in a [`Dag`] graph.
/// [`Segment`] reads directly from the byte slice, without a full parsing.
pub(crate) struct Segment<'a>(pub(crate) &'a [u8]);

// Serialization format for Segment:
//
// ```plain,ignore
// SEGMENT := LEVEL (1B) + HIGH (8B) + vlq(HIGH-LOW) + vlq(PARENT_COUNT) + vlq(VLQ, PARENTS)
// ```
//
// The reason HIGH is not stored in VLQ is because it's used by range lookup,
// and vlq `[u8]` order does not match integer order.
//
// The reason HIGH-LOW is used instead of LOW is because it is more compact
// for the worse case (i.e. each flat segment has length 1). Each segment has
// only 1 byte overhead.

impl Dag {
    const INDEX_HEAD: usize = 0;
    const KEY_LEN: usize = Segment::OFFSET_DELTA;

    /// Open [`Dag`] at the given directory. Create it on demand.
    pub fn open(path: impl AsRef<Path>) -> Fallible<Self> {
        let path = path.as_ref();
        let log = log::OpenOptions::new()
            .create(true)
            .index("head", |_| {
                vec![log::IndexOutput::Reference(0..Self::KEY_LEN as u64)]
            })
            .open(path)?;
        // The first byte of the largest key is the maximum level.
        let max_level = match log.lookup_range(Self::INDEX_HEAD, ..)?.rev().nth(0) {
            None => 0,
            Some(key) => key?.0.get(0).cloned().unwrap_or(0),
        };
        Ok(Self {
            log,
            path: path.to_path_buf(),
            max_level,
        })
    }

    /// Find segment by level and head.
    pub(crate) fn find_segment_by_head(&self, head: Id, level: u8) -> Fallible<Option<Segment>> {
        let key = Self::serialize_lookup_key(head, level);
        match self.log.lookup(Self::INDEX_HEAD, &key)?.nth(0) {
            None => Ok(None),
            Some(bytes) => Ok(Some(Segment(bytes?))),
        }
    }

    /// Find segment of the specified level containing the given id.
    pub(crate) fn find_segment_including_id(&self, id: Id, level: u8) -> Fallible<Option<Segment>> {
        debug_assert_eq!(
            level, 0,
            "logic error: find_segment_by_value is only meaningful for level 0"
        );
        let low = Self::serialize_lookup_key(id, level);
        let high = [level + 1];
        let iter = self
            .log
            .lookup_range(Self::INDEX_HEAD, &low[..]..&high[..])?;
        for entry in iter {
            let (_, entries) = entry?;
            for entry in entries {
                let entry = entry?;
                let seg = Segment(entry);
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

    /// Add a new segment.
    ///
    /// For simplicity, it does not check if the new segment overlaps with
    /// an existing segment (which is a logic error). Those checks can be
    /// offline.
    pub fn insert(&mut self, level: Level, low: Id, high: Id, parents: &[Id]) -> Fallible<()> {
        let buf = Segment::serialize(level, low, high, parents);
        self.log.append(buf)?;
        Ok(())
    }

    /// Return the next unused id for segments of the specified level.
    ///
    /// Useful for building segments incrementally.
    pub fn next_free_id(&self, level: Level) -> Fallible<Id> {
        let prefix = [level];
        match self
            .log
            .lookup_prefix(Self::INDEX_HEAD, &prefix)?
            .rev()
            .nth(0)
        {
            None => Ok(0),
            Some(result) => {
                let (key, _) = result?;
                // This is an abuse of Segment. Segment expects the input buffer
                // to be a complete entry. This input buffer is the key, which is
                // the prefix of a complete entry (see `index` in `open`). However,
                // the prefix is enough to answer the "high" question.
                Ok(Segment(&key).high()? + 1)
            }
        }
    }

    /// Return a [`SyncableDag`] instance that provides race-free
    /// filesytem read and write access by taking an exclusive lock.
    ///
    /// The [`SyncableDag`] instance provides a `sync` method that
    /// actually writes changes to disk.
    ///
    /// Block if another instance is taking the lock.
    ///
    /// Panic if there are pending in-memory writes.
    pub fn prepare_filesystem_sync(&mut self) -> Fallible<SyncableDag> {
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

        Ok(SyncableDag {
            dag: self,
            lock_file,
        })
    }

    // Used internally to generate the index key for lookup
    fn serialize_lookup_key(value: Id, level: u8) -> [u8; Self::KEY_LEN] {
        let mut buf = [0u8; Self::KEY_LEN];
        {
            let mut cur = Cursor::new(&mut buf[..]);
            cur.write_u8(level).unwrap();
            cur.write_u64::<BigEndian>(value).unwrap();
            debug_assert_eq!(cur.position(), Self::KEY_LEN as u64);
        }
        buf
    }
}

// Build segments.
impl Dag {
    /// Incrementally build flat (level 0) segments towards `high` (inclusive).
    ///
    /// `get_parents` describes the DAG. Its input and output are `Id`s.
    ///
    /// `last_threshold` decides the minimal size for the last incomplete flat
    /// segment. Setting it to 0 will makes sure flat segments cover the given
    /// `high - 1`, with the downside of increasing fragmentation.  Setting it
    /// to a larger value will reduce fragmentation, with the downside of
    /// [`Dag`] covers less ids.
    ///
    /// Return number of segments inserted.
    pub fn build_flat_segments<F>(
        &mut self,
        high: Id,
        get_parents: &F,
        last_threshold: Id,
    ) -> Fallible<usize>
    where
        F: Fn(Id) -> Fallible<Vec<Id>>,
    {
        let low = self.next_free_id(0)?;
        let mut current_low = None;
        let mut current_parents = Vec::new();
        let mut insert_count = 0;
        for id in low..=high {
            let parents = get_parents(id)?;
            if parents.len() != 1 || parents[0] + 1 != id {
                // Must start a new segment.
                if let Some(low) = current_low {
                    debug_assert!(id > 0);
                    self.insert(0, low, id - 1, &current_parents)?;
                    insert_count += 1;
                }
                current_parents = parents;
                current_low = Some(id);
            }
        }

        // For the last flat segment, only build it if its length satisfies the threshold.
        if let Some(low) = current_low {
            if low + last_threshold <= high {
                self.insert(0, low, high, &current_parents)?;
                insert_count += 1;
            }
        }

        Ok(insert_count)
    }

    /// Find segments that covers `id..` range at the given level.
    fn next_segments(&self, id: Id, level: Level) -> Fallible<Vec<Segment>> {
        let lower_bound = Self::serialize_lookup_key(id, level);
        let upper_bound = [level + 1];
        let mut result = Vec::new();
        for entry in self
            .log
            .lookup_range(Self::INDEX_HEAD, (&lower_bound[..])..&upper_bound)?
        {
            let (_, values) = entry?;
            for value in values {
                result.push(Segment(value?));
            }
        }
        Ok(result)
    }

    /// Incrementally build high level segments at the given `level`.
    ///
    /// The new, high level segments are built on top of the lower level
    /// (`level - 1`) segments. Each high level segment covers at most `size`
    /// `level - 1` segments.
    ///
    /// If `drop_last` is `true`, the last segment is dropped because it's
    /// likely to be incomplete. This helps reduce fragmentation if segments
    /// are built frequently.
    ///
    /// Return number of segments inserted.
    pub fn build_high_level_segments(
        &mut self,
        level: Level,
        size: usize,
        drop_last: bool,
    ) -> Fallible<usize> {
        assert!(level > 0);

        // `get_parents` is on the previous level of segments.
        let get_parents = |head: Id| -> Fallible<Vec<Id>> {
            if let Some(seg) = self.find_segment_by_head(head, level - 1)? {
                seg.parents()
            } else {
                panic!("programming error: get_parents called with wrong head");
            }
        };

        let new_segments = {
            let low = self.next_free_id(level)?;

            // Find all segments on the previous level that haven't been built.
            let segments: Vec<_> = self.next_segments(low, level - 1)?;

            // Sanity check: They should be sorted and connected.
            for i in 1..segments.len() {
                assert_eq!(segments[i - 1].high()? + 1, segments[i].span()?.low);
            }

            // Build the graph from the first head. `low_idx` is the
            // index of `segments`.
            let find_segment = |low_idx: usize| -> Fallible<_> {
                let segment_low = segments[low_idx].span()?.low;
                let mut heads = BTreeSet::new();
                let mut parents = IndexSet::new();
                let mut candidate = None;
                for i in low_idx..segments.len().min(low_idx + size) {
                    let head = segments[i].head()?;
                    heads.insert(head);
                    let direct_parents = get_parents(head)?;
                    for p in &direct_parents {
                        if *p < segment_low {
                            // No need to remove p from heads, since it cannot be a head.
                            parents.insert(*p);
                        } else {
                            heads.remove(p);
                        }
                    }
                    if heads.len() == 1 {
                        candidate = Some((i, segment_low, head, parents.len()));
                    }
                }
                // There must be at least one valid high-level segment,
                // because `segments[low_idx]` is such a high-level segment.
                let (new_idx, low, high, parent_count) = candidate.unwrap();
                let parents = parents.into_iter().take(parent_count).collect::<Vec<Id>>();
                Ok((new_idx, low, high, parents))
            };

            let mut idx = 0;
            let mut new_segments = Vec::new();
            while idx < segments.len() {
                let segment_info = find_segment(idx)?;
                idx = segment_info.0 + 1;
                new_segments.push(segment_info);
            }

            // Drop the last segment. It could be incomplete.
            if drop_last {
                new_segments.pop();
            }

            new_segments
        };

        let insert_count = new_segments.len();

        for (_, low, high, parents) in new_segments {
            self.insert(level, low, high, &parents)?;
        }

        if level > self.max_level && insert_count > 0 {
            self.max_level = level;
        }

        Ok(insert_count)
    }
}

// Algorithms using SpanSet as output.
impl Dag {
    /// Calculate all ancestors reachable from the given `id`.
    pub fn ancestors(&self, id: Id) -> Fallible<SpanSet> {
        let mut result = SpanSet::empty();
        let mut to_visit = BinaryHeap::new();
        to_visit.push(id);

        'outer: while let Some(id) = to_visit.pop() {
            if result.contains(id) {
                continue;
            }
            for level in (0..=self.max_level).rev() {
                let seg = match level {
                    0 => self.find_segment_including_id(id, 0)?,
                    _ => self.find_segment_by_head(id, level)?,
                };
                if let Some(seg) = seg {
                    let span = (seg.span()?.low..=id).into();
                    result.push_span(span);
                    for parent in seg.parents()? {
                        to_visit.push(parent);
                    }
                    continue 'outer;
                }
            }
            panic!("logic error: flat segments are expected to cover everything but they are not");
        }

        Ok(result)
    }

    /// Calculate one "greatest common ancestor" of two `Id`s.
    ///
    /// If there are no common ancestors, return None.
    /// If there are multiple greatest common ancestors, pick one arbitrarily.
    /// Use `gca_all` to get all of them.
    pub fn gca_one(&self, a: Id, b: Id) -> Fallible<Option<Id>> {
        let set = self.ancestors(a)?.intersection(&self.ancestors(b)?);
        Ok(set.iter().nth(0))
    }

    /// Calculate all "greatest common ancestor"s of two `Id`s.
    /// `gca_one` is faster if an arbitrary answer is ok.
    pub fn gca_all(&self, a: Id, b: Id) -> Fallible<SpanSet> {
        let set = self.ancestors(a)?.intersection(&self.ancestors(b)?);
        Ok(self.heads_ancestors(set)?)
    }

    /// Test if `ancestor_id` is an ancestor of `descendant_id`.
    pub fn is_ancestor(&self, ancestor_id: Id, descendant_id: Id) -> Fallible<bool> {
        let set = self.ancestors(descendant_id)?;
        Ok(set.contains(ancestor_id))
    }

    /// Calculate "heads" of the ancestors of the given [`SpanSet`]. That is,
    /// Find Y, which is the smallest subset of set X, where `ancestors(Y)` is
    /// `ancestors(X)`.
    ///
    /// This is faster than calculating `heads(ancestors(set))`.
    ///
    /// This is different from `heads`. In case set contains X and Y, and Y is
    /// an ancestor of X, but not the immediate ancestor, `heads` will include
    /// Y while this function won't.
    pub fn heads_ancestors(&self, set: impl Into<SpanSet>) -> Fallible<SpanSet> {
        let set = set.into();
        let mut remaining = set;
        let mut result = SpanSet::empty();
        // `iter().nth(0)` returns the "largest" Id, which must be a head.
        while let Some(id) = remaining.iter().nth(0) {
            result.push_span((id..=id).into());
            // Remove ancestors reachable from that head.
            remaining = remaining.difference(&self.ancestors(id)?);
        }
        Ok(result)
    }
}

impl<'a> SyncableDag<'a> {
    /// Write pending changes to disk.
    ///
    /// This method must be called if there are new entries inserted.
    /// Otherwise [`SyncableDag`] will panic once it gets dropped.
    pub fn sync(&mut self) -> Fallible<()> {
        self.dag.log.sync()?;
        Ok(())
    }
}

impl<'a> Deref for SyncableDag<'a> {
    type Target = Dag;

    fn deref(&self) -> &Self::Target {
        self.dag
    }
}

impl<'a> DerefMut for SyncableDag<'a> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dag
    }
}

impl<'a> Drop for SyncableDag<'a> {
    fn drop(&mut self) {
        // TODO: handles `sync` failures gracefully.
        assert!(
            self.dag.log.iter_dirty().next().is_none(),
            "programming error: sync must be called before dropping WritableIdMap"
        );
    }
}

impl<'a> Segment<'a> {
    const OFFSET_LEVEL: usize = 0;
    const OFFSET_HIGH: usize = Self::OFFSET_LEVEL + 1;
    const OFFSET_DELTA: usize = Self::OFFSET_HIGH + 8;

    pub(crate) fn high(&self) -> Fallible<Id> {
        match self.0.get(Self::OFFSET_HIGH..Self::OFFSET_HIGH + 8) {
            Some(slice) => Ok(BigEndian::read_u64(slice)),
            None => bail!("cannot read high"),
        }
    }

    // high - low
    fn delta(&self) -> Fallible<Id> {
        let (len, _) = self.0.read_vlq_at(Self::OFFSET_DELTA)?;
        Ok(len)
    }

    pub(crate) fn span(&self) -> Fallible<Span> {
        let high = self.high()?;
        let delta = self.delta()?;
        let low = high - delta;
        Ok((low..=high).into())
    }

    pub(crate) fn head(&self) -> Fallible<Id> {
        self.high()
    }

    pub(crate) fn level(&self) -> Fallible<Level> {
        match self.0.get(Self::OFFSET_LEVEL) {
            Some(level) => Ok(*level),
            None => bail!("cannot read level"),
        }
    }

    pub(crate) fn parents(&self) -> Fallible<Vec<Id>> {
        let mut cur = Cursor::new(self.0);
        cur.set_position(Self::OFFSET_DELTA as u64);
        let _: u64 = cur.read_vlq()?;
        let parent_count: usize = cur.read_vlq()?;
        let mut result = Vec::with_capacity(parent_count);
        for _ in 0..parent_count {
            result.push(cur.read_vlq()?);
        }
        Ok(result)
    }

    pub(crate) fn serialize(level: Level, low: Id, high: Id, parents: &[Id]) -> Vec<u8> {
        assert!(high >= low);
        let mut buf = Vec::with_capacity(1 + 8 + (parents.len() + 2) * 4);
        buf.write_u8(level).unwrap();
        buf.write_u64::<BigEndian>(high).unwrap();
        buf.write_vlq(high - low).unwrap();
        buf.write_vlq(parents.len()).unwrap();
        for parent in parents {
            buf.write_vlq(*parent).unwrap();
        }
        buf
    }
}

impl Debug for Dag {
    fn fmt(&self, f: &mut Formatter) -> Result<(), fmt::Error> {
        let mut first = true;
        let mut last_level = 255;
        let mut segments = self
            .log
            .iter()
            .map(|e| Segment(e.unwrap()))
            .collect::<Vec<_>>();
        segments.sort_by_key(|s| (s.level().unwrap(), s.head().unwrap()));

        for segment in segments {
            let span = segment.span().unwrap();
            let level = segment.level().unwrap();
            if level != last_level {
                if !first {
                    write!(f, "\n")?;
                }
                first = false;
                write!(f, "Lv{}: ", level)?;
                last_level = level;
            } else {
                write!(f, " ")?;
            }
            write!(
                f,
                "{}-{}{:?}",
                span.low,
                span.high,
                segment.parents().unwrap()
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;
    use tempfile::tempdir;

    #[test]
    fn test_segment_roundtrip() {
        fn prop(level: Level, low: Id, delta: Id, parents: Vec<Id>) -> bool {
            let high = low + delta;
            let buf = Segment::serialize(level, low, high, &parents);
            let node = Segment(&buf);
            node.level().unwrap() == level
                && node.span().unwrap() == (low..=high).into()
                && node.parents().unwrap() == parents
        }
        quickcheck(prop as fn(Level, Id, Id, Vec<Id>) -> bool);
    }

    #[test]
    fn test_segment_basic_lookups() {
        let dir = tempdir().unwrap();
        let mut dag = Dag::open(dir.path()).unwrap();
        assert_eq!(dag.next_free_id(0).unwrap(), 0);
        assert_eq!(dag.next_free_id(1).unwrap(), 0);

        let mut dag = dag.prepare_filesystem_sync().unwrap();

        dag.insert(0, 0, 50, &vec![]).unwrap();
        assert_eq!(dag.next_free_id(0).unwrap(), 51);
        dag.insert(0, 51, 100, &vec![50]).unwrap();
        assert_eq!(dag.next_free_id(0).unwrap(), 101);
        dag.insert(0, 101, 150, &vec![100]).unwrap();
        assert_eq!(dag.next_free_id(0).unwrap(), 151);
        assert_eq!(dag.next_free_id(1).unwrap(), 0);
        dag.insert(1, 0, 100, &vec![]).unwrap();
        assert_eq!(dag.next_free_id(1).unwrap(), 101);
        dag.insert(1, 101, 150, &vec![100]).unwrap();
        assert_eq!(dag.next_free_id(1).unwrap(), 151);
        dag.sync().unwrap();

        // Helper functions to make the below lines shorter.
        let low_by_head = |head, level| match dag.find_segment_by_head(head, level) {
            Ok(Some(seg)) => seg.span().unwrap().low as i64,
            Ok(None) => -1,
            _ => panic!("unexpected error"),
        };

        let low_by_id = |id, level| match dag.find_segment_including_id(id, level) {
            Ok(Some(seg)) => seg.span().unwrap().low as i64,
            Ok(None) => -1,
            _ => panic!("unexpected error"),
        };

        assert_eq!(low_by_head(0, 0), -1);
        assert_eq!(low_by_head(49, 0), -1);
        assert_eq!(low_by_head(50, 0), 0);
        assert_eq!(low_by_head(51, 0), -1);
        assert_eq!(low_by_head(150, 0), 101);
        assert_eq!(low_by_head(100, 1), 0);

        assert_eq!(low_by_id(0, 0), 0);
        assert_eq!(low_by_id(30, 0), 0);
        assert_eq!(low_by_id(49, 0), 0);
        assert_eq!(low_by_id(50, 0), 0);
        assert_eq!(low_by_id(51, 0), 51);
        assert_eq!(low_by_id(52, 0), 51);
        assert_eq!(low_by_id(99, 0), 51);
        assert_eq!(low_by_id(100, 0), 51);
        assert_eq!(low_by_id(101, 0), 101);
        assert_eq!(low_by_id(102, 0), 101);
        assert_eq!(low_by_id(149, 0), 101);
        assert_eq!(low_by_id(150, 0), 101);
        assert_eq!(low_by_id(151, 0), -1);
    }
}
