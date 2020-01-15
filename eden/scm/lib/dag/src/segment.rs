/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # segment
//!
//! Segmented DAG. See [`IdDag`] for the main structure.
//!
//! There are 2 flavors of DAG: [`IdDag`] and [`SyncableIdDag`]. [`IdDag`] loads
//! from the filesystem, is responsible for all kinds of queires, and can
//! have in-memory-only changes. [`SyncableIdDag`] is the only way to update
//! the filesystem state, and does not support queires.

use crate::id::{Group, Id};
use crate::spanset::Span;
use crate::spanset::SpanSet;
use anyhow::{bail, ensure, format_err, Result};
use bitflags::bitflags;
use byteorder::{BigEndian, ByteOrder, WriteBytesExt};
use fs2::FileExt;
use indexedlog::log;
use indexmap::set::IndexSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::{BTreeSet, BinaryHeap};
use std::fmt::{self, Debug, Formatter};
use std::fs::{self, File};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use vlqencoding::{VLQDecode, VLQDecodeAt, VLQEncode};

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
///
/// [`IdDag`] is often used together with [`IdMap`] to allow customized names
/// on vertexes. The [`NameDag`] type provides an easy-to-use interface to
/// keep [`IdDag`] and [`IdMap`] in sync.
pub struct IdDag {
    pub(crate) log: log::Log,
    path: PathBuf,
    max_level: Level,
    new_seg_size: usize,
}

/// Guard to make sure [`IdDag`] on-disk writes are race-free.
pub struct SyncableIdDag {
    dag: IdDag,
    lock_file: File,
}

/// [`Segment`] provides access to fields of a node in a [`IdDag`] graph.
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

impl IdDag {
    const INDEX_LEVEL_HEAD: usize = 0;
    const INDEX_PARENT: usize = 1;
    const KEY_LEVEL_HEAD_LEN: usize = Segment::OFFSET_DELTA - Segment::OFFSET_LEVEL;

    /// Magic bytes in `Log` that indicates "remove all non-master segments".
    /// A Segment entry has at least KEY_LEVEL_HEAD_LEN (9) bytes so it does
    /// not conflict with this.
    const MAGIC_CLEAR_NON_MASTER: &'static [u8] = b"CLRNM";

    /// Open [`IdDag`] at the given directory. Create it on demand.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let log = log::OpenOptions::new()
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
                let seg = Segment(data);
                let mut result = Vec::new();
                if seg.level().ok() == Some(0) {
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
            .open(path)?;
        let max_level = Self::max_level_from_log(&log)?;
        let mut dag = Self {
            log,
            path: path.to_path_buf(),
            max_level,
            new_seg_size: 16, // see D16660078 for this default setting
        };
        dag.build_all_high_level_segments(false)?;
        Ok(dag)
    }

    fn max_level_from_log(log: &log::Log) -> Result<Level> {
        // The first byte of the largest key is the maximum level.
        let max_level = match log.lookup_range(Self::INDEX_LEVEL_HEAD, ..)?.rev().nth(0) {
            None => 0,
            Some(key) => key?.0.get(0).cloned().unwrap_or(0),
        };
        Ok(max_level)
    }

    /// Find segment by level and head.
    fn find_segment_by_head_and_level(&self, head: Id, level: u8) -> Result<Option<Segment>> {
        let key = Self::serialize_head_level_lookup_key(head, level);
        match self.log.lookup(Self::INDEX_LEVEL_HEAD, &key)?.nth(0) {
            None => Ok(None),
            Some(bytes) => Ok(Some(Segment(bytes?))),
        }
    }

    /// Find flat segment containing the given id.
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
    pub fn insert(
        &mut self,
        flags: SegmentFlags,
        level: Level,
        low: Id,
        high: Id,
        parents: &[Id],
    ) -> Result<()> {
        let buf = Segment::serialize(flags, level, low, high, parents);
        self.log.append(buf)?;
        Ok(())
    }

    /// Return the next unused id for segments of the specified level.
    ///
    /// Useful for building segments incrementally.
    pub fn next_free_id(&self, level: Level, group: Group) -> Result<Id> {
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
                    let seg = Segment(bytes?);
                    Ok(seg.high()? + 1)
                } else {
                    bail!("key {:?} should have some values", key);
                }
            }
        }
    }

    /// Return a [`SyncableIdDag`] instance that provides race-free
    /// filesytem read and write access by taking an exclusive lock.
    ///
    /// The [`SyncableIdDag`] instance provides a `sync` method that
    /// actually writes changes to disk.
    ///
    /// Block if another instance is taking the lock.
    pub fn prepare_filesystem_sync(&self) -> Result<SyncableIdDag> {
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

        // Clone. But drop in-memory data.
        let mut log = self.log.try_clone_without_dirty()?;

        // Read new entries from filesystem.
        log.sync()?;
        let max_level = Self::max_level_from_log(&log)?;

        Ok(SyncableIdDag {
            dag: IdDag {
                log,
                path: self.path.clone(),
                max_level,
                new_seg_size: self.new_seg_size,
            },
            lock_file,
        })
    }

    /// Set the maximum size of a new high-level segment.
    ///
    /// This does not affect existing segments.
    ///
    /// This might help performance a bit for certain rare types of DAGs.
    /// The default value is Usually good enough.
    pub fn set_new_segment_size(&mut self, size: usize) {
        self.new_seg_size = size.max(2);
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
}

// Build segments.
impl IdDag {
    /// Make sure the [`IdDag`] contains the given id (and all ids smaller than
    /// `high`) by building up segments on demand.
    ///
    /// `get_parents` describes the DAG. Its input and output are `Id`s.
    ///
    /// This is often used together with [`crate::idmap::IdMap`].
    ///
    /// Content inserted by this function *will not* be written to disk.
    /// For example, [`IdDag::prepare_filesystem_sync`] will drop them.
    pub fn build_segments_volatile<F>(&mut self, high: Id, get_parents: &F) -> Result<usize>
    where
        F: Fn(Id) -> Result<Vec<Id>>,
    {
        let mut count = 0;
        count += self.build_flat_segments(high, get_parents, 0)?;
        if self.next_free_id(0, high.group())? <= high {
            bail!("internal error: flat segments are not built as expected");
        }
        count += self.build_all_high_level_segments(false)?;
        Ok(count)
    }

