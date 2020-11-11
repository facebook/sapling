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

use crate::errors::bug;
use crate::id::Id;
use crate::spanset::Span;
use crate::Level;
use crate::Result;
use bitflags::bitflags;
use byteorder::{BigEndian, ByteOrder, WriteBytesExt};
use minibytes::Bytes;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Debug, Formatter};
use std::io::Cursor;
use vlqencoding::{VLQDecode, VLQDecodeAt, VLQEncode};

/// [`Segment`] represents a range of [`Id`]s in an [`IdDag`] graph.
/// It provides methods to access properties of the segments, including the range itself,
/// parents, and level information.
#[derive(Clone, Eq, Serialize, Deserialize)]
pub struct Segment(pub(crate) Bytes);

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

impl Segment {
    pub(crate) const OFFSET_FLAGS: usize = 0;
    pub(crate) const OFFSET_LEVEL: usize = Self::OFFSET_FLAGS + 1;
    pub(crate) const OFFSET_HIGH: usize = Self::OFFSET_LEVEL + 1;
    pub(crate) const OFFSET_DELTA: usize = Self::OFFSET_HIGH + 8;

    pub(crate) fn flags(&self) -> Result<SegmentFlags> {
        match self.0.get(Self::OFFSET_FLAGS) {
            Some(bits) => Ok(SegmentFlags::from_bits_truncate(*bits)),
            None => bug("cannot read Segment::flags"),
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
            None => bug("cannot read Segment::high"),
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
            None => bug("cannot read Segment::level"),
        }
    }

    pub(crate) fn parents(&self) -> Result<Vec<Id>> {
        let mut cur = Cursor::new(&self.0);
        cur.set_position(Self::OFFSET_DELTA as u64);
        let _: u64 = cur.read_vlq()?;
        let parent_count: usize = cur.read_vlq()?;
        let mut result = Vec::with_capacity(parent_count);
        for _ in 0..parent_count {
            result.push(Id(cur.read_vlq()?));
        }
        Ok(result)
    }

    pub(crate) fn new(
        flags: SegmentFlags,
        level: Level,
        low: Id,
        high: Id,
        parents: &[Id],
    ) -> Self {
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
        Self(buf.into())
    }
}

impl PartialEq for Segment {
    fn eq(&self, other: &Self) -> bool {
        self.0[..] == other.0[..]
    }
}

impl Debug for Segment {
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

/// Base segment.
///
/// Intermediate structure between processing a Dag and constructing high level segments.
#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Serialize, Deserialize)]
pub struct FlatSegment {
    pub low: Id,
    pub high: Id,
    pub parents: Vec<Id>,
}

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FlatSegment {
    fn arbitrary<G: quickcheck::Gen>(g: &mut G) -> Self {
        Self {
            low: Id::arbitrary(g),
            high: Id::arbitrary(g),
            parents: Vec::arbitrary(g),
        }
    }
}

/// These segments can be used directly in the build process of the IdDag.
/// They produced by `IdMap::assign_head` and `IdDag::all_flat_segments`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[derive(Serialize, Deserialize)]
pub struct PreparedFlatSegments {
    /// New flat segments.
    pub segments: Vec<FlatSegment>,
}

impl PreparedFlatSegments {
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
    pub fn push_edge(&mut self, id: Id, parent_ids: &[Id]) {
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

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;

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
            let node = Segment::new(flags, level, low, high, &parents);
            node.flags().unwrap() == flags
                && node.level().unwrap() == level
                && node.span().unwrap() == (low..=high).into()
                && node.parents().unwrap() == parents
        }
        quickcheck(prop as fn(bool, Level, u64, u64, Vec<u64>) -> bool);
    }
}
