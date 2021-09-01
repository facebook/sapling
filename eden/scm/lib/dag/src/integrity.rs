/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Integrity checks.

use crate::iddag::IdDag;
use crate::iddagstore::IdDagStore;
use crate::idmap::IdMapAssignHead;
use crate::namedag::AbstractNameDag;
use crate::nameset::NameSet;
use crate::ops::CheckIntegrity;
use crate::ops::DagAlgorithm;
use crate::ops::Persist;
use crate::ops::TryClone;
use crate::segment::SegmentFlags;
use crate::Group;
use crate::Id;
use crate::Result;
use std::collections::BTreeSet;

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

                // Spans need to be continuous within a group.
                if span.low.group() > expected_low.group() {
                    expected_low = span.low.group().min_id();
                }
                if span.low > span.high || span.low.group() != span.high.group() {
                    add_problem(format!("has invalid span {:?}", span));
                }
                if span.low != expected_low {
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
                    for p in &parents {
                        heads.remove(p);
                    }
                    heads.insert(span.high);
                    if parents.is_empty() {
                        roots.insert(span.low);
                    }
                }

                // Check flags.
                let mut expected_flags = SegmentFlags::empty();
                if roots.range(span.low..=span.high).take(1).count() > 0 {
                    expected_flags |= SegmentFlags::HAS_ROOT;
                }
                if level == 0 && heads.len() == 1 && span.high.group() == Group::MASTER {
                    expected_flags |= SegmentFlags::ONLY_HEAD;
                }
                let flags = seg.flags()?;
                if flags != expected_flags {
                    add_problem(format!(
                        "has unexpected flags: {:?} (expected: {:?})",
                        flags, expected_flags
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
        let _ = (other, heads);
        unimplemented!();
    }
}
