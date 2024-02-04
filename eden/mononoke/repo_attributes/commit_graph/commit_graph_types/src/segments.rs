/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt::Display;

use mononoke_types::ChangesetId;
use mononoke_types::Generation;
use smallvec::SmallVec;

/// A representation of a segment of changesets.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct ChangesetSegment {
    /// The highest generation changeset in the segment.
    pub head: ChangesetId,
    /// The lowest generation changeset in the segment.
    pub base: ChangesetId,
    /// The number of changesets in the segment.
    pub length: u64,
    /// Parents of the lowest generation changeset in the segment.
    pub parents: SmallVec<[ChangesetSegmentParent; 1]>,
}

/// A representation of a parent of a segment.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ChangesetSegmentParent {
    pub cs_id: ChangesetId,
    pub location: Option<ChangesetSegmentLocation>,
}

/// A location of a changeset in a segment, represented as the ancestor
/// of the head of the segment that's at the specified distance (i.e. head~distance).
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ChangesetSegmentLocation {
    pub head: ChangesetId,
    pub distance: u64,
}

impl Display for ChangesetSegment {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(
            f,
            "{} -> {}, length: {}, parents: {}",
            self.head,
            self.base,
            self.length,
            self.parents
                .iter()
                .map(|cs_id| cs_id.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        )
    }
}

impl Display for ChangesetSegmentParent {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.cs_id)?;

        if let Some(location) = &self.location {
            write!(f, " ({})", location)?;
        }

        Ok(())
    }
}

impl Display for ChangesetSegmentLocation {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}~{}", self.head, self.distance)
    }
}

/// A frontier of changeset segments ordered by the generation
/// number of their base. For each generation the segments are
/// represented as a map from their base to the set of their heads.
#[derive(Default, Debug)]
pub struct ChangesetSegmentFrontier {
    pub segments: BTreeMap<Generation, HashMap<ChangesetId, HashSet<ChangesetId>>>,
}

/// A location of a changeset represented as the `distance`th ancestor
/// of `cs_id` (i.e. `cs_id~distance`)
#[derive(Debug)]
pub struct Location {
    pub cs_id: ChangesetId,
    pub distance: u64,
}
