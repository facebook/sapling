/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use serde::{Deserialize, Serialize};

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
