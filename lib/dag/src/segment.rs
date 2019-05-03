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
use std::collections::{BTreeSet, BinaryHeap};
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
}
