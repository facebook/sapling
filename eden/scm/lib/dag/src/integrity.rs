/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Integrity checks.

use std::collections::BTreeSet;

use futures::StreamExt;
use futures::TryStreamExt;

use crate::iddag::IdDag;
use crate::iddagstore::IdDagStore;
use crate::idmap::IdMapAssignHead;
use crate::namedag::AbstractNameDag;
use crate::nameset::NameSet;
use crate::ops::CheckIntegrity;
use crate::ops::DagAlgorithm;
use crate::ops::IdConvert;
use crate::ops::Persist;
use crate::ops::TryClone;
use crate::segment::SegmentFlags;
use crate::Group;
use crate::Id;
use crate::Result;
use crate::VertexName;

#[async_trait::async_trait]
impl<IS, M, P, S> CheckIntegrity for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore + Persist + 'static,
    IdDag<IS>: TryClone,
    M: TryClone + IdMapAssignHead + Persist + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Persist + Send + Sync + 'static,
{
    async fn check_universal_ids(&self) -> Result<Vec<Id>> {
        let universal_ids: Vec<Id> = self.dag.universal_ids()?.into_iter().collect();
        tracing::debug!("{} universally known vertexes", universal_ids.len());
        let exists = self.map.contains_vertex_id_locally(&universal_ids).await?;
        let missing_ids = universal_ids
            .into_iter()
            .zip(exists)
            .filter_map(|(id, b)| if b { None } else { Some(id) })
            .collect();
        Ok(missing_ids)
    }

    async fn check_segments(&self) -> Result<Vec<String>> {
        // Detected problems.
        let mut problems = Vec::new();

        // Track heads and roots in the graph.
        let mut heads: BTreeSet<Id> = Default::default();
        let mut roots: BTreeSet<Id> = Default::default();

        for level in 0..=self.dag.max_level()? {
            let mut expected_low = Id::MIN;

            // Check all levels.
            for seg in self.dag.iter_segments_ascending(Id::MIN, level)? {
                let seg = seg?;
                let span = seg.span()?;
                let mut add_problem =
                    |msg| problems.push(format!("Level {} segment {:?} {}", level, &seg, msg));

                // Spans need to be sorted and non-overlapping within a group.
                if span.low.group() > expected_low.group() {
                    expected_low = span.low.group().min_id();
                }
                if span.low > span.high || span.low.group() != span.high.group() {
                    add_problem(format!("has invalid span {:?}", span));
                }
                if span.low < expected_low {
                    add_problem(format!(
                        "has unexpected span ({:?}), expected low ({:?})",
                        span, expected_low
                    ));
                }
                expected_low = span.high + 1;

                // Parents should be < low to avoid cycles, and should not have duplicates.
                let mut parents = seg.parents()?;
                let orig_parents_len = parents.len();
                parents.sort_unstable();
                parents.dedup();
                if parents.len() < orig_parents_len {
                    add_problem("has duplicated parents".to_string());
                }
                if parents.iter().any(|&p| p >= span.low) {
                    add_problem("has parents that might cause cycles".to_string());
                }

                // Maintain heads and roots. This can only be calculated from flat segments.
                if level == 0 {
                    let previous_head = heads.iter().rev().next().cloned();
                    if let Some(head) = previous_head {
                        if span.low <= head {
                            add_problem(format!(
                                "overlapped segments: {:?} with previous head {:?}",
                                span, head
                            ));
                        }
                    }
                    for p in &parents {
                        heads.remove(p);
                    }
                    heads.insert(span.high);
                    if parents.is_empty() {
                        roots.insert(span.low);
                    }
                }

                // Check flags. min: must have flags, max: all possible flags.
                let mut expected_flags_max = SegmentFlags::empty();
                let mut expected_flags_min = SegmentFlags::empty();
                if roots.range(span.low..=span.high).take(1).count() > 0 {
                    expected_flags_min |= SegmentFlags::HAS_ROOT;
                    expected_flags_max |= SegmentFlags::HAS_ROOT;
                }
                if level == 0 && heads.len() == 1 && span.high.group() == Group::MASTER {
                    // ONLY_HEAD is optional.
                    expected_flags_max |= SegmentFlags::ONLY_HEAD;
                }
                let flags = seg.flags()?;
                if !flags.contains(expected_flags_min) || !expected_flags_max.contains(flags) {
                    add_problem(format!(
                        "has unexpected flags: {:?} (expected: min: {:?}, max: {:?})",
                        flags, expected_flags_min, expected_flags_max
                    ));
                }

                // Check parents for high-level segments.
                if level > 0 {
                    let mut expected_parents = Vec::with_capacity(parents.len());
                    for seg in self.dag.iter_segments_ascending(span.low, level - 1)? {
                        let seg = seg?;
                        let subspan = seg.span()?;
                        if subspan.high > span.high && subspan.low <= span.high {
                            add_problem(format!(
                                "does not align with low-level segment {:?}",
                                &seg
                            ));
                        }
                        if subspan.low > span.high {
                            break;
                        }
                        for p in seg.parents()? {
                            if p < span.low {
                                expected_parents.push(p);
                            }
                        }
                    }
                    expected_parents.sort_unstable();
                    expected_parents.dedup();
                    if parents != expected_parents {
                        add_problem(format!(
                            "has unexpected parents (expected: {:?})",
                            expected_parents
                        ));
                    }
                }
            }
        }
        Ok(problems)
    }

    async fn check_isomorphic_graph(
        &self,
        other: &dyn DagAlgorithm,
        heads: NameSet,
    ) -> Result<Vec<String>> {
        let mut problems = Vec::new();

        // Prefetch merges and their parents in both graphs' master group.
        // This reduces round-trips.
        tracing::debug!("prefetching merges and parents");
        for graph in [self, other] as [&dyn DagAlgorithm; 2] {
            if !graph.is_vertex_lazy() {
                continue;
            }
            let related = graph.master_group().await? & graph.ancestors(heads.clone()).await?;
            let merges = graph.merges(related).await?;
            let parents = graph.parents(merges.clone()).await?;
            let prefetch = merges | parents;
            let mut iter = prefetch.iter().await?;
            while let Some(_) = iter.next().await {}
        }
        tracing::trace!("prefetched merges and parents");

        // To verify two graphs are isomorphic. Start from some heads,
        // check and compare their linear ancestors recursively.
        //
        // For example, starting from "to_check" having just "E":
        //
        //      A--C--D--E      B--C--D--E
        //        /               /
        //       B               A
        //
        // 1. Figure out the linear portion (C--D--E).
        // 2. Check the linear portion is the same in both graphs.
        //    (only its root C can be a merge and C has the same parents)
        // 3. Remove the head (E) from "to_check", insert root (C)'s parents
        //    to "to_check".
        // 4. Repeat from 1 until "to_check" is empty.
        let mut to_check: Vec<VertexName> = heads.iter().await?.try_collect().await?;
        let mut visited: BTreeSet<VertexName> = Default::default();
        while let Some(head) = to_check.pop() {
            if !visited.insert(head.clone()) {
                continue;
            }

            // Use flat segment to figure out the linear portion.
            let head_id = self.vertex_id(head.clone()).await?;
            let seg = match self.dag.find_flat_segment_including_id(head_id)? {
                Some(seg) => seg,
                None => {
                    problems.push(format!(
                        "head_id {:?} should be covered by a flat segment",
                        head_id
                    ));
                    continue;
                }
            };
            let span = seg.span()?;
            let root_id = span.low;
            let root = self.vertex_name(root_id).await?;
            let parents = self.parent_names(root.clone()).await?;
            let mut add_problem = |msg| {
                problems.push(format!(
                    "range {:?}::{:?} with parents {:?}: {}",
                    &root, &head, &parents, msg
                ));
            };
            tracing::trace!("checking range {:?}::{:?}", &root, &head);

            // Check against the other graph for various properties.
            // Check vertex count in the range.
            let this_count = head_id.0 - root_id.0 + 1;
            let set = match other.range(root.clone().into(), head.clone().into()).await {
                Ok(set) => set,
                Err(e) => {
                    add_problem(format!("cannot resolve range on the other graph: {:?}", e));
                    continue;
                }
            };
            let other_count = set.count().await? as u64;
            if other_count != this_count {
                add_problem(format!(
                    "length mismatch: {} != {}",
                    this_count, other_count
                ));
            }

            // Check that merge can only be at most 1 (`root`).
            let other_merges = other.merges(set).await?.count().await?;
            let this_merges = if parents.len() > 1 { 1 } else { 0 };
            if other_merges != this_merges {
                add_problem(format!(
                    "merge mismatch: {} != {}",
                    this_merges, other_merges
                ));
            }

            // Check parents of root.
            let other_parents = match other.parent_names(root.clone()).await {
                Ok(ps) => ps,
                Err(e) => {
                    add_problem(format!(
                        "cannot get parents of {:?} on the other graph: {:?}",
                        &root, e
                    ));
                    continue;
                }
            };
            if other_parents != parents {
                add_problem(format!(
                    "parents mismatch: {:?} != {:?}",
                    &parents, other_parents
                ));
            }

            // Check parents recursively.
            to_check.extend(parents);
        }

        Ok(problems)
    }
}