    /// Incrementally build flat (level 0) segments towards `high` (inclusive).
    ///
    /// `get_parents` describes the DAG. Its input and output are `Id`s.
    ///
    /// `last_threshold` decides the minimal size for the last incomplete flat
    /// segment. Setting it to 0 will makes sure flat segments cover the given
    /// `high - 1`, with the downside of increasing fragmentation.  Setting it
    /// to a larger value will reduce fragmentation, with the downside of
    /// [`IdDag`] covers less ids.
    ///
    /// Return number of segments inserted.
    fn build_flat_segments<F>(
        &mut self,
        high: Id,
        get_parents: &F,
        last_threshold: u64,
    ) -> Result<usize>
    where
        F: Fn(Id) -> Result<Vec<Id>>,
    {
        let group = high.group();
        let low = self.next_free_id(0, group)?;
        let mut current_low = None;
        let mut current_parents = Vec::new();
        let mut insert_count = 0;
        let mut head_ids: HashSet<Id> = if group == Group::MASTER && low > Id::MIN {
            self.heads(Id::MIN..=(low - 1))?.iter().collect()
        } else {
            Default::default()
        };
        let mut get_flags = |parents: &Vec<Id>, head: Id| {
            let mut flags = SegmentFlags::empty();
            if parents.is_empty() {
                flags |= SegmentFlags::HAS_ROOT
            }
            if group == Group::MASTER {
                head_ids = &head_ids - &parents.iter().cloned().collect();
                if head_ids.is_empty() {
                    flags |= SegmentFlags::ONLY_HEAD;
                }
                head_ids.insert(head);
            }
            flags
        };
        for id in low.to(high) {
            let parents = get_parents(id)?;
            if parents.len() != 1 || parents[0] + 1 != id || current_low.is_none() {
                // Must start a new segment.
                if let Some(low) = current_low {
                    debug_assert!(id > Id::MIN);
                    let flags = get_flags(&current_parents, id - 1);
                    self.insert(flags, 0, low, id - 1, &current_parents)?;
                    insert_count += 1;
                }
                current_parents = parents;
                current_low = Some(id);
            }
        }

        // For the last flat segment, only build it if its length satisfies the threshold.
        if let Some(low) = current_low {
            if low + last_threshold <= high {
                let flags = get_flags(&current_parents, high);
                self.insert(flags, 0, low, high, &current_parents)?;
                insert_count += 1;
            }
        }

        Ok(insert_count)
    }

    /// Find segments that covers `id..` range at the given level, within a same group.
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
                result.push(Segment(value?));
            }
        }
        Ok(result)
    }

    /// Iterate through segments at the given level in descending order.
    fn iter_segments_descending(
        &self,
        max_high_id: Id,
        level: Level,
    ) -> Result<impl Iterator<Item = Result<Segment>>> {
        let lower_bound = Self::serialize_head_level_lookup_key(Id::MIN, level);
        let upper_bound = Self::serialize_head_level_lookup_key(max_high_id, level);
        let iter = self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, &lower_bound[..]..=&upper_bound[..])?
            .rev();
        let iter = iter.flat_map(|entry| match entry {
            Ok((_key, values)) => values
                .into_iter()
                .map(|value| {
                    let value = value?;
                    Ok(Segment(value))
                })
                .collect(),
            Err(err) => vec![Err(err.into())],
        });
        Ok(iter)
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
    fn build_high_level_segments(&mut self, level: Level, drop_last: bool) -> Result<usize> {
        ensure!(level > 0, "build_high_level_segments requires level > 0");
        let size = self.new_seg_size;

        let mut insert_count = 0;
        for &group in Group::ALL.iter() {
            // `get_parents` is on the previous level of segments.
            let get_parents = |head: Id| -> Result<Vec<Id>> {
                if let Some(seg) = self.find_segment_by_head_and_level(head, level - 1)? {
                    seg.parents()
                } else {
                    bail!("programming error: get_parents called with wrong head");
                }
            };

            let new_segments = {
                let low = self.next_free_id(level, group)?;

                // Find all segments on the previous level that haven't been built.
                let segments: Vec<_> = self.next_segments(low, level - 1)?;

                // Sanity check: They should be sorted and connected.
                for i in 1..segments.len() {
                    if segments[i - 1].high()? + 1 != segments[i].span()?.low {
                        bail!(
                            "level {} segments {:?} are not sorted or connected!",
                            level,
                            &segments[i - 1..=i]
                        );
                    }
                }

                // Build the graph from the first head. `low_idx` is the
                // index of `segments`.
                let find_segment = |low_idx: usize| -> Result<_> {
                    let segment_low = segments[low_idx].span()?.low;
                    let mut heads = BTreeSet::new();
                    let mut parents = IndexSet::new();
                    let mut candidate = None;
                    let mut has_root = false;
                    for i in low_idx..segments.len().min(low_idx + size) {
                        let head = segments[i].head()?;
                        if !has_root && segments[i].has_root()? {
                            has_root = true;
                        }
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
                            candidate = Some((i, segment_low, head, parents.len(), has_root));
                        }
                    }
                    // There must be at least one valid high-level segment,
                    // because `segments[low_idx]` is such a high-level segment.
                    let (new_idx, low, high, parent_count, has_root) = candidate.unwrap();
                    let parents = parents.into_iter().take(parent_count).collect::<Vec<Id>>();
                    Ok((new_idx, low, high, parents, has_root))
                };

                let mut idx = 0;
                let mut new_segments = Vec::new();
                while idx < segments.len() {
                    let segment_info = find_segment(idx)?;
                    idx = segment_info.0 + 1;
                    new_segments.push(segment_info);
                }

                // No point to introduce new levels if it has the same segment count
                // as the lower level.
                if segments.len() == new_segments.len()
                    && self.max_level < level
                    && group == Group::MASTER
                {
                    return Ok(0);
                }

                // Drop the last segment. It could be incomplete.
                if drop_last {
                    new_segments.pop();
                }

                new_segments
            };

            insert_count += new_segments.len();

            for (_, low, high, parents, has_root) in new_segments {
                let flags = if has_root {
                    SegmentFlags::HAS_ROOT
                } else {
                    SegmentFlags::empty()
                };
                self.insert(flags, level, low, high, &parents)?;
            }
        }

        if level > self.max_level && insert_count > 0 {
            self.max_level = level;
        }

        Ok(insert_count)
    }

    /// Build high level segments using default setup.
    ///
    /// If `drop_last` is `true`, the last segment is dropped to help
    /// reduce fragmentation.
    ///
    /// Return number of segments inserted.
    fn build_all_high_level_segments(&mut self, drop_last: bool) -> Result<usize> {
        let mut total = 0;
        for level in 1.. {
            let count = self.build_high_level_segments(level, drop_last)?;
            if count == 0 {
                break;
            }
            total += count;
        }
        Ok(total)
    }
}

