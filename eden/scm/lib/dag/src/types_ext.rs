/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Extensions to types in other crates.

use std::collections::BTreeSet;

use crate::FlatSegment;
use crate::Id;
use crate::PreparedFlatSegments;

pub(crate) trait PreparedFlatSegmentsExt {
    /// Merge with another (newer) `PreparedFlatSegmentsExt`.
    fn merge(&mut self, rhs: Self);

    /// Add graph edges: id -> parent_ids. Used by `assign_head`.
    fn push_edge(&mut self, id: Id, parent_ids: &[Id]) {
        self.push_segment(id, id, parent_ids)
    }

    /// Add segment. Used by `import_pull_data`.
    fn push_segment(&mut self, low: Id, high: Id, parent_ids: &[Id]);
}

impl PreparedFlatSegmentsExt for PreparedFlatSegments {
    fn merge(&mut self, rhs: Self) {
        if rhs.segments.is_empty() {
            return;
        }
        if self.segments.is_empty() {
            *self = rhs;
            return;
        }

        for seg in rhs.segments {
            if !maybe_merge_in_place(&mut self.segments, seg.low, seg.high, &seg.parents) {
                self.segments.insert(seg);
            }
        }

        if cfg!(debug_assertions) {
            ensure_sorted_and_merged(&self.segments);
        }
    }

    fn push_segment(&mut self, low: Id, high: Id, parent_ids: &[Id]) {
        if !maybe_merge_in_place(&mut self.segments, low, high, parent_ids) {
            let new_seg = FlatSegment {
                low,
                high,
                parents: parent_ids.to_vec(),
            };
            self.segments.insert(new_seg);
        }
        if cfg!(debug_assertions) {
            ensure_sorted_and_merged(&self.segments);
        }
    }
}

/// Try to merge a flat segment (low..=high, parents=parents) in place.
/// Return true if it was merged in place.
fn maybe_merge_in_place(
    segments: &mut BTreeSet<FlatSegment>,
    low: Id,
    high: Id,
    parent_ids: &[Id],
) -> bool {
    if let [parent_id] = parent_ids {
        if *parent_id + 1 != low {
            return false;
        }
    } else {
        return false;
    }
    let upper_bound = FlatSegment {
        low,
        high: low,
        parents: Vec::new(),
    };
    if let Some(candidate) = segments.range(..=upper_bound).rev().next() {
        if candidate.high + 1 == low {
            // Merge
            let candidate = candidate.clone();
            let new_seg = FlatSegment {
                low: candidate.low,
                high,
                parents: candidate.parents.clone(),
            };
            segments.remove(&candidate);
            segments.insert(new_seg);
            return true;
        }
    }
    false
}

/// Check that segments are sorted and merged.
fn ensure_sorted_and_merged(segments: &BTreeSet<FlatSegment>) {
    let mut last_high = None;
    for seg in segments {
        // Sorted? No overlap?
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
        assert_eq!(
            dbg(&segs),
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

    #[test]
    fn test_push_segment() {
        let mut segs = PreparedFlatSegments::default();
        segs.push_segment(Id(10), Id(20), &[]);
        segs.push_segment(Id(40), Id(50), &[Id(15), Id(10), Id(12)]);
        segs.push_segment(Id(21), Id(30), &[Id(20)]);
        segs.push_segment(Id(31), Id(35), &[Id(20)]);
        assert_eq!(
            dbg(&segs),
            [
                "FlatSegment { low: 10, high: 30, parents: [] }",
                "FlatSegment { low: 31, high: 35, parents: [20] }",
                "FlatSegment { low: 40, high: 50, parents: [15, 10, 12] }",
            ]
        );
    }

    #[test]
    fn test_merge() {
        let mut segs1 = PreparedFlatSegments::default();
        segs1.push_edge(Id(10), &[]);
        segs1.push_edge(Id(11), &[Id(10)]);
        let mut segs2 = PreparedFlatSegments::default();
        segs2.push_edge(Id(12), &[Id(11)]);
        segs2.push_edge(Id(13), &[Id(12)]);
        segs2.push_edge(Id(14), &[Id(11)]);
        segs1.merge(segs2);
        assert_eq!(
            dbg(&segs1),
            [
                "FlatSegment { low: 10, high: 13, parents: [] }",
                "FlatSegment { low: 14, high: 14, parents: [11] }"
            ]
        );
    }

    fn dbg(segs: &PreparedFlatSegments) -> Vec<String> {
        segs.segments.iter().map(|s| format!("{:?}", s)).collect()
    }
}
