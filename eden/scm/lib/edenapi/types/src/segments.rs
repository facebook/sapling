/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;

use anyhow::bail;
use anyhow::Result;
use dag_types::CloneData;
use dag_types::FlatSegment;
use dag_types::Id;
use dag_types::PreparedFlatSegments;
use dag_types::VertexName;

use crate::commit::CommitGraphSegmentsEntry;

pub struct CommitGraphSegments {
    pub segments: Vec<CommitGraphSegmentsEntry>,
}

impl TryFrom<CommitGraphSegments> for CloneData<VertexName> {
    type Error = anyhow::Error;

    /// Convert server-provided commit graph segments into valid clone data by
    /// assigning ids to each segment.
    ///
    /// If any segments are encountered with external parents (that is, parents
    /// that are not members of any of these segments and for which their location
    /// is `None`), those parents are assigned temporary ids in a range starting
    /// at 0.  All proper segments will be assigned ids above the temporary id
    /// range.
    ///
    /// If all segment parents are internal (i.e. have a location referring to
    /// another segment in the set), then the id range will start at 0.
    fn try_from(graph_segments: CommitGraphSegments) -> Result<Self> {
        // Work out an upper-bound on the number of parents without a location
        // (there may be duplicates, but that's not important for reserving enough
        // id space).
        let first_id = graph_segments
            .segments
            .iter()
            .map(|segment| {
                segment
                    .parents
                    .iter()
                    .filter(|parent| parent.location.is_none())
                    .count() as u64
            })
            .sum();
        let mut next_parent_id = 0;
        let mut next_id = first_id;
        let mut name_map = HashMap::new();
        let mut flat_segments = PreparedFlatSegments {
            segments: BTreeSet::new(),
        };
        // Segments are in reverse topological order, so start from the end.
        for segment in graph_segments.segments.into_iter().rev() {
            let low = Id(next_id);
            let high = Id(next_id + segment.length - 1);
            next_id += segment.length;
            let mut parents = Vec::with_capacity(segment.parents.len());
            for parent in segment.parents {
                if let Some(location) = &parent.location {
                    if let Some(Id(id)) = name_map.get(&location.descendant) {
                        let parent_id = Id(*id - location.distance);
                        name_map.insert(parent.hgid, parent_id);
                        parents.push(parent_id);
                    } else {
                        bail!(
                            "Couldn't find parent of {} as {}~{}",
                            segment.base,
                            location.descendant,
                            location.distance
                        );
                    }
                } else if let Some(parent_id) = name_map.get(&parent.hgid) {
                    parents.push(*parent_id);
                } else if next_parent_id >= first_id {
                    bail!("Programming error: not enough ids reserved for external parents");
                } else {
                    let parent_id = Id(next_parent_id);
                    next_parent_id += 1;
                    parents.push(parent_id);
                    name_map.insert(parent.hgid, parent_id);
                }
            }
            name_map.insert(segment.base, low);
            name_map.insert(segment.head, high);
            flat_segments
                .segments
                .insert(FlatSegment { low, high, parents });
        }
        Ok(CloneData {
            flat_segments,
            idmap: name_map
                .into_iter()
                .map(|(name, id)| (id, VertexName::copy_from(&name.into_byte_array())))
                .collect(),
        })
    }
}