// Reload.
impl IdDag {
    /// Reload from the filesystem. Discard pending changes.
    pub fn reload(&mut self) -> Result<()> {
        self.log.clear_dirty()?;
        self.log.sync()?;
        self.max_level = Self::max_level_from_log(&self.log)?;
        self.build_all_high_level_segments(false)?;
        Ok(())
    }
}

// Remove data.
impl IdDag {
    /// Mark non-master ids as "removed".
    pub fn remove_non_master(&mut self) -> Result<()> {
        self.log.append(Self::MAGIC_CLEAR_NON_MASTER)?;
        for level in 0..=self.max_level {
            ensure!(
                self.next_free_id(level, Group::NON_MASTER)? == Group::NON_MASTER.min_id(),
                "bug: remove_non_master did not take effect"
            );
        }
        Ok(())
    }
}

// User-facing DAG-related algorithms.
impl IdDag {
    /// Return a [`SpanSet`] that covers all ids stored in this [`IdDag`].
    pub fn all(&self) -> Result<SpanSet> {
        let mut result = SpanSet::empty();
        for &group in Group::ALL.iter().rev() {
            let next = self.next_free_id(0, group)?;
            if next > group.min_id() {
                result.push(group.min_id()..=(next - 1));
            }
        }
        Ok(result)
    }

    /// Return a [`SpanSet`] that covers all ids stored in the master group.
    pub(crate) fn master_group(&self) -> Result<SpanSet> {
        let group = Group::MASTER;
        let next = self.next_free_id(0, group)?;
        if next > group.min_id() {
            Ok((group.min_id()..=(next - 1)).into())
        } else {
            Ok(SpanSet::empty())
        }
    }

    /// Calculate all ancestors reachable from any id from the given set.
    ///
    /// ```plain,ignore
    /// union(ancestors(i) for i in set)
    /// ```
    pub fn ancestors(&self, set: impl Into<SpanSet>) -> Result<SpanSet> {
        let mut set: SpanSet = set.into();
        if set.count() > 2 {
            // Try to (greatly) reduce the size of the `set` to make calculation cheaper.
            set = self.heads_ancestors(set)?;
        }
        let mut result = SpanSet::empty();
        let mut to_visit: BinaryHeap<_> = set.iter().collect();
        'outer: while let Some(id) = to_visit.pop() {
            if result.contains(id) {
                // If `id` is in `result`, then `ancestors(id)` are all in `result`.
                continue;
            }
            let flat_seg = self.find_flat_segment_including_id(id)?;
            if let Some(ref s) = flat_seg {
                if s.only_head()? {
                    // Fast path.
                    result.push_span((Id::MIN..=id).into());
                    break 'outer;
                }
            }
            for level in (1..=self.max_level).rev() {
                let seg = self.find_segment_by_head_and_level(id, level)?;
                if let Some(seg) = seg {
                    let span = seg.span()?.into();
                    result.push_span(span);
                    for parent in seg.parents()? {
                        to_visit.push(parent);
                    }
                    continue 'outer;
                }
            }
            if let Some(seg) = flat_seg {
                let span = (seg.span()?.low..=id).into();
                result.push_span(span);
                for parent in seg.parents()? {
                    to_visit.push(parent);
                }
            } else {
                bail!(
                    "logic error: flat segments are expected to cover everything but they are not"
                );
            }
        }

