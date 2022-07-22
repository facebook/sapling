/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::cmp::Ordering;
use std::fmt;

use futures::StreamExt;

use super::hints::Flags;
use super::AsyncNameSetQuery;
use super::BoxVertexStream;
use super::Hints;
use super::NameSet;
use crate::fmt::write_debug;
use crate::Id;
use crate::Result;
use crate::VertexName;

/// Intersection of 2 sets.
///
/// The iteration order is defined by the first set.
pub struct IntersectionSet {
    lhs: NameSet,
    rhs: NameSet,
    hints: Hints,
}

struct Iter {
    iter: BoxVertexStream,
    rhs: NameSet,
    ended: bool,

    /// Optional fast path for stop.
    stop_condition: Option<StopCondition>,
}

impl Iter {
    async fn next(&mut self) -> Option<Result<VertexName>> {
        if self.ended {
            return None;
        }
        loop {
            let result = self.iter.as_mut().next().await;
            if let Some(Ok(ref name)) = result {
                match self.rhs.contains(&name).await {
                    Err(err) => break Some(Err(err)),
                    Ok(false) => {
                        // Check if we can stop iteration early using hints.
                        if let Some(ref cond) = self.stop_condition {
                            if let Some(id_convert) = self.rhs.id_convert() {
                                if let Ok(Some(id)) = id_convert.vertex_id_optional(&name).await {
                                    if cond.should_stop_with_id(id) {
                                        self.ended = true;
                                        return None;
                                    }
                                }
                            }
                        }
                        continue;
                    }
                    Ok(true) => {}
                }
            }
            break result;
        }
    }

    fn into_stream(self) -> BoxVertexStream {
        Box::pin(futures::stream::unfold(self, |mut state| async move {
            let result = state.next().await;
            result.map(|r| (r, state))
        }))
    }
}

struct StopCondition {
    order: Ordering,
    id: Id,
}

impl StopCondition {
    fn should_stop_with_id(&self, id: Id) -> bool {
        id.cmp(&self.id) == self.order
    }
}

impl IntersectionSet {
    pub fn new(lhs: NameSet, rhs: NameSet) -> Self {
        // More efficient if `lhs` is smaller. Swap `lhs` and `rhs` if `lhs` is `FULL`.
        let (lhs, rhs) = if lhs.hints().contains(Flags::FULL)
            && !rhs.hints().contains(Flags::FULL)
            && !rhs.hints().contains(Flags::FILTER)
            && lhs.hints().dag_version() >= rhs.hints().dag_version()
        {
            (rhs, lhs)
        } else {
            (lhs, rhs)
        };

        let hints = Hints::new_inherit_idmap_dag(lhs.hints());
        hints.add_flags(
            lhs.hints().flags()
                & (Flags::EMPTY
                    | Flags::ID_DESC
                    | Flags::ID_ASC
                    | Flags::TOPO_DESC
                    | Flags::FILTER),
        );
        // Only keep the ANCESTORS flag if lhs and rhs use a compatible Dag.
        if lhs.hints().dag_version() >= rhs.hints().dag_version() {
            hints.add_flags(lhs.hints().flags() & rhs.hints().flags() & Flags::ANCESTORS);
        }
        let (rhs_min_id, rhs_max_id) = if hints.id_map_version() >= rhs.hints().id_map_version() {
            // rhs ids are all known by lhs.
            (rhs.hints().min_id(), rhs.hints().max_id())
        } else {
            (None, None)
        };
        match (lhs.hints().min_id(), rhs_min_id) {
            (Some(id), None) | (None, Some(id)) => {
                hints.set_min_id(id);
            }
            (Some(id1), Some(id2)) => {
                hints.set_min_id(id1.max(id2));
            }
            (None, None) => {}
        }
        match (lhs.hints().max_id(), rhs_max_id) {
            (Some(id), None) | (None, Some(id)) => {
                hints.set_max_id(id);
            }
            (Some(id1), Some(id2)) => {
                hints.set_max_id(id1.min(id2));
            }
            (None, None) => {}
        }
        Self { lhs, rhs, hints }
    }

    fn is_rhs_id_map_comapatible(&self) -> bool {
        let lhs_version = self.lhs.hints().id_map_version();
        let rhs_version = self.rhs.hints().id_map_version();
        lhs_version == rhs_version || (lhs_version > rhs_version && rhs_version > None)
    }
}

#[async_trait::async_trait]
impl AsyncNameSetQuery for IntersectionSet {
    async fn iter(&self) -> Result<BoxVertexStream> {
        let stop_condition = if !self.is_rhs_id_map_comapatible() {
            None
        } else if self.lhs.hints().contains(Flags::ID_ASC) {
            if let Some(id) = self.rhs.hints().max_id() {
                Some(StopCondition {
                    id,
                    order: Ordering::Greater,
                })
            } else {
                None
            }
        } else if self.lhs.hints().contains(Flags::ID_DESC) {
            if let Some(id) = self.rhs.hints().min_id() {
                Some(StopCondition {
                    id,
                    order: Ordering::Less,
                })
            } else {
                None
            }
        } else {
            None
        };

        let iter = Iter {
            iter: self.lhs.iter().await?,
            rhs: self.rhs.clone(),
            ended: false,
            stop_condition,
        };
        Ok(iter.into_stream())
    }

