/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::ops::Bound;

use anyhow::bail;
use anyhow::Result;
use dag_types::CloneData;
use dag_types::FlatSegment;
use dag_types::Id;
use dag_types::Location;
use dag_types::PreparedFlatSegments;
use dag_types::VertexName;
use types::HgId;

use crate::commit::CommitGraphSegmentParent;
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

impl TryFrom<CloneData<VertexName>> for CommitGraphSegments {
    type Error = anyhow::Error;

    fn try_from(clone_data: CloneData<VertexName>) -> Result<Self> {
        let CloneData {
            flat_segments,
            idmap,
        } = clone_data;
        let idmap = idmap
            .into_iter()
            .map(|(id, name)| anyhow::Ok((id, HgId::from_slice(name.as_ref())?)))
            .collect::<Result<BTreeMap<_, _>>>()?;

        let mut relidmap: BTreeMap<Id, (HgId, Location<HgId>)> = BTreeMap::new();
        for flat_segment in flat_segments.segments.iter() {
            let high_name = &idmap[&flat_segment.high];
            for (id, name) in idmap.range((
                Bound::Excluded(&flat_segment.low),
                Bound::Included(&flat_segment.high),
            )) {
                relidmap.insert(
                    *id,
                    (
                        name.clone(),
                        Location::new(high_name.clone(), flat_segment.high.0 - id.0),
                    ),
                );
            }
            let low_name = &idmap[&flat_segment.low];
            relidmap.insert(
                flat_segment.low,
                (low_name.clone(), Location::new(low_name.clone(), 0)),
            );
        }

        let segments = flat_segments
            .segments
            .into_iter()
            .rev()
            .map(|flat_segment| CommitGraphSegmentsEntry {
                head: idmap[&flat_segment.high].clone(),
                base: idmap[&flat_segment.low].clone(),
                length: flat_segment.high.0 - flat_segment.low.0 + 1,
                parents: flat_segment
                    .parents
                    .into_iter()
                    .map(|parent_id| {
                        relidmap.get(&parent_id).map_or_else(
                            || CommitGraphSegmentParent {
                                hgid: idmap[&parent_id].clone(),
                                location: None,
                            },
                            |(parent_name, location)| CommitGraphSegmentParent {
                                hgid: parent_name.clone(),
                                location: Some(location.clone()),
                            },
                        )
                    })
                    .collect(),
            })
            .collect();

        Ok(CommitGraphSegments { segments })
    }
}
