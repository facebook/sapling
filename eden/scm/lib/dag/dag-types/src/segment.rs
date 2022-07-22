/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

use crate::id::Id;

/// Base segment.
///
/// Intermediate structure between processing a Dag and constructing high level segments.
#[derive(Debug, Clone, PartialEq, Eq)]
#[derive(Serialize, Deserialize, Ord, PartialOrd)]
#[cfg_attr(
    any(test, feature = "for-tests"),
    derive(quickcheck_arbitrary_derive::Arbitrary)
)]
pub struct FlatSegment {
    pub low: Id,
    pub high: Id,
    pub parents: Vec<Id>,
}

use std::collections::BTreeSet;

/// These segments can be used directly in the build process of the IdDag.
/// They produced by `IdMap::assign_head` and `IdDag::all_flat_segments`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
#[derive(Serialize, Deserialize)]
pub struct PreparedFlatSegments {
    /// New flat segments.
    pub segments: BTreeSet<FlatSegment>,
}

impl PreparedFlatSegments {
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

    /// Return set of all (unique) parents + head + roots of flat segments.
    ///
    /// Used by the pull fast path to provide necessary "anchor" vertexes
    /// ("universally known", and ones needed by the client to make decisions)
    /// in the IdMap.
    ///
    /// Might return some extra `Id`s that are not part of parents, heads, or
    /// roots. They are useful for the client to verify the graph is the same
    /// as the server.
    ///
    /// The size of the returned `Id`s is about `O(segments)`.
    pub fn parents_head_and_roots(&self) -> BTreeSet<Id> {
        self.segments
            .iter()
            .map(|seg| {
                // `seg.high` is either a head, or a parent referred by another seg
                // `seg.low` is either a room, or something unnecessary for lazy protocol,
                // but speeds up graph shape verification (see `check_isomorphic_graph`).
                // `parents` are either "universally known", essential for lazy protocol,
                // or something necessary for the pull protocol to re-map the IdMap.
                [seg.high, seg.low]
                    .into_iter()
                    .chain(seg.parents.clone().into_iter())
            })
            .flatten()
            .collect()
    }
}