    async fn iter_rev(&self) -> Result<BoxVertexStream> {
        let stop_condition = if !self.is_rhs_id_map_comapatible() {
            None
        } else if self.lhs.hints().contains(Flags::ID_DESC) {
            if let Some(id) = self.rhs.hints().max_id() {
                Some(StopCondition {
                    id,
                    order: Ordering::Greater,
                })
            } else {
                None
            }
        } else if self.lhs.hints().contains(Flags::ID_ASC) {
            if let Some(id) = self.rhs.hints().min_id() {
                Some(StopCondition {
                    id,
                    order: Ordering::Less,
                })
            } else {
                None
            }
        } else {
            None
        };

        let iter = Iter {
            iter: self.lhs.iter_rev().await?,
            rhs: self.rhs.clone(),
            ended: false,
            stop_condition,
        };
        Ok(iter.into_stream())
    }

    async fn contains(&self, name: &VertexName) -> Result<bool> {
        Ok(self.lhs.contains(name).await? && self.rhs.contains(name).await?)
    }

    async fn contains_fast(&self, name: &VertexName) -> Result<Option<bool>> {
        for set in &[&self.lhs, &self.rhs] {
            let contains = set.contains_fast(name).await?;
            match contains {
                Some(false) | None => return Ok(contains),
                Some(true) => {}
            }
        }
        Ok(Some(true))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn hints(&self) -> &Hints {
        &self.hints
    }
}

impl fmt::Debug for IntersectionSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<and")?;
        write_debug(f, &self.lhs)?;
        write_debug(f, &self.rhs)?;
        write!(f, ">")
    }
}

#[cfg(test)]
#[allow(clippy::redundant_clone)]
mod tests {
    use std::collections::HashSet;

    use super::super::id_lazy::tests::lazy_set;
    use super::super::id_lazy::tests::lazy_set_inherit;
    use super::super::tests::*;
    use super::*;
    use crate::Id;

    fn intersection(a: &[u8], b: &[u8]) -> IntersectionSet {
        let a = NameSet::from_query(VecQuery::from_bytes(a));
        let b = NameSet::from_query(VecQuery::from_bytes(b));
        IntersectionSet::new(a, b)
    }

    #[test]
    fn test_intersection_basic() -> Result<()> {
        let set = intersection(b"\x11\x33\x55\x22\x44", b"\x44\x33\x66");
        check_invariants(&set)?;
        assert_eq!(shorten_iter(ni(set.iter())), ["33", "44"]);
        assert_eq!(shorten_iter(ni(set.iter_rev())), ["44", "33"]);
        assert!(!nb(set.is_empty())?);
        assert_eq!(nb(set.count())?, 2);
        assert_eq!(shorten_name(nb(set.first())?.unwrap()), "33");
        assert_eq!(shorten_name(nb(set.last())?.unwrap()), "44");
        for &b in b"\x11\x22\x55\x66".iter() {
            assert!(!nb(set.contains(&to_name(b)))?);
        }
        Ok(())
    }

    #[test]
    fn test_intersection_min_max_id_fast_path() {
        // The min_ids are intentionally wrong to test the fast paths.
        let a = lazy_set(&[0x70, 0x60, 0x50, 0x40, 0x30, 0x20]);
        let b = lazy_set_inherit(&[0x70, 0x65, 0x50, 0x40, 0x35, 0x20], &a);
        let a = NameSet::from_query(a);
        let b = NameSet::from_query(b);
        a.hints().add_flags(Flags::ID_DESC);
        b.hints().set_min_id(Id(0x40));
        b.hints().set_max_id(Id(0x50));

        let set = IntersectionSet::new(a, b.clone());
        // No "20" - filtered out by min id fast path.
        assert_eq!(shorten_iter(ni(set.iter())), ["70", "50", "40"]);
        // No "70" - filtered out by max id fast path.
        assert_eq!(shorten_iter(ni(set.iter_rev())), ["20", "40", "50"]);

        // Test the reversed sort order.
        let a = lazy_set(&[0x20, 0x30, 0x40, 0x50, 0x60, 0x70]);
        let b = lazy_set_inherit(&[0x70, 0x65, 0x50, 0x40, 0x35, 0x20], &a);
        let a = NameSet::from_query(a);
        let b = NameSet::from_query(b);
        a.hints().add_flags(Flags::ID_ASC);
        b.hints().set_min_id(Id(0x40));
        b.hints().set_max_id(Id(0x50));
        let set = IntersectionSet::new(a, b.clone());
        // No "70".
        assert_eq!(shorten_iter(ni(set.iter())), ["20", "40", "50"]);
        // No "20".
        assert_eq!(shorten_iter(ni(set.iter_rev())), ["70", "50", "40"]);

        // If two sets have incompatible IdMap, fast paths are not used.
        let a = NameSet::from_query(lazy_set(&[0x20, 0x30, 0x40, 0x50, 0x60, 0x70]));
        a.hints().add_flags(Flags::ID_ASC);
        let set = IntersectionSet::new(a, b.clone());
        // Should contain "70" and "20".
        assert_eq!(shorten_iter(ni(set.iter())), ["20", "40", "50", "70"]);
        assert_eq!(shorten_iter(ni(set.iter_rev())), ["70", "50", "40", "20"]);
    }

    quickcheck::quickcheck! {
        fn test_intersection_quickcheck(a: Vec<u8>, b: Vec<u8>) -> bool {
            let set = intersection(&a, &b);
            check_invariants(&set).unwrap();

            let count = nb(set.count()).unwrap();
            assert!(count <= a.len(), "len({:?}) = {} should <= len({:?})" , &set, count, &a);
            assert!(count <= b.len(), "len({:?}) = {} should <= len({:?})" , &set, count, &b);

            let contains_a: HashSet<u8> = a.into_iter().filter(|&b| nb(set.contains(&to_name(b))).ok() == Some(true)).collect();
            let contains_b: HashSet<u8> = b.into_iter().filter(|&b| nb(set.contains(&to_name(b))).ok() == Some(true)).collect();
            assert_eq!(contains_a, contains_b);

            true
        }
    }
}
