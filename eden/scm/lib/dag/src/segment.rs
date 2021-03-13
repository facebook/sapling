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
use crate::IdSpan;
use crate::Level;
use crate::Result;
use bitflags::bitflags;
use byteorder::ReadBytesExt;
use byteorder::{BigEndian, ByteOrder, WriteBytesExt};
use minibytes::Bytes;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Debug, Formatter};
use std::io::Cursor;
use vlqencoding::{VLQDecode, VLQDecodeAt, VLQEncode};

pub use dag_types::segment::FlatSegment;
pub use dag_types::segment::PreparedFlatSegments;

/// [`Segment`] represents a range of [`Id`]s in an [`IdDag`] graph.
/// It provides methods to access properties of the segments, including the range itself,
/// parents, and level information.
#[derive(Clone, Eq, Serialize, Deserialize)]
pub struct Segment(pub(crate) Bytes);

// Serialization format for Segment:
//
// ```plain,ignore
// SEGMENT := FLAG (1B) + LEVEL (1B) + HIGH (8B) + vlq(HIGH-LOW) + vlq(PARENT_COUNT) + vlq(VLQ, PARENTS)
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

    pub(crate) fn span(&self) -> Result<IdSpan> {
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

    pub(crate) fn parent_count(&self) -> Result<usize> {
        let mut cur = Cursor::new(&self.0);
        cur.set_position(Self::OFFSET_DELTA as u64);
        let _: u64 = cur.read_vlq()?;
        let parent_count: usize = cur.read_vlq()?;
        Ok(parent_count)
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
        debug_assert!(parents.iter().all(|&p| p < low));
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
        if span.low > span.high {
            write!(f, " (Invalid Span!!)")?;
        }
        if parents.iter().any(|&p| p >= span.low) {
            write!(f, " (Invalid Parent!!)")?;
        }
        Ok(())
    }
}

/// Describe bytes of a Segment.
/// This is only for troubleshooting purpose.
pub fn describe_segment_bytes(data: &[u8]) -> String {
    let mut message = String::new();
    let mut cur = Cursor::new(data);
    let mut start = 0;
    let mut explain = |cur: &Cursor<_>, m: String| {
        let end = cur.position() as usize;
        message += &format!("# {}: {}\n", hex(&data[start..end]), m);
        start = end;
    };
    if let Ok(flags) = cur.read_u8() {
        let flags = SegmentFlags::from_bits_truncate(flags);
        explain(&cur, format!("Flags = {:?}", flags));
    }
    if let Ok(lv) = cur.read_u8() {
        explain(&cur, format!("Level = {:?}", lv));
    }
    if let Ok(head) = cur.read_u64::<BigEndian>() {
        explain(&cur, format!("High = {}", Id(head)));
        if let Ok(delta) = VLQDecode::<u64>::read_vlq(&mut cur) {
            let low = head - delta;
            explain(&cur, format!("Delta = {} (Low = {})", delta, Id(low)));
        }
    }
    if let Ok(count) = VLQDecode::<usize>::read_vlq(&mut cur) {
        explain(&cur, format!("Parent count = {}", count));
        for i in 0..count {
            if let Ok(p) = VLQDecode::<u64>::read_vlq(&mut cur) {
                explain(&cur, format!("Parents[{}] = {}", i, Id(p)));
            }
        }
    }
    message
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .cloned()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<String>>()
        .join(" ")
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
            let parents: Vec<Id> = parents.into_iter().filter(|&p| p < low).map(Id).collect();
            let low = Id(low);
            let high = Id(high);
            let node = Segment::new(flags, level, low, high, &parents);
            node.flags().unwrap() == flags
                && node.level().unwrap() == level
                && node.span().unwrap() == (low..=high).into()
                && node.parents().unwrap() == parents
        }
        quickcheck(prop as fn(bool, Level, u64, u64, Vec<u64>) -> bool);
    }

    #[test]
    fn test_describe() {
        let seg = Segment::new(
            SegmentFlags::ONLY_HEAD,
            3,
            Id(101),
            Id(202),
            &[Id(90), Id(80)],
        );
        assert_eq!(
            describe_segment_bytes(&seg.0),
            r#"# 02: Flags = ONLY_HEAD
# 03: Level = 3
# 00 00 00 00 00 00 00 ca: High = 202
# 65: Delta = 101 (Low = 101)
# 02: Parent count = 2
# 5a: Parents[0] = 90
# 50: Parents[1] = 80
"#
        );
    }

    #[test]
    fn test_invalid_fmt() {
        let bytes = Bytes::from_static(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 0, 1, 10]);
        let segment = Segment(bytes);
        assert_eq!(format!("{:?}", segment), "10-10[10] (Invalid Parent!!)");
    }
}