        Ok(result)
    }

    /// Calculate parents of the given set.
    ///
    /// Note: [`SpanSet`] does not preserve order. Use [`IdDag::parent_ids`] if
    /// order is needed.
    pub fn parents(&self, set: impl Into<SpanSet>) -> Result<SpanSet> {
        let mut result = SpanSet::empty();
        let mut set = set.into();

        'outer: while let Some(head) = set.max() {
            // For high-level segments. If the set covers the entire segment, then
            // the parents is (the segment - its head + its parents).
            for level in (1..=self.max_level).rev() {
                if let Some(seg) = self.find_segment_by_head_and_level(head, level)? {
                    let seg_span = seg.span()?;
                    if set.contains(seg_span) {
                        let seg_set = SpanSet::from(seg_span);
                        let mut parent_set = seg_set.difference(&head.into());
                        parent_set.push_set(&SpanSet::from_spans(seg.parents()?));
                        set = set.difference(&seg_set);
                        result = result.union(&parent_set);
                        continue 'outer;
                    }
                }
            }

            // A flat segment contains information to calculate
            // parents(subset of the segment).
            let seg = self.find_flat_segment_including_id(head)?.expect(
                "logic error: flat segments are expected to cover everything but they do not",
            );
            let seg_span = seg.span()?;
            let seg_low = seg_span.low;
            let seg_set: SpanSet = seg_span.into();
            let seg_set = seg_set.intersection(&set);

            // Get parents for a linear set (ex. parent(i) is (i - 1)).
            fn parents_linear(set: &SpanSet) -> SpanSet {
                debug_assert!(!set.contains(Id::MIN));
                SpanSet::from_sorted_spans(set.as_spans().iter().map(|s| s.low - 1..=s.high - 1))
            }

            let parent_set = if seg_set.contains(seg_low) {
                let mut parent_set = parents_linear(&seg_set.difference(&SpanSet::from(seg_low)));
                parent_set.push_set(&SpanSet::from_spans(seg.parents()?));
                parent_set
            } else {
                parents_linear(&seg_set)
            };

            set = set.difference(&seg_set);
            result = result.union(&parent_set);
        }

        Ok(result)
    }

    /// Get parents of a single `id`. Preserve the order.
    pub fn parent_ids(&self, id: Id) -> Result<Vec<Id>> {
        let seg = self
            .find_flat_segment_including_id(id)?
            .expect("logic error: flat segments are expected to cover everything but they do not");
        let span = seg.span()?;
        if id == span.low {
            Ok(seg.parents()?)
        } else {
            Ok(vec![id - 1])
        }
    }

    /// Calculate the n-th first ancestor. If `n` is 0, return `id` unchanged.
    /// If `n` is 1, return the first parent of `id`.
    pub fn first_ancestor_nth(&self, mut id: Id, mut n: u64) -> Result<Id> {
        // PERF: this can have fast paths from high-level segments if high-level
        // segments have extra information.
        while n > 0 {
            let seg = self
                .find_flat_segment_including_id(id)?
                .ok_or_else(|| format_err!("id {} is not covered by dag", id))?;
            // segment: low ... id ... high
            //          \________/
            //            delta
            let low = seg.span()?.low;
            let delta = id.0 - low.0;
            let step = delta.min(n);
            id = id - step;
            n -= step;
            if n > 0 {
                // Follow the first parent.
                id = *seg
                    .parents()?
                    .get(0)
                    .ok_or_else(|| format_err!("{}~{} cannot be resolved - no parents", &id, n))?;
                n -= 1;
            }
        }
        Ok(id)
    }

    /// Convert an `id` to `x~n` form with the given constraint.
    ///
    /// Return `None` if the conversion can not be done with the constraints.
    pub fn to_first_ancestor_nth(
        &self,
        id: Id,
        constraint: FirstAncestorConstraint,
    ) -> Result<Option<(Id, u64)>> {
        match constraint {
            FirstAncestorConstraint::None => Ok(Some((id, 0))),
            FirstAncestorConstraint::KnownUniversally { heads } => {
                self.to_first_ancestor_nth_known_universally(id, heads)
            }
        }
    }

    /// See `FirstAncestorConstraint::KnownUniversally`.
    ///
    /// Return `None` if `id` is not part of `ancestors(heads)`.
    fn to_first_ancestor_nth_known_universally(
        &self,
        mut id: Id,
        heads: SpanSet,
    ) -> Result<Option<(Id, u64)>> {
        let ancestors = self.ancestors(heads.clone())?;
        if !ancestors.contains(id) {
            return Ok(None);
        }

        let mut n = 0;
        let result = 'outer: loop {
            let seg = self
                .find_flat_segment_including_id(id)?
                .ok_or_else(|| format_err!("{} is not covered by segments", id))?;
            let head = seg.head()?;
            // Can we use an `id` from `heads` as `x`?
            let intersected = heads.intersection(&(id..=head).into());
            if !intersected.is_empty() {
                let head = intersected.min().unwrap();
                n += head.0 - id.0;
                break 'outer (head, n);
            }
            // Can we use `head` in `seg` as `x`?
            let mut next_id = None;
            let mut key = Vec::with_capacity(8);
            key.write_vlq(head.0).expect("write to Vec should not fail");
            for seg_bytes in self.log.lookup(Self::INDEX_PARENT, &key)? {
                let child_seg = Segment(seg_bytes?);
                if child_seg.parents()?.len() > 1 {
                    // `child_seg.span().low` is a merge, so `head` is a parent of a merge.
                    // Therefore `head` can be used as `x`.
                    n += head.0 - id.0;
                    break 'outer (head, n);
                }
                let child_low = child_seg.span()?.low;
                if ancestors.contains(child_low) {
                    next_id = Some(child_low);
                }
            }
            match next_id {
                // This should not happen if indexes and segments are legit.
                None => bail!("internal error: cannot convert {} to x~n form", id),
                Some(next_id) => {
                    n += head.0 - id.0 + 1;
                    id = next_id;
                }
            }
        };
        Ok(Some(result))
    }

    /// Calculate heads of the given set.
    pub fn heads(&self, set: impl Into<SpanSet>) -> Result<SpanSet> {
        let set = set.into();
        Ok(set.difference(&self.parents(set.clone())?))
    }

    /// Calculate children of the given set.
    pub fn children(&self, set: impl Into<SpanSet>) -> Result<SpanSet> {
        let set = set.into();

        // The algorithm works as follows:
        // - Iterate through level N segments [1].
        // - Considering a level N segment S:
        //   Could we take the entire S?
        //     - If `set` covers `S - S.head + S.parents`, then yes, take S
        //       and continue with the next level N segment.
        //   Could we ignore the entire S and check the next level N segment?
        //     - If (S + S.parents) do not overlap with `set`, then yes, skip.
        //   No fast paths. Is S a flat segment?
        //     - No:  Iterate through level N-1 segments covered by S,
        //            recursively (goto [1]).
        //     - Yes: Figure out children in the flat segment.
        //            Push them to the result.

        struct Context<'a> {
            this: &'a IdDag,
            set: SpanSet,
            result_lower_bound: Id,
            result: SpanSet,
        }

        fn visit_segments(ctx: &mut Context, range: Span, level: Level) -> Result<()> {
            for seg in ctx.this.iter_segments_descending(range.high, level)? {
                let seg = seg?;
                let span = seg.span()?;
                if span.low < range.low || span.high < ctx.result_lower_bound {
                    break;
                }

                let parents = seg.parents()?;

                // Count of parents overlapping with `set`.
                let overlapped_parents = parents.iter().filter(|p| ctx.set.contains(**p)).count();

                // Remove the `high`. This segment cannot calculate
                // `children(high)`. If `high` matches a `parent` of
                // another segment, that segment will handle it.
                let intersection = ctx
                    .set
                    .intersection(&span.into())
                    .difference(&span.high.into());

                if !seg.has_root()? {
                    // A segment must have at least one parent to be rootless.
                    debug_assert!(!parents.is_empty());
                    // Fast path: Take the segment directly.
                    if overlapped_parents == parents.len()
                        && intersection.count() + 1 == span.count()
                    {
                        ctx.result.push_span(span);
                        continue;
                    }
                }

                if !intersection.is_empty() {
                    if level > 0 {
                        visit_segments(ctx, span, level - 1)?;
                        continue;
                    } else {
                        let seg_children = SpanSet::from_spans(
                            intersection
                                .as_spans()
                                .iter()
                                .map(|s| s.low + 1..=s.high + 1),
                        );
                        ctx.result.push_set(&seg_children);
                    }
                }

                if overlapped_parents > 0 {
                    if level > 0 {
                        visit_segments(ctx, span, level - 1)?;
                    } else {
                        // child(any parent) = lowest id in this flag segment.
                        ctx.result.push_span(span.low.into());
                    }
                }
            }
            Ok(())
        }

        let result_lower_bound = set.min().unwrap_or(Id::MAX);
        let mut ctx = Context {
            this: self,
            set,
            result_lower_bound,
            result: SpanSet::empty(),
        };

        visit_segments(&mut ctx, (Id::MIN..=Id::MAX).into(), self.max_level)?;
        Ok(ctx.result)
    }

    /// Calculate roots of the given set.
    pub fn roots(&self, set: impl Into<SpanSet>) -> Result<SpanSet> {
        let set = set.into();
        Ok(set.difference(&self.children(set.clone())?))
    }

    /// Calculate one "greatest common ancestor" of the given set.
    ///
    /// If there are no common ancestors, return None.
    /// If there are multiple greatest common ancestors, pick one arbitrarily.
    /// Use `gca_all` to get all of them.
    pub fn gca_one(&self, set: impl Into<SpanSet>) -> Result<Option<Id>> {
        let set = set.into();
        // The set is sorted in DESC order. Therefore its first item can be used as the result.
        Ok(self.common_ancestors(set)?.max())
    }

    /// Calculate all "greatest common ancestor"s of the given set.
    /// `gca_one` is faster if an arbitrary answer is ok.
    pub fn gca_all(&self, set: impl Into<SpanSet>) -> Result<SpanSet> {
        let set = set.into();
        self.heads_ancestors(self.common_ancestors(set)?)
    }

    /// Calculate all common ancestors of the given set.
    ///
    /// ```plain,ignore
    /// intersect(ancestors(i) for i in set)
    /// ```
    pub fn common_ancestors(&self, set: impl Into<SpanSet>) -> Result<SpanSet> {
        let set = set.into();
        let result = match set.count() {
            0 => set,
            1 => self.ancestors(set)?,
            2 => {
                // Fast path that does not calculate "heads".
                let mut iter = set.iter();
                let a = iter.next().unwrap();
                let b = iter.next().unwrap();
                self.ancestors(a)?.intersection(&self.ancestors(b)?)
            }
            _ => {
                // Try to reduce the size of `set`.
                // `common_ancestors(X)` = `common_ancestors(roots(X))`.
                let set = self.roots(set)?;
                set.iter()
                    .fold(Ok(SpanSet::full()), |set: Result<SpanSet>, id| {
                        Ok(set?.intersection(&self.ancestors(id)?))
                    })?
            }
        };
        Ok(result)
    }

    /// Test if `ancestor_id` is an ancestor of `descendant_id`.
    pub fn is_ancestor(&self, ancestor_id: Id, descendant_id: Id) -> Result<bool> {
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
    pub fn heads_ancestors(&self, set: impl Into<SpanSet>) -> Result<SpanSet> {
        let set = set.into();
        let mut remaining = set;
        let mut result = SpanSet::empty();
        while let Some(id) = remaining.max() {
            result.push_span((id..=id).into());
            // Remove ancestors reachable from that head.
            remaining = remaining.difference(&self.ancestors(id)?);
        }
        Ok(result)
    }

    /// Calculate the "dag range" - ids reachable from both sides.
    ///
    /// ```plain,ignore
    /// intersect(ancestors(heads), descendants(roots))
    /// ```
    pub fn range(&self, roots: impl Into<SpanSet>, heads: impl Into<SpanSet>) -> Result<SpanSet> {
        // Pre-calculate ancestors.
        let ancestors = self.ancestors(heads)?;
        let roots = roots.into();

        if ancestors.is_empty() || roots.is_empty() {
            return Ok(SpanSet::empty());
        }

        // The problem then becomes:
        // Given `roots`, find `ancestors & descendants(roots)`.
        //
        // The algorithm is divide and conquer:
        // - Iterate through level N segments [1].
        // - Considering a level N segment S:
        //   Can we ignore the entire segment and go check next?
        //      - Is `intersect(S, ancestors)` empty?
        //      - Are `roots` unreachable from `S.head`?
        //        (i.e. `interact(ancestors(S.head), roots)` is empty)
        //      If either is "yes", then skip and continue.
        //   Can we add the entire segment to result?
        //      - Is S.head part of `ancestors`?
        //      - Is S rootless, and all of `S.parents` can reach `roots`?
        //      If both are "yes", then take S and continue.
        //   If above fast paths do not work, then go deeper:
        //     - Iterate through level N-1 segments covered by S.

        struct Context<'a> {
            this: &'a IdDag,
            roots: SpanSet,
            ancestors: SpanSet,
            roots_min: Id,
            ancestors_max: Id,
            result: SpanSet,
        }

        fn visit_segments(ctx: &mut Context, range: Span, level: Level) -> Result<()> {
            for seg in ctx.this.iter_segments_descending(range.high, level)? {
                let seg = seg?;
                let span = seg.span()?;
                if span.low < range.low {
                    break;
                }

                // Skip this segment entirely?
                let intersection = ctx.ancestors.intersection(&span.into());
                if span.low > ctx.ancestors_max
                    || span.high < ctx.roots_min
                    || intersection.is_empty()
                    || ctx
                        .this
                        .ancestors(span.high)?
                        .intersection(&ctx.roots)
                        .is_empty()
                {
                    continue;
                }

                // Include the entire segment?
                let parents = seg.parents()?;
                let mut overlapped_parents = LazyPredicate::new(parents, |p| {
                    Ok(!ctx.this.ancestors(p)?.intersection(&ctx.roots).is_empty())
                });

                if !seg.has_root()?
                    && ctx.ancestors.contains(span.high)
                    && overlapped_parents.all()?
                {
                    ctx.result.push_span(span);
                    continue;
                }

                if level == 0 {
                    // Figure out what subset of this flat segment to be added to `result`.
                    let span_low = if overlapped_parents.any()? {
                        span.low
                    } else {
                        // Because
                        // - This is a flat segment.
                        //   i.e. only span.low has parents outside span.
                        // - Tested above: ancestors(seg.head).intersection(roots) is not empty.
                        //   i.e. descendants(roots).intersection(seg.head) is not empty.
                        // - Tested just now: no parents reach any of roots.
                        // Therefore: intersect(roots, span) cannot be empty.
                        ctx.roots.intersection(&span.into()).min().unwrap()
                    };
                    let span_high = intersection.max().unwrap();
                    if span_high >= span_low {
                        ctx.result.push_span(Span::from(span_low..=span_high));
                    }
                } else {
                    // Go deeper.
                    visit_segments(ctx, span, level - 1)?;
                }
            }
            Ok(())
        }

        let roots_min = roots.min().unwrap();
        let ancestors_max = ancestors.max().unwrap();
        let mut ctx = Context {
            this: self,
            roots,
            ancestors,
            roots_min,
            ancestors_max,
            result: SpanSet::empty(),
        };

        if ctx.roots_min <= ctx.ancestors_max {
            visit_segments(&mut ctx, (Id::MIN..=Id::MAX).into(), self.max_level)?;
        }
        Ok(ctx.result)
    }

    /// Calculate the descendants of the given set.
    ///
    /// Logically equivalent to `range(set, all())`.
    pub fn descendants(&self, set: impl Into<SpanSet>) -> Result<SpanSet> {
        // The algorithm is a manually "inlined" version of `range` where `ancestors`
        // is known to be `all()`.

        let roots = set.into();
        if roots.is_empty() {
            return Ok(SpanSet::empty());
        }

        struct Context<'a> {
            this: &'a IdDag,
            roots: SpanSet,
            roots_min: Id,
            result: SpanSet,
        }

        fn visit_segments(ctx: &mut Context, range: Span, level: Level) -> Result<()> {
            for seg in ctx.this.iter_segments_descending(range.high, level)? {
                let seg = seg?;
                let span = seg.span()?;
                if span.low < range.low || span.high < ctx.roots_min {
                    break;
                }

                // Skip this segment entirely?
                if ctx
                    .this
                    .ancestors(span.high)?
                    .intersection(&ctx.roots)
                    .is_empty()
                {
                    continue;
                }

                // Include the entire segment?
                let parents = seg.parents()?;
                let mut overlapped_parents = LazyPredicate::new(parents, |p| {
                    Ok(!ctx.this.ancestors(p)?.intersection(&ctx.roots).is_empty())
                });
                if !seg.has_root()? && overlapped_parents.all()? {
                    ctx.result.push_span(span);
                    continue;
                }

                if level == 0 {
                    let span_low = if overlapped_parents.any()? {
                        span.low
                    } else {
                        ctx.roots.intersection(&span.into()).min().unwrap()
                    };
                    let span_high = span.high;
                    if span_high >= span_low {
                        ctx.result.push_span(Span::from(span_low..=span_high));
                    }
                } else {
                    // Go deeper.
                    visit_segments(ctx, span, level - 1)?;
                }
            }
            Ok(())
        }

        let roots_min: Id = roots.min().unwrap();
        let mut ctx = Context {
            this: self,
            roots,
            roots_min,
            result: SpanSet::empty(),
        };

        visit_segments(&mut ctx, (Id::MIN..=Id::MAX).into(), self.max_level)?;
        Ok(ctx.result)
    }
}

