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

use crate::id::Id;
use crate::spanset::Span;
use crate::Level;
use anyhow::{bail, Result};
use bitflags::bitflags;
use byteorder::{BigEndian, ByteOrder, WriteBytesExt};
use std::fmt::{self, Debug, Formatter};
use std::io::Cursor;
use vlqencoding::{VLQDecode, VLQDecodeAt, VLQEncode};

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
    pub(crate) const OFFSET_FLAGS: usize = 0;
    pub(crate) const OFFSET_LEVEL: usize = Self::OFFSET_FLAGS + 1;
    pub(crate) const OFFSET_HIGH: usize = Self::OFFSET_LEVEL + 1;
    pub(crate) const OFFSET_DELTA: usize = Self::OFFSET_HIGH + 8;

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
            let buf = Segment::serialize(flags, level, low, high, &parents);
            let node = Segment(&buf);
            node.flags().unwrap() == flags
                && node.level().unwrap() == level
                && node.span().unwrap() == (low..=high).into()
                && node.parents().unwrap() == parents
        }
        quickcheck(prop as fn(bool, Level, u64, u64, Vec<u64>) -> bool);
    }
}
