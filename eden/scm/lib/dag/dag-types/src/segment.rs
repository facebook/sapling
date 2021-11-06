/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::Deserialize;
use serde::Serialize;

use crate::id::Id;

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

use std::collections::BTreeSet;

#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Arbitrary;
#[cfg(any(test, feature = "for-tests"))]
use quickcheck::Gen;

#[cfg(any(test, feature = "for-tests"))]
impl Arbitrary for FlatSegment {
    fn arbitrary(g: &mut Gen) -> Self {
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

    pub fn vertex_count(&self) -> u64 {
        let mut count = 0;
        for segment in &self.segments {
            count += segment.high.0 - segment.low.0 + 1;
        }
        count
    }

    pub fn segment_count(&self) -> usize {
        self.segments.len()
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

    /// Return set of all (unique) parents + head + roots of flat segments.
    pub fn parents_head_and_roots(&self) -> BTreeSet<Id> {
        // Parents
        let mut s: BTreeSet<Id> = self
            .segments
            .iter()
            .map(|seg| &seg.parents)
            .flatten()
            .copied()
            .collect();
        // Head
        if let Some(h) = self.head_id() {
            s.insert(h);
        }
        // Roots
        let id_set: BTreeSet<(Id, Id)> = self.segments.iter().map(|s| (s.low, s.high)).collect();
        let contains = |id: Id| -> bool {
            for &(low, high) in id_set.range(..=(id, Id::MAX)).rev() {
                if id >= low && id <= high {
                    return true;
                }
                if id < low {
                    break;
                }
            }
            false
        };
        for seg in &self.segments {
            let pids: Vec<Id> = seg.parents.iter().copied().collect();
            if pids.iter().all(|&p| !contains(p)) {
                // seg.low is a root.
                s.insert(seg.low);
            }
        }
        s
    }

    /// Add graph edges: id -> parent_ids. Used by `assign_head`.
    pub fn push_edge(&mut self, id: Id, parent_ids: &[Id]) {
        let new_seg = FlatSegment {
            low: id,
            high: id,
            parents: parent_ids.to_vec(),
        };

        // Find the position to insert the new segment.
        let idx = match self.segments.binary_search_by_key(&id, |seg| seg.high) {
            Ok(i) => i,
            Err(i) => i,
        };

        if parent_ids.len() != 1 || parent_ids[0] + 1 != id || idx == 0 {
            // Start a new segment.
            self.segments.insert(idx, new_seg);
        } else {
            // Try to reuse the existing segment.
            if let Some(seg) = self.segments.get_mut(idx - 1) {
                if seg.high + 1 == id {
                    seg.high = id;
                } else {
                    self.segments.insert(idx, new_seg);
                }
            } else {
                self.segments.insert(idx, new_seg);
            }
        }

        // Check that segments are sorted and merged.
        if cfg!(debug_assertions) {
            let mut last_high = None;
            for seg in &self.segments {
                // Sorted?
                assert!(Some(seg.low) > last_high);
                // Merged?
                    if let Some(last_high) = last_high {
                if seg.parents.len() == 1 && seg.parents[0] + 1 == seg.low {
                    assert_ne!(last_high + 1, seg.low);
                }
                    }
                last_high = Some(seg.high);
            }
        }
    }

    #[cfg(feature = "for-tests")]
    /// Verify against a parent function. For testing only.
    pub fn verify<F, E>(&self, parent_func: F)
    where
        F: Fn(Id) -> Result<Vec<Id>, E>,
        E: std::fmt::Debug,
    {
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

    #[test]
    fn test_push_edge_out_of_order() {
        let mut segs = PreparedFlatSegments::default();
        segs.push_edge(Id(0), &[]);
        segs.push_edge(Id(50), &[]);
        segs.push_edge(Id(100), &[]);
        segs.push_edge(Id(1), &[Id(0)]);
        segs.push_edge(Id(51), &[Id(50)]);
        segs.push_edge(Id(101), &[Id(100)]);
        segs.push_edge(Id(2), &[]);
        segs.push_edge(Id(52), &[Id(51), Id(50)]);
        segs.push_edge(Id(102), &[Id(100)]);
        segs.push_edge(Id(103), &[Id(102)]);
        segs.push_edge(Id(53), &[Id(52)]);
        segs.push_edge(Id(105), &[Id(103)]);
        segs.push_edge(Id(106), &[Id(105)]);
        segs.push_edge(Id(104), &[Id(103)]);
        segs.push_edge(Id(3), &[Id(2)]);
        segs.push_edge(Id(4), &[Id(3)]);
        segs.push_edge(Id(54), &[Id(53)]);
        segs.push_edge(Id(49), &[Id(3)]);
        segs.push_edge(Id(107), &[Id(106)]);

        // Check that adjacent segments are merged.
        let dbg: Vec<String> = segs.segments.iter().map(|s| format!("{:?}", s)).collect();
        assert_eq!(
            dbg,
            [
                "FlatSegment { low: 0, high: 1, parents: [] }",
                "FlatSegment { low: 2, high: 4, parents: [] }",
                "FlatSegment { low: 49, high: 49, parents: [3] }",
                "FlatSegment { low: 50, high: 51, parents: [] }",
                "FlatSegment { low: 52, high: 54, parents: [51, 50] }",
                "FlatSegment { low: 100, high: 101, parents: [] }",
                "FlatSegment { low: 102, high: 104, parents: [100] }",
                "FlatSegment { low: 105, high: 107, parents: [103] }"
            ]
        );
    }
}