// Full IdMap -> Sparse IdMap
impl IdDag {
    /// Copy a subset of "Universal" mapping from `full_idmap` to
    /// `sparse_idmap`. See [`IdDag::universal`].
    pub fn write_sparse_idmap(
        &self,
        full_idmap: &dyn crate::idmap::IdMapLike,
        sparse_idmap: &mut crate::idmap::IdMap,
    ) -> Result<()> {
        for id in self.universal()? {
            let name = full_idmap.vertex_name(id)?;
            sparse_idmap.insert(id, name.as_ref())?
        }
        Ok(())
    }

    /// Return a subset of [`Id`]s that should be "Universal", including:
    ///
    /// - Heads of the master group.
    /// - Parents of merges (a merge is an id with multiple parents)
    ///   in the MASTER group.
    ///
    /// See also [`FirstAncestorConstraint::KnownUniversally`].
    ///
    /// Complexity: `O(flat segments)` for both time and space.
    fn universal(&self) -> Result<BTreeSet<Id>> {
        let mut result = BTreeSet::new();
        for seg in self.next_segments(Id::MIN, 0)? {
            let parents = seg.parents()?;
            // Is it a merge?
            if parents.len() >= 2 {
                for id in parents {
                    debug_assert_eq!(id.group(), Group::MASTER);
                    result.insert(id);
                }
            }
        }
        for head in self.heads_ancestors(self.master_group()?)? {
            debug_assert_eq!(head.group(), Group::MASTER);
            result.insert(head);
        }
        Ok(result)
    }
}

