/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # segment
//!
//! Segmented DAG. See [`Dag`] for the main structure.
//!
//! There are 2 flavors of DAG: [`Dag`] and [`SyncableDag`]. [`Dag`] loads
//! from the filesystem, is responsible for all kinds of queires, and can
//! have in-memory-only changes. [`SyncableDag`] is the only way to update
//! the filesystem state, and does not support queires.

use crate::id::Id;
use crate::spanset::Span;
use crate::spanset::SpanSet;
use anyhow::{bail, Result};
use bitflags::bitflags;
use byteorder::{BigEndian, ByteOrder, WriteBytesExt};
use fs2::FileExt;
use indexedlog::log;
use indexmap::set::IndexSet;
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
pub struct Dag {
    pub(crate) log: log::Log,
    path: PathBuf,
    max_level: Level,
    new_seg_size: usize,
}

/// Guard to make sure [`Dag`] on-disk writes are race-free.
pub struct SyncableDag {
    dag: Dag,
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
    const INDEX_LEVEL_HEAD: usize = 0;
    const KEY_LEVEL_HEAD_LEN: usize = Segment::OFFSET_DELTA - Segment::OFFSET_LEVEL;

    /// Open [`Dag`] at the given directory. Create it on demand.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let log = log::OpenOptions::new()
            .create(true)
            .index("level-head", |_| {
                // (level, high)
                vec![log::IndexOutput::Reference(
                    Segment::OFFSET_LEVEL as u64..Segment::OFFSET_DELTA as u64,
                )]
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
    pub fn next_free_id(&self, level: Level) -> Result<Id> {
        let prefix = [level];
        match self
            .log
            .lookup_prefix(Self::INDEX_LEVEL_HEAD, &prefix)?
            .rev()
            .nth(0)
        {
            None => Ok(Id(0)),
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

    /// Return a [`SyncableDag`] instance that provides race-free
    /// filesytem read and write access by taking an exclusive lock.
    ///
    /// The [`SyncableDag`] instance provides a `sync` method that
    /// actually writes changes to disk.
    ///
    /// Block if another instance is taking the lock.
    pub fn prepare_filesystem_sync(&self) -> Result<SyncableDag> {
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

        Ok(SyncableDag {
            dag: Dag {
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
        assert!(size > 1);
        self.new_seg_size = size;
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
impl Dag {
    /// Make sure the [`Dag`] contains the given id (and all ids smaller than
    /// `high`) by building up segments on demand.
    ///
    /// `get_parents` describes the DAG. Its input and output are `Id`s.
    ///
    /// This is often used together with [`crate::idmap::IdMap`].
    ///
    /// Content inserted by this function *will not* be written to disk.
    /// For example, [`Dag::prepare_filesystem_sync`] will drop them.
    pub fn build_segments_volatile<F>(&mut self, high: Id, get_parents: &F) -> Result<usize>
    where
        F: Fn(Id) -> Result<Vec<Id>>,
    {
        let mut count = 0;
        count += self.build_flat_segments(high, get_parents, 0)?;
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
    /// [`Dag`] covers less ids.
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
        let low = self.next_free_id(0)?;
        let mut current_low = None;
        let mut current_parents = Vec::new();
        let mut insert_count = 0;
        for id in low.to(high) {
            let parents = get_parents(id)?;
            if parents.len() != 1 || parents[0] + 1 != id {
                // Must start a new segment.
                if let Some(low) = current_low {
                    debug_assert!(id > Id(0));
                    let flags = if current_parents.is_empty() {
                        SegmentFlags::HAS_ROOT
                    } else {
                        SegmentFlags::empty()
                    };
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
                let flags = if current_parents.is_empty() {
                    SegmentFlags::HAS_ROOT
                } else {
                    SegmentFlags::empty()
                };
                self.insert(flags, 0, low, high, &current_parents)?;
                insert_count += 1;
            }
        }

        Ok(insert_count)
    }

    /// Find segments that covers `id..` range at the given level.
    fn next_segments(&self, id: Id, level: Level) -> Result<Vec<Segment>> {
        let lower_bound = Self::serialize_head_level_lookup_key(id, level);
        let upper_bound = [level + 1];
        let mut result = Vec::new();
        for entry in self
            .log
            .lookup_range(Self::INDEX_LEVEL_HEAD, (&lower_bound[..])..&upper_bound)?
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
        let lower_bound = Self::serialize_head_level_lookup_key(Id(0), level);
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
        assert!(level > 0);
        let size = self.new_seg_size;

        // `get_parents` is on the previous level of segments.
        let get_parents = |head: Id| -> Result<Vec<Id>> {
            if let Some(seg) = self.find_segment_by_head_and_level(head, level - 1)? {
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

            // Drop the last segment. It could be incomplete.
            if drop_last {
                new_segments.pop();
            }

            // No point to introduce new levels if it has the same segment count
            // as the loweer level.
            if segments.len() == new_segments.len() && self.max_level < level {
                return Ok(0);
            }

            new_segments
        };

        let insert_count = new_segments.len();

        for (_, low, high, parents, has_root) in new_segments {
            let flags = if has_root {
                SegmentFlags::HAS_ROOT
            } else {
                SegmentFlags::empty()
            };
            self.insert(flags, level, low, high, &parents)?;
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
impl Dag {
    /// Reload from the filesystem. Discard pending changes.
    pub fn reload(&mut self) -> Result<()> {
        self.log.clear_dirty()?;
        self.log.sync()?;
        self.max_level = Self::max_level_from_log(&self.log)?;
        self.build_all_high_level_segments(false)?;
        Ok(())
    }
}

// User-facing DAG-related algorithms.
impl Dag {
    /// Return a [`SpanSet`] that covers all ids stored in this [`Dag`].
    pub fn all(&self) -> Result<SpanSet> {
        match self.next_free_id(0)? {
            Id(0) => Ok(SpanSet::empty()),
            n => Ok(SpanSet::from(Id(0)..=(n - 1))),
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
            for level in (0..=self.max_level).rev() {
                let seg = match level {
                    0 => self.find_flat_segment_including_id(id)?,
                    _ => self.find_segment_by_head_and_level(id, level)?,
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

    /// Calculate parents of the given set.
    ///
    /// Note: [`SpanSet`] does not preserve order. Use [`Dag::parent_ids`] if
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
                debug_assert!(!set.contains(Id(0)));
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
            this: &'a Dag,
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

        let result_lower_bound = set.min().unwrap_or(Id::max_value());
        let mut ctx = Context {
            this: self,
            set,
            result_lower_bound,
            result: SpanSet::empty(),
        };

        visit_segments(&mut ctx, (Id(0)..=Id::max_value()).into(), self.max_level)?;
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
                // `common_ancestors(X)` = `common_ancestors(heads(X))`.
                let set = self.heads(set)?;
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
            this: &'a Dag,
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
            visit_segments(&mut ctx, (Id(0)..=Id::max_value()).into(), self.max_level)?;
        }
        Ok(ctx.result)
    }

    /// Calculate the descendants of the given set.
    ///
    /// Logically equvilent to `range(set, all())`.
    pub fn descendants(&self, set: impl Into<SpanSet>) -> Result<SpanSet> {
        // The algorithm is a manually "inlined" version of `range` where `ancestors`
        // is known to be `all()`.

        let roots = set.into();
        if roots.is_empty() {
            return Ok(SpanSet::empty());
        }

        struct Context<'a> {
            this: &'a Dag,
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

        visit_segments(&mut ctx, (Id(0)..=Id::max_value()).into(), self.max_level)?;
        Ok(ctx.result)
    }
}

impl SyncableDag {
    /// Make sure the [`SyncableDag`] contains the given id (and all ids smaller
    /// than `high`) by building up segments on demand.
    ///
    /// This is similar to [`Dag::build_segments_volatile`]. However, the build
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
    /// The newly written entries can be fetched by [`Dag::reload`].
    ///
    /// To avoid races, [`Dag`]s in the `reload_dags` list will be
    /// reloaded while [`SyncableDag`] still holds the lock.
    pub fn sync<'a>(mut self, reload_dags: impl IntoIterator<Item = &'a mut Dag>) -> Result<()> {
        self.dag.log.sync()?;
        for dag in reload_dags {
            dag.reload()?;
        }
        let _lock_file = self.lock_file; // Make sure lock is not dropped until here.
        Ok(())
    }
}

bitflags! {
    pub struct SegmentFlags: u8 {
        /// This segment has roots (i.e. there is at least one id in
        /// `low..=high`, `parents(id)` is empty).
        const HAS_ROOT = 0b1;
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
        assert!(high >= low);
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

impl Debug for Dag {
    fn fmt(&self, f: &mut Formatter) -> fmt::Result {
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
            if segment.has_root().unwrap() {
                write!(f, "R")?;
            }
            let parents = segment
                .parents()
                .unwrap()
                .into_iter()
                .map(|i| i.0)
                .collect::<Vec<_>>();
            write!(f, "{}-{}{:?}", span.low, span.high, parents,)?;
        }
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
        let mut dag = Dag::open(dir.path()).unwrap();
        assert_eq!(dag.next_free_id(0).unwrap().0, 0);
        assert_eq!(dag.next_free_id(1).unwrap().0, 0);

        let flags = SegmentFlags::empty();

        dag.insert(flags, 0, Id(0), Id(50), &vec![]).unwrap();
        assert_eq!(dag.next_free_id(0).unwrap().0, 51);
        dag.insert(flags, 0, Id(51), Id(100), &vec![Id(50)])
            .unwrap();
        assert_eq!(dag.next_free_id(0).unwrap().0, 101);
        dag.insert(flags, 0, Id(101), Id(150), &vec![Id(100)])
            .unwrap();
        assert_eq!(dag.next_free_id(0).unwrap().0, 151);
        assert_eq!(dag.next_free_id(1).unwrap().0, 0);
        dag.insert(flags, 1, Id(0), Id(100), &vec![]).unwrap();
        assert_eq!(dag.next_free_id(1).unwrap().0, 101);
        dag.insert(flags, 1, Id(101), Id(150), &vec![Id(100)])
            .unwrap();
        assert_eq!(dag.next_free_id(1).unwrap().0, 151);

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
        let mut dag = Dag::open(dir.path()).unwrap();
        assert_eq!(dag.next_free_id(0).unwrap().0, 0);

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
        let mut dag = Dag::open(dir.path()).unwrap();
        assert!(dag.all().unwrap().is_empty());
        dag.build_segments_volatile(Id(1001), &get_parents).unwrap();
        assert_eq!(dag.all().unwrap().count(), 1002);
    }
}