/// There are many `x~n`s that all resolves to a single commit.
/// Constraint about `x~n`.
pub enum FirstAncestorConstraint {
    /// No constraints.
    None,

    /// `x` and its slice is expected to be known both locally and remotely.
    ///
    /// Practically, this means `x` is either:
    /// - referred explicitly by `heads`.
    /// - a parent of a merge (multi-parent id).
    ///   (at clone time, client gets a sparse idmap including them)
    ///
    /// This also enforces `x` to be part of `ancestors(heads)`.
    KnownUniversally { heads: SpanSet },
}

impl SyncableIdDag {
    /// Make sure the [`SyncableIdDag`] contains the given id (and all ids smaller
    /// than `high`) by building up segments on demand.
    ///
    /// This is similar to [`IdDag::build_segments_volatile`]. However, the build
    /// result is intended to be written to the filesystem. Therefore high-level
    /// segments are intentionally made lagging to reduce fragmentation.
    pub fn build_segments_persistent<F>(&mut self, high: Id, get_parents: &F) -> Result<usize>
    where
        F: Fn(Id) -> Result<Vec<Id>>,
    {
        let mut count = 0;
        count += self.dag.build_flat_segments(high, get_parents, 0)?;
        count += self.dag.build_all_high_level_segments(true)?;
        Ok(count)
    }

    /// Write pending changes to disk. Release the exclusive lock.
    ///
    /// The newly written entries can be fetched by [`IdDag::reload`].
    ///
    /// To avoid races, [`IdDag`]s in the `reload_dags` list will be
    /// reloaded while [`SyncableIdDag`] still holds the lock.
    pub fn sync<'a>(mut self, reload_dags: impl IntoIterator<Item = &'a mut IdDag>) -> Result<()> {
        self.dag.log.sync()?;
        for dag in reload_dags {
            dag.reload()?;
        }
        let _lock_file = self.lock_file; // Make sure lock is not dropped until here.
        Ok(())
    }

    /// Export non-master DAG as parent_id_func on HashMap.
    ///
    /// This can be expensive if there are a lot of non-master ids.
    /// It is currently only used to rebuild non-master groups after
    /// id re-assignment.
    pub fn non_master_parent_ids(&self) -> Result<HashMap<Id, Vec<Id>>> {
        let mut parents = HashMap::new();
        let start = Group::NON_MASTER.min_id();
        for seg in self.dag.next_segments(start, 0)? {
            let span = seg.span()?;
            parents.insert(span.low, seg.parents()?);
            for i in (span.low + 1).to(span.high) {
                parents.insert(i, vec![i - 1]);
            }
        }
        Ok(parents)
    }

    /// Mark non-master segments as "removed".
    pub fn remove_non_master(&mut self) -> Result<()> {
        self.dag.remove_non_master()
    }
}

bitflags! {
    pub struct SegmentFlags: u8 {
        /// This segment has roots (i.e. there is at least one id in
        /// `low..=high`, `parents(id)` is empty).
        const HAS_ROOT = 0b1;

        /// This segment is the only head in `0..=high`.
        /// In other words, `heads(0..=high)` is `[high]`.
        ///
        /// This flag should not be set if the segment is either a high-level
        /// segment, or in a non-master group.
        const ONLY_HEAD = 0b10;
    }
}

impl<'a> Segment<'a> {
    const OFFSET_FLAGS: usize = 0;
    const OFFSET_LEVEL: usize = Self::OFFSET_FLAGS + 1;
    const OFFSET_HIGH: usize = Self::OFFSET_LEVEL + 1;
    const OFFSET_DELTA: usize = Self::OFFSET_HIGH + 8;

    pub(crate) fn flags(&self) -> Result<SegmentFlags> {
        match self.0.get(Self::OFFSET_FLAGS) {
            Some(bits) => Ok(SegmentFlags::from_bits_truncate(*bits)),
            None => bail!("cannot read flags"),
        }
    }

    pub(crate) fn has_root(&self) -> Result<bool> {
        Ok(self.flags()?.contains(SegmentFlags::HAS_ROOT))
    }

    pub(crate) fn only_head(&self) -> Result<bool> {
        Ok(self.flags()?.contains(SegmentFlags::ONLY_HEAD))
    }

    pub(crate) fn high(&self) -> Result<Id> {
        match self.0.get(Self::OFFSET_HIGH..Self::OFFSET_HIGH + 8) {
            Some(slice) => Ok(Id(BigEndian::read_u64(slice))),
            None => bail!("cannot read high"),
        }
    }

    // high - low
    fn delta(&self) -> Result<u64> {
        let (len, _) = self.0.read_vlq_at(Self::OFFSET_DELTA)?;
        Ok(len)
    }

    pub(crate) fn span(&self) -> Result<Span> {
        let high = self.high()?;
        let delta = self.delta()?;
        let low = high - delta;
        Ok((low..=high).into())
    }

    pub(crate) fn head(&self) -> Result<Id> {
        self.high()
    }

    pub(crate) fn level(&self) -> Result<Level> {
        match self.0.get(Self::OFFSET_LEVEL) {
            Some(level) => Ok(*level),
            None => bail!("cannot read level"),
        }
    }

    pub(crate) fn parents(&self) -> Result<Vec<Id>> {
        let mut cur = Cursor::new(self.0);
        cur.set_position(Self::OFFSET_DELTA as u64);
        let _: u64 = cur.read_vlq()?;
        let parent_count: usize = cur.read_vlq()?;
        let mut result = Vec::with_capacity(parent_count);
        for _ in 0..parent_count {
            result.push(Id(cur.read_vlq()?));
        }
        Ok(result)
    }

    pub(crate) fn serialize(
        flags: SegmentFlags,
        level: Level,
        low: Id,
        high: Id,
        parents: &[Id],
    ) -> Vec<u8> {
        debug_assert!(high >= low);
        let mut buf = Vec::with_capacity(1 + 8 + (parents.len() + 2) * 4);
        buf.write_u8(flags.bits()).unwrap();
        buf.write_u8(level).unwrap();
        buf.write_u64::<BigEndian>(high.0).unwrap();
        buf.write_vlq(high.0 - low.0).unwrap();
        buf.write_vlq(parents.len()).unwrap();
        for parent in parents {
            buf.write_vlq(parent.0).unwrap();
        }
        buf
    }
}

impl Debug for IdDag {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let mut first = true;
        for level in 0..=self.max_level {
            if !first {
                write!(f, "\n")?;
            }
            first = false;
            write!(f, "Lv{}:", level)?;

            for group in Group::ALL.iter() {
                let segments = self.next_segments(group.min_id(), level).unwrap();
                if !segments.is_empty() {
                    for segment in segments {
                        write!(f, " {:?}", segment)?;
                    }
                }
            }
        }
        Ok(())
    }
}

impl<'a> Debug for Segment<'a> {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
        let span = self.span().unwrap();
        if self.has_root().unwrap() {
            write!(f, "R")?;
        }
        if self.only_head().unwrap() {
            write!(f, "H")?;
        }
        let parents = self.parents().unwrap();
        write!(f, "{}-{}{:?}", span.low, span.high, parents,)?;
        Ok(())
    }
}

/// Lazily answer `any(...)`, `all(...)`.
struct LazyPredicate<P> {
    ids: Vec<Id>,
    predicate: P,
    true_count: usize,
    false_count: usize,
}

impl<P: Fn(Id) -> Result<bool>> LazyPredicate<P> {
    pub fn new(ids: Vec<Id>, predicate: P) -> Self {
        Self {
            ids,
            predicate,
            true_count: 0,
            false_count: 0,
        }
    }

    pub fn any(&mut self) -> Result<bool> {
        loop {
            if self.true_count > 0 {
                return Ok(true);
            }
            if self.true_count + self.false_count == self.ids.len() {
                return Ok(false);
            }
            self.test_one()?;
        }
    }

    pub fn all(&mut self) -> Result<bool> {
        loop {
            if self.true_count == self.ids.len() {
                return Ok(true);
            }
            if self.false_count > 0 {
                return Ok(false);
            }
            self.test_one()?;
        }
    }

    fn test_one(&mut self) -> Result<()> {
        let i = self.true_count + self.false_count;
        if (self.predicate)(self.ids[i])? {
            self.true_count += 1;
        } else {
            self.false_count += 1;
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
        fn prop(has_root: bool, level: Level, low: u64, delta: u64, parents: Vec<u64>) -> bool {
            let flags = if has_root {
                SegmentFlags::HAS_ROOT
            } else {
                SegmentFlags::empty()
            };
            let high = low + delta;
            let low = Id(low);
            let high = Id(high);
            let parents: Vec<Id> = parents.into_iter().map(Id).collect();
            let buf = Segment::serialize(flags, level, low, high, &parents);
            let node = Segment(&buf);
            node.flags().unwrap() == flags
                && node.level().unwrap() == level
                && node.span().unwrap() == (low..=high).into()
                && node.parents().unwrap() == parents
        }
        quickcheck(prop as fn(bool, Level, u64, u64, Vec<u64>) -> bool);
    }

    #[test]
    fn test_segment_basic_lookups() {
        let dir = tempdir().unwrap();
        let mut dag = IdDag::open(dir.path()).unwrap();
        assert_eq!(dag.next_free_id(0, Group::MASTER).unwrap().0, 0);
        assert_eq!(dag.next_free_id(1, Group::MASTER).unwrap().0, 0);

        let flags = SegmentFlags::empty();

        dag.insert(flags, 0, Id::MIN, Id(50), &vec![]).unwrap();
        assert_eq!(dag.next_free_id(0, Group::MASTER).unwrap().0, 51);
        dag.insert(flags, 0, Id(51), Id(100), &vec![Id(50)])
            .unwrap();
        assert_eq!(dag.next_free_id(0, Group::MASTER).unwrap().0, 101);
        dag.insert(flags, 0, Id(101), Id(150), &vec![Id(100)])
            .unwrap();
        assert_eq!(dag.next_free_id(0, Group::MASTER).unwrap().0, 151);
        assert_eq!(dag.next_free_id(1, Group::MASTER).unwrap().0, 0);
        dag.insert(flags, 1, Id::MIN, Id(100), &vec![]).unwrap();
        assert_eq!(dag.next_free_id(1, Group::MASTER).unwrap().0, 101);
        dag.insert(flags, 1, Id(101), Id(150), &vec![Id(100)])
            .unwrap();
        assert_eq!(dag.next_free_id(1, Group::MASTER).unwrap().0, 151);

        // Helper functions to make the below lines shorter.
        let low_by_head = |head, level| match dag.find_segment_by_head_and_level(Id(head), level) {
            Ok(Some(seg)) => seg.span().unwrap().low.0 as i64,
            Ok(None) => -1,
            _ => panic!("unexpected error"),
        };

        let low_by_id = |id| match dag.find_flat_segment_including_id(Id(id)) {
            Ok(Some(seg)) => seg.span().unwrap().low.0 as i64,
            Ok(None) => -1,
            _ => panic!("unexpected error"),
        };

        assert_eq!(low_by_head(0, 0), -1);
        assert_eq!(low_by_head(49, 0), -1);
        assert_eq!(low_by_head(50, 0), 0);
        assert_eq!(low_by_head(51, 0), -1);
        assert_eq!(low_by_head(150, 0), 101);
        assert_eq!(low_by_head(100, 1), 0);

        assert_eq!(low_by_id(0), 0);
        assert_eq!(low_by_id(30), 0);
        assert_eq!(low_by_id(49), 0);
        assert_eq!(low_by_id(50), 0);
        assert_eq!(low_by_id(51), 51);
        assert_eq!(low_by_id(52), 51);
        assert_eq!(low_by_id(99), 51);
        assert_eq!(low_by_id(100), 51);
        assert_eq!(low_by_id(101), 101);
        assert_eq!(low_by_id(102), 101);
        assert_eq!(low_by_id(149), 101);
        assert_eq!(low_by_id(150), 101);
        assert_eq!(low_by_id(151), -1);
    }

    fn get_parents(id: Id) -> Result<Vec<Id>> {
        match id.0 {
            0 | 1 | 2 => Ok(Vec::new()),
            _ => Ok(vec![id - 1, Id(id.0 / 2)]),
        }
    }

    #[test]
    fn test_sync_reload() {
        let dir = tempdir().unwrap();
        let mut dag = IdDag::open(dir.path()).unwrap();
        assert_eq!(dag.next_free_id(0, Group::MASTER).unwrap().0, 0);

        let mut syncable = dag.prepare_filesystem_sync().unwrap();
        syncable
            .build_segments_persistent(Id(1001), &get_parents)
            .unwrap();

        syncable.sync(std::iter::once(&mut dag)).unwrap();

        assert_eq!(dag.max_level, 3);
        assert_eq!(
            dag.children(Id(1000)).unwrap().iter().collect::<Vec<Id>>(),
            vec![Id(1001)]
        );
    }

    #[test]
    fn test_all() {
        let dir = tempdir().unwrap();
        let mut dag = IdDag::open(dir.path()).unwrap();
        assert!(dag.all().unwrap().is_empty());
        dag.build_segments_volatile(Id(1001), &get_parents).unwrap();
        assert_eq!(dag.all().unwrap().count(), 1002);
    }
}
