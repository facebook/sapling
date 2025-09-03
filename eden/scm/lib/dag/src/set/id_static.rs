/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::borrow::Cow;
use std::cmp;
use std::fmt;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use nonblocking::non_blocking_result;

use super::AsyncSetQuery;
use super::BoxVertexStream;
use super::Hints;
use super::hints::Flags;
use crate::Group;
use crate::Id;
use crate::IdSet;
use crate::Result;
use crate::Set;
use crate::Vertex;
use crate::idset::IdList;
use crate::idset::OrderedSpan;
use crate::ops::DagAlgorithm;
use crate::ops::IdConvert;
use crate::protocol::disable_remote_protocol;

/// A set backed by [`IdSet`] + [`IdMap`].
/// Efficient for DAG calculation.
#[derive(Clone)]
pub struct IdStaticSet {
    spans: IdSet,
    pub(crate) map: Arc<dyn IdConvert + Send + Sync>,
    pub(crate) dag: Arc<dyn DagAlgorithm + Send + Sync>,
    hints: Hints,
    // If true, iterate in ASC order instead of DESC.
    iteration_order: IterationOrder,
}

/// Iteration order of the `IdStaticSet`.
#[derive(Clone, Debug)]
enum IterationOrder {
    /// From smaller ids to larger ids.
    Asc,
    /// From larger ids to smaller ids.
    Desc,
    /// Custom iteration order. Must match `IdStaticSet.spans`.
    Custom(IdList),
    /// Custom iteration order, reversed.
    CustomReversed(IdList),
}

/// Basic iteration order. A subset of `IterationOrder`.
#[derive(Copy, Clone, Debug)]
pub enum BasicIterationOrder {
    /// From smaller ids to larger ids.
    Asc,
    /// From larger ids to smaller ids.
    Desc,
}

struct Iter {
    iter: Box<dyn DoubleEndedIterator<Item = Id> + Send + Sync + 'static>,
    map: Arc<dyn IdConvert + Send + Sync>,
    reversed: bool,
    buf: Vec<Result<Vertex>>,
}

impl Iter {
    fn into_box_stream(self) -> BoxVertexStream {
        Box::pin(futures::stream::unfold(self, |this| this.next()))
    }

    async fn next(mut self) -> Option<(Result<Vertex>, Self)> {
        if let Some(name) = self.buf.pop() {
            return Some((name, self));
        }
        let map = &self.map;
        let opt_id = if self.reversed {
            self.iter.next_back()
        } else {
            self.iter.next()
        };
        match opt_id {
            None => None,
            Some(id) => {
                let contains = map
                    .contains_vertex_id_locally(&[id])
                    .await
                    .unwrap_or_default();
                if contains == [true] {
                    Some((map.vertex_name(id).await, self))
                } else {
                    // On demand prefetch in batch.
                    let batch_size = crate::config::BATCH_SIZE.load(Ordering::Acquire);
                    let mut ids = Vec::with_capacity(batch_size);
                    ids.push(id);
                    for _ in ids.len()..batch_size {
                        if let Some(id) = if self.reversed {
                            self.iter.next_back()
                        } else {
                            self.iter.next()
                        } {
                            ids.push(id);
                        } else {
                            break;
                        }
                    }
                    ids.reverse();
                    self.buf = match self.map.vertex_name_batch(&ids).await {
                        Err(e) => return Some((Err(e), self)),
                        Ok(names) => names,
                    };
                    if self.buf.len() != ids.len() {
                        let result =
                            crate::errors::bug("vertex_name_batch does not return enough items");
                        return Some((result, self));
                    }
                    let name = self.buf.pop().expect("buf is not empty");
                    Some((name, self))
                }
            }
        }
    }
}

struct DebugSpan {
    // start, end are for debug fmt, not iteration order.
    span: OrderedSpan,
    end_name: Option<Vertex>,
    start_name: Option<Vertex>,
}

impl fmt::Debug for DebugSpan {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match (
            self.span.start == self.span.end,
            &self.start_name,
            &self.end_name,
        ) {
            (true, Some(name), _) => {
                fmt::Debug::fmt(&name, f)?;
                write!(f, "+{:?}", self.span.start)?;
            }
            (true, None, _) => {
                write!(f, "{:?}", self.span.start)?;
            }
            (false, Some(start), Some(end)) => {
                fmt::Debug::fmt(&start, f)?;
                write!(f, ":")?;
                fmt::Debug::fmt(&end, f)?;
                write!(f, "+{:?}:{:?}", self.span.start, self.span.end)?;
            }
            (false, _, _) => {
                write!(f, "{:?}:{:?}", self.span.start, self.span.end)?;
            }
        }
        Ok(())
    }
}

impl fmt::Debug for IdStaticSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<spans ")?;
        let spans_iter: Box<dyn Iterator<Item = OrderedSpan>> = match self.iteration_order {
            IterationOrder::Custom(ref list) | IterationOrder::CustomReversed(ref list) => {
                Box::new(list.as_spans().iter().copied())
            }
            _ => Box::new(self.spans.as_spans().iter().map(|s| OrderedSpan {
                start: s.low,
                end: s.high,
            })),
        };

        let len = spans_iter.size_hint().0;
        let limit = f.width().unwrap_or(3);
        f.debug_list()
            .entries(spans_iter.take(limit).map(|span| DebugSpan {
                span,
                end_name: disable_remote_protocol(|| {
                    non_blocking_result(self.map.vertex_name(span.end)).ok()
                }),
                start_name: disable_remote_protocol(|| {
                    non_blocking_result(self.map.vertex_name(span.start)).ok()
                }),
            }))
            .finish()?;
        match len.saturating_sub(limit) {
            0 => {}
            1 => write!(f, " + 1 span")?,
            n => write!(f, " + {} spans", n)?,
        }
        match &self.iteration_order {
            // + means ASC order.
            IterationOrder::Asc => write!(f, " +")?,
            // For compatibility with existing tests, do not show a sign for DESC (default) order.
            // Otherwise this should show "-".
            IterationOrder::Desc => {}
            IterationOrder::Custom(_) => write!(f, " ?")?,
            IterationOrder::CustomReversed(_) => write!(f, " ¿")?,
        }
        write!(f, ">")?;
        Ok(())
    }
}

impl IdStaticSet {
    pub(crate) fn from_id_set_idmap_dag(
        spans: IdSet,
        map: Arc<dyn IdConvert + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
    ) -> Self {
        let hints = Hints::new_with_idmap_dag(map.clone(), dag.clone());
        hints.add_flags(Flags::ID_DESC | Flags::TOPO_DESC);
        if spans.is_empty() {
            hints.add_flags(Flags::EMPTY);
        } else {
            hints.set_min_id(spans.min().unwrap());
            hints.set_max_id(spans.max().unwrap());
        }
        Self {
            spans,
            map,
            hints,
            dag,
            iteration_order: IterationOrder::Desc,
        }
    }

    /// Construct from `list`, `map`, `dag`. The ids in the `list` must match the map and dag.
    pub(crate) fn from_id_list_idmap_dag(
        list: IdList,
        map: Arc<dyn IdConvert + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
    ) -> Self {
        let hints = Hints::new_with_idmap_dag(map.clone(), dag.clone());

        // Calculate hints (flags, min_id, max_id).
        let mut flags = Flags::ID_DESC | Flags::TOPO_DESC | Flags::ID_ASC | Flags::EMPTY;
        let mut min_id = None;
        let mut max_id = None;
        let mut last_min_id = None;
        let mut last_max_id = None;
        for span in list.as_spans() {
            let (this_min_id, this_max_id) = (span.min(), span.max());
            flags -= Flags::EMPTY;
            if span.start < span.end || last_min_id.unwrap_or(Id::MAX) < this_max_id {
                // Not DESC or TOPO.
                flags -= Flags::ID_DESC | Flags::TOPO_DESC;
            }
            if span.start > span.end || last_max_id.unwrap_or(Id::MIN) > this_min_id {
                // Not ASC.
                flags -= Flags::ID_ASC;
            }
            (last_min_id, last_max_id) = (Some(this_min_id), Some(this_max_id));
            min_id = Some(this_min_id.min(min_id.unwrap_or(Id::MAX)));
            max_id = Some(this_max_id.max(max_id.unwrap_or(Id::MIN)));
        }

        hints.add_flags(flags);
        if let Some(min_id) = min_id {
            hints.set_min_id(min_id);
        }
        if let Some(max_id) = max_id {
            hints.set_max_id(max_id);
        }

        let spans = list.to_set();

        // If `list` is already sorted, then just use BasicIterationOrder.
        let iteration_order = if flags.contains(Flags::ID_DESC) {
            IterationOrder::Desc
        } else if flags.contains(Flags::ID_ASC) {
            IterationOrder::Asc
        } else {
            IterationOrder::Custom(list)
        };

        Self {
            spans,
            map,
            hints,
            dag,
            iteration_order,
        }
    }

    /// Get the low-level `IdSet`, which no longer preserves iteration order.
    pub(crate) fn id_set_losing_order(&self) -> &IdSet {
        &self.spans
    }

    /// Get the low-level `IdSet`, or `None` if iteration order cannot be preserved.
    /// Note: `reserved` is not preserved and needs to be considered separately.
    pub(crate) fn id_set_try_preserving_order(&self) -> Option<&IdSet> {
        if self
            .hints()
            .flags()
            .intersects(Flags::ID_DESC | Flags::ID_ASC)
        {
            Some(&self.spans)
        } else {
            None
        }
    }

    /// If `lhs` and `rhs` are compatible, return a new IdStaticSet with:
    /// - `map` and `dag` set to the newer version of `lhs` and `rhs`.
    /// - `spans` set to `edit_spans(&lhs.spans, &rhs.spans)`.
    ///
    /// Otherwise return `None`.
    ///
    /// Iteration order will not be preserved.
    pub(crate) fn from_edit_spans(
        lhs: &Self,
        rhs: &Self,
        edit_spans: fn(&IdSet, &IdSet) -> IdSet,
    ) -> Option<Self> {
        let order = lhs.map.map_version().partial_cmp(rhs.map.map_version())?;
        let spans = edit_spans(&lhs.spans, &rhs.spans);
        let picked = match order {
            cmp::Ordering::Less => rhs,
            cmp::Ordering::Greater | cmp::Ordering::Equal => lhs,
        };
        let (map, dag) = (picked.map.clone(), picked.dag.clone());
        let mut result = Self::from_id_set_idmap_dag(spans, map, dag);
        if let Some(order) = lhs.iteration_order() {
            result.set_iteration_order(order);
        }
        Some(result)
    }

    /// Change the iteration order between (DESC default) and ASC.
    pub fn reversed(mut self) -> Self {
        match self.iteration_order {
            IterationOrder::Desc => {
                self.hints.remove_flags(Flags::ID_DESC | Flags::TOPO_DESC);
                self.hints.add_flags(Flags::ID_ASC);
                self.iteration_order = IterationOrder::Asc
            }
            IterationOrder::Asc => {
                self.hints.remove_flags(Flags::ID_ASC);
                self.hints.add_flags(Flags::ID_DESC | Flags::TOPO_DESC);
                self.iteration_order = IterationOrder::Desc
            }
            IterationOrder::Custom(list) => {
                // Conservatively drop order-related flags.
                self.hints
                    .remove_flags(Flags::ID_ASC | Flags::ID_DESC | Flags::TOPO_DESC);
                self.iteration_order = IterationOrder::CustomReversed(list);
            }
            IterationOrder::CustomReversed(list) => {
                self.hints
                    .remove_flags(Flags::ID_ASC | Flags::ID_DESC | Flags::TOPO_DESC);
                self.iteration_order = IterationOrder::Custom(list);
            }
        }
        self
    }

    /// Update iteration order. Only `Asc` and `Desc` is accepted.
    pub(crate) fn set_iteration_order(&mut self, order: BasicIterationOrder) {
        // Only reuse Asc or Desc order. Cannot handle custom order.
        match order {
            BasicIterationOrder::Asc => {
                self.hints.remove_flags(Flags::ID_DESC | Flags::TOPO_DESC);
                self.hints.add_flags(Flags::ID_ASC);
                self.iteration_order = IterationOrder::Asc;
            }
            BasicIterationOrder::Desc => {
                self.hints.remove_flags(Flags::ID_ASC);
                self.hints.add_flags(Flags::ID_DESC | Flags::TOPO_DESC);
                self.iteration_order = IterationOrder::Desc
            }
        }
    }

    /// Obtain the iteration order. Only `Asc` and `Desc` is returned. Otherwise report as `None`.
    pub(crate) fn iteration_order(&self) -> Option<BasicIterationOrder> {
        match self.iteration_order {
            IterationOrder::Asc => Some(BasicIterationOrder::Asc),
            IterationOrder::Desc => Some(BasicIterationOrder::Desc),
            #[allow(unreachable_patterns)]
            _ => None,
        }
    }

    async fn max(&self) -> Result<Option<Vertex>> {
        debug_assert_eq!(self.spans.max(), self.spans.iter_desc().nth(0));
        self.resolve_optional_id(self.spans.max()).await
    }

    async fn min(&self) -> Result<Option<Vertex>> {
        debug_assert_eq!(self.spans.min(), self.spans.iter_desc().rev().nth(0));
        self.resolve_optional_id(self.spans.min()).await
    }

    async fn resolve_optional_id(&self, id: Option<Id>) -> Result<Option<Vertex>> {
        match id {
            Some(id) => {
                let map = &self.map;
                let name = map.vertex_name(id).await?;
                Ok(Some(name))
            }
            None => Ok(None),
        }
    }

    pub(crate) fn slice_spans(mut self, skip: u64, take: u64) -> Self {
        let len = self.spans.count();
        let (skip, mut take) = match self.iteration_order {
            IterationOrder::Asc | IterationOrder::CustomReversed(_) => {
                // [---take1----][skip]
                // [skip2][take2][skip]
                // [--------len-------]
                let take1 = len.saturating_sub(skip);
                let take2 = take1.min(take);
                let skip2 = take1 - take2;
                (skip2, take2)
            }
            IterationOrder::Desc | IterationOrder::Custom(_) => {
                // [skip][take][---]
                // [------len------]
                (skip, take)
            }
        };
        if skip >= len {
            take = 0;
        } else if skip.saturating_add(take) >= len {
            take = u64::MAX;
        }
        if skip != 0 || take != u64::MAX {
            match self.iteration_order {
                IterationOrder::Custom(ref mut list)
                | IterationOrder::CustomReversed(ref mut list) => {
                    match (skip, take) {
                        (0, _) => *list = list.take(take),
                        (_, u64::MAX) => *list = list.skip(skip),
                        _ => *list = list.skip(skip).take(take),
                    };
                    self.spans = list.to_set();
                }
                _ => match (skip, take) {
                    (0, _) => self.spans = self.spans.take(take),
                    (_, u64::MAX) => self.spans = self.spans.skip(skip),
                    _ => self.spans = self.spans.skip(skip).take(take),
                },
            }
            // No longer sound to keep the ANCESTORS hint.
            self.hints.remove_flags(Flags::ANCESTORS);
        }
        self
    }

    // used by iter and iter_rev.
    fn get_iter_and_reversed(
        &self,
    ) -> (
        Box<dyn DoubleEndedIterator<Item = Id> + Send + Sync + 'static>,
        bool,
    ) {
        let iter: Box<dyn DoubleEndedIterator<Item = Id> + Send + Sync + 'static> =
            match self.iteration_order {
                IterationOrder::Custom(ref list) | IterationOrder::CustomReversed(ref list) => {
                    Box::new(list.into_iter())
                }
                _ => Box::new(self.spans.clone().into_iter()),
            };
        let reversed = matches!(
            self.iteration_order,
            IterationOrder::Asc | IterationOrder::CustomReversed(_)
        );
        (iter, reversed)
    }
}

#[async_trait::async_trait]
impl AsyncSetQuery for IdStaticSet {
    async fn iter(&self) -> Result<BoxVertexStream> {
        let (iter, reversed) = self.get_iter_and_reversed();
        let iter = Iter {
            iter,
            map: self.map.clone(),
            reversed,
            buf: Default::default(),
        };
        Ok(iter.into_box_stream())
    }

    async fn iter_rev(&self) -> Result<BoxVertexStream> {
        let (iter, reversed) = self.get_iter_and_reversed();
        let iter = Iter {
            iter,
            map: self.map.clone(),
            reversed: !reversed,
            buf: Default::default(),
        };
        Ok(iter.into_box_stream())
    }

    // Usually, the "count" should not be manually implemented so the universal fast path can
    // apply. However, the IdStaticSet does not need a separate "universal fast path".
    // So let's just override the "count".
    async fn count(&self) -> Result<u64> {
        Ok(self.spans.count())
    }

    async fn count_slow(&self) -> Result<u64> {
        Ok(self.spans.count())
    }

    async fn size_hint(&self) -> (u64, Option<u64>) {
        let size = self.spans.count();
        (size, Some(size))
    }

    async fn first(&self) -> Result<Option<Vertex>> {
        match self.iteration_order {
            IterationOrder::Asc => self.min().await,
            IterationOrder::Desc => self.max().await,
            IterationOrder::Custom(ref list) => {
                self.resolve_optional_id(list.into_iter().next()).await
            }
            IterationOrder::CustomReversed(ref list) => {
                self.resolve_optional_id(list.into_iter().next_back()).await
            }
        }
    }

    async fn last(&self) -> Result<Option<Vertex>> {
        match self.iteration_order {
            IterationOrder::Asc => self.max().await,
            IterationOrder::Desc => self.min().await,
            IterationOrder::Custom(ref list) => {
                self.resolve_optional_id(list.into_iter().next_back()).await
            }
            IterationOrder::CustomReversed(ref list) => {
                self.resolve_optional_id(list.into_iter().next()).await
            }
        }
    }

    async fn is_empty(&self) -> Result<bool> {
        Ok(self.spans.is_empty())
    }

    async fn contains(&self, name: &Vertex) -> Result<bool> {
        let result = match self.map.vertex_id_with_max_group(name, Group::MAX).await? {
            Some(id) => self.spans.contains(id),
            None => false,
        };
        Ok(result)
    }

    async fn contains_fast(&self, name: &Vertex) -> Result<Option<bool>> {
        self.contains(name).await.map(Some)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn hints(&self) -> &Hints {
        &self.hints
    }

    fn id_convert(&self) -> Option<&dyn IdConvert> {
        Some(self.map.as_ref() as &dyn IdConvert)
    }

    fn specialized_reverse(&self) -> Option<Set> {
        Some(Set::from_query(self.clone().reversed()))
    }

    fn specialized_take(&self, take: u64) -> Option<Set> {
        Some(Set::from_query(self.clone().slice_spans(0, take)))
    }

    fn specialized_skip(&self, skip: u64) -> Option<Set> {
        Some(Set::from_query(self.clone().slice_spans(skip, u64::MAX)))
    }

    /// Specialized "flatten_id" implementation.
    fn specialized_flatten_id(&self) -> Option<Cow<'_, IdStaticSet>> {
        Some(Cow::Borrowed(self))
    }
}

#[cfg(test)]
#[allow(clippy::redundant_clone)]
pub(crate) mod tests {
    use std::ops::Deref;
    use std::sync::atomic::Ordering::Acquire;

    use futures::TryStreamExt;
    use nonblocking::non_blocking_result as r;

    use super::super::Set;
    use super::super::tests::*;
    use super::*;
    use crate::Dag;
    use crate::DagAlgorithm;
    use crate::ops::IdMapSnapshot;
    use crate::ops::ToIdSet;
    use crate::set::difference::DifferenceSet;
    use crate::set::intersection::IntersectionSet;
    use crate::set::slice::SliceSet;
    use crate::set::union::UnionSet;
    use crate::tests::build_segments;

    /// Test with a predefined DAG.
    pub(crate) fn with_dag<R, F: Fn(&Dag) -> R>(func: F) -> R {
        let built = build_segments(
            r#"
            A--B--C--D
                \--E--F--G"#,
            "D G",
            2,
        );
        //  0--1--2--3
        //      \--4--5--6
        func(&built.name_dag)
    }

    #[test]
    fn test_dag_invariants() -> Result<()> {
        with_dag(|dag| {
            let bef = r(dag.range("B".into(), "F".into()))?;
            check_invariants(bef.deref())?;
            assert_eq!(nb(bef.size_hint()), (3, Some(3)));

            Ok(())
        })
    }

    #[test]
    fn test_dag_fast_paths() -> Result<()> {
        with_dag(|dag| {
            let abcd = r(dag.ancestors("D".into()))?;
            let abefg = r(dag.ancestors("G".into()))?;

            let ab = abcd.intersection(&abefg);
            check_invariants(ab.deref())?;

            assert!(nb(abcd.contains(&vec![b'A'].into()))?);
            assert!(!nb(abcd.contains(&vec![b'E'].into()))?);

            // should not be "<and <...> <...>>"
            assert_eq!(dbg(&ab), "<spans [A:B+0:1]>");

            let abcdefg = abcd.union(&abefg);
            check_invariants(abcd.deref())?;
            // should not be "<or <...> <...>>"
            assert_eq!(dbg(&abcdefg), "<spans [A:G+0:6]>");

            let cd = abcd.difference(&abefg);
            check_invariants(cd.deref())?;
            // should not be "<difference <...> <...>>"
            assert_eq!(dbg(&cd), "<spans [C:D+2:3]>");

            Ok(())
        })
    }

    #[test]
    fn test_dag_fast_path_set_ops() -> Result<()> {
        with_dag(|dag| {
            let abcd = r(dag.ancestors("D".into()))?.reverse();
            let unordered = abcd.take(2).union_zip(&abcd.skip(3));

            // Intersection and difference can flatten the "unordered" set because rhs order does
            // not matter.
            assert_eq!(
                dbg(abcd.intersection(&unordered)),
                "<spans [D+3, A:B+0:1] +>"
            );
            assert_eq!(dbg(abcd.difference(&unordered)), "<spans [C+2] +>");

            // but lhs order matters (no fast path if lhs order is to be preserved).
            assert_eq!(
                dbg(unordered.intersection(&abcd)),
                "<and <or <spans [A:B+0:1] +> <spans [D+3] +> (order=Zip)> <spans [A:D+0:3] +>>"
            );
            assert_eq!(
                dbg(unordered.difference(&abcd)),
                "<diff <or <spans [A:B+0:1] +> <spans [D+3] +> (order=Zip)> <spans [A:D+0:3] +>>"
            );

            // Union drops order (by flattening) aggressively on both sides.
            assert_eq!(dbg(abcd.union(&unordered)), "<spans [A:D+0:3] +>");

            // Union (preserving order) cannot flatten sets for fast paths.
            assert_eq!(
                dbg(abcd.union_preserving_order(&unordered)),
                "<or <spans [A:D+0:3] +> <or <spans [A:B+0:1] +> <spans [D+3] +> (order=Zip)>>"
            );

            Ok(())
        })
    }

    /// Show set iteration and flatten set iteration for debugging purpose.
    fn dbg_flat(set: &Set) -> String {
        let flat = set.specialized_flatten_id();
        let flat_str = match flat {
            Some(flat) => format!(" flat:{}", fmt_iter(&Set::from_query(flat.into_owned()))),
            None => String::new(),
        };
        format!("{}{}", fmt_iter(set), flat_str)
    }

    // Construct diff, intersection, union sets directly to bypass fast paths.
    fn set_ops(a: &Set, b: &Set) -> (Set, Set, Set) {
        let difference = DifferenceSet::new(a.clone(), b.clone());
        let intersection = IntersectionSet::new(a.clone(), b.clone());
        let union = UnionSet::new(a.clone(), b.clone());
        (
            Set::from_query(difference),
            Set::from_query(intersection),
            Set::from_query(union),
        )
    }

    #[test]
    fn test_dag_specialized_flatten_id_fast_path_with_set_ops() -> Result<()> {
        with_dag(|dag| {
            let mut abcd = "A B C D"
                .split_whitespace()
                .map(|s: &'static str| r(dag.sort(&s.into())).unwrap())
                .collect::<Vec<_>>();
            let d = abcd.pop().unwrap();
            let c = abcd.pop().unwrap();
            let b = abcd.pop().unwrap();
            let a = abcd.pop().unwrap();

            let acb = a.union_preserving_order(&b.union_preserving_order(&c).reverse());
            let bcd = b.union_preserving_order(&c).union_preserving_order(&d);

            // All set operations can use fast paths.
            let diff = acb.difference(&bcd);
            let intersect = acb.intersection(&bcd);
            let union1 = diff.union_preserving_order(&intersect);
            let reversed1 = union1.reverse();
            let union2 = reversed1.union_zip(&diff);
            let reversed2 = union2.reverse();

            // Show the values of the sets.
            assert_eq!(dbg_flat(&diff), "[A] flat:[A]");
            assert_eq!(dbg_flat(&intersect), "[C, B] flat:[C, B]");
            assert_eq!(dbg_flat(&union1), "[A, C, B] flat:[C, B, A]");
            assert_eq!(dbg_flat(&reversed1), "[B, C, A] flat:[A, B, C]");
            assert_eq!(dbg_flat(&union2), "[B, C, A] flat:[A, B, C]");
            assert_eq!(dbg_flat(&reversed2), "[A, C, B] flat:[C, B, A]");

            // The union2 should use a fast path to "count".
            let count1 = union2
                .as_any()
                .downcast_ref::<UnionSet>()
                .unwrap()
                .test_slow_count
                .load(Acquire);
            let _ = r(union2.count())?;
            let count2 = union2
                .as_any()
                .downcast_ref::<UnionSet>()
                .unwrap()
                .test_slow_count
                .load(Acquire);
            assert_eq!(count1, count2, "union.count() should not use slow path");

            // dag.sort(reversed2) should have a fast path.
            let count1 = dag.internal_stats.sort_slow_path_count.load(Acquire);
            let _ = r(dag.sort(&reversed2))?;
            let count2 = dag.internal_stats.sort_slow_path_count.load(Acquire);
            assert_eq!(count1, count2, "dag.sort() should not use slow path");

            // Show the debug format. This shows whether internal structure is flattened or not.
            assert_eq!(
                wrap_dbg_lines(&reversed2),
                r#"
                <reverse
                  <or
                    <reverse
                      <or
                        <diff
                          <or <spans [A+0]> <reverse <or <spans [B+1]> <spans [C+2]>>>>
                          <or <or <spans [B+1]> <spans [C+2]>> <spans [D+3]>>>
                        <and
                          <or <spans [A+0]> <reverse <or <spans [B+1]> <spans [C+2]>>>>
                          <or <or <spans [B+1]> <spans [C+2]>> <spans [D+3]>>>>>
                    <diff
                      <or <spans [A+0]> <reverse <or <spans [B+1]> <spans [C+2]>>>>
                      <or <or <spans [B+1]> <spans [C+2]>> <spans [D+3]>>> (order=Zip)>>"#
            );

            // Flattened turns the tree into a single set.
            let flattened = reversed2.specialized_flatten_id().unwrap();
            assert_eq!(dbg(&flattened), "<spans [A:C+0:2]>");

            Ok(())
        })
    }

    #[test]
    fn test_dag_specialized_flatten_id_fast_path_with_slices() -> Result<()> {
        // SliceSet cannot use fast paths easily. It must check the order.
        with_dag(|dag| {
            let abcd = r(dag.ancestors("D".into()))?.reverse();
            let abefg = r(dag.ancestors("G".into()))?.reverse();

            let slice12 =
                |a: &Set| -> Set { Set::from_query(SliceSet::new(a.clone(), 1, Some(2))) };

            let (d, i, u) = set_ops(&abcd, &abefg);
            assert_eq!(dbg_flat(&d), "[C, D] flat:[C, D]");
            assert_eq!(dbg_flat(&i), "[A, B] flat:[A, B]");
            assert_eq!(
                dbg_flat(&u),
                "[A, B, C, D, E, F, G] flat:[A, B, C, D, E, F, G]"
            );
            assert_eq!(dbg_flat(&slice12(&d)), "[D] flat:[D]");
            assert_eq!(dbg_flat(&slice12(&i)), "[B] flat:[B]");
            assert_eq!(dbg_flat(&slice12(&u)), "[B, C]"); // no fast path for union_preserving_order

            // Make abcd and abefg use different order.
            let (d, i, u) = set_ops(&abcd.reverse(), &abefg);
            assert_eq!(dbg_flat(&d), "[D, C] flat:[D, C]");
            assert_eq!(dbg_flat(&i), "[B, A] flat:[B, A]");
            assert_eq!(
                dbg_flat(&u),
                "[D, C, B, A, E, F, G] flat:[G, F, E, D, C, B, A]"
            );
            assert_eq!(dbg_flat(&slice12(&d)), "[C] flat:[C]");
            assert_eq!(dbg_flat(&slice12(&i)), "[A] flat:[A]");
            assert_eq!(dbg_flat(&slice12(&u)), "[C, B]"); // no fast path for union_preserving_order

            // Set without either order.
            let unordered = abcd.skip(1).take(2).union_zip(&abefg.take(2));
            assert!(
                !unordered
                    .hints()
                    .flags()
                    .intersects(Flags::ID_ASC | Flags::ID_DESC)
            );
            assert_eq!(dbg_flat(&unordered), "[B, A, C] flat:[A, B, C]");

            // S & unordered; or S - unordered can preserve order and maintain fast path.
            assert_eq!(
                dbg_flat(&slice12(&abcd.intersection(&unordered))),
                "[B, C] flat:[B, C]"
            );
            assert_eq!(
                dbg_flat(&slice12(&abefg.difference(&unordered))),
                "[F, G] flat:[F, G]"
            );

            // S + unordered (any order) usually does not have a fast path.
            assert_eq!(
                dbg_flat(&slice12(&abcd.union_preserving_order(&unordered))),
                "[B, C]"
            );
            assert_eq!(dbg_flat(&slice12(&abcd.union_zip(&unordered))), "[B, C]");

            // "union" does not promise order and might have a fast path.
            assert_eq!(
                dbg_flat(&slice12(&abcd.union(&unordered))),
                "[B, C] flat:[B, C]"
            );

            Ok(())
        })
    }

    #[test]
    fn test_dag_no_fast_paths() -> Result<()> {
        let f = |s: Set| -> String { dbg(s) };
        with_dag(|dag1| -> Result<()> {
            with_dag(|dag2| -> Result<()> {
                let abcd = r(dag1.ancestors("D".into()))?;
                let abefg = r(dag2.ancestors("G".into()))?;

                // Since abcd and abefg are from 2 "separate" Dags, fast paths should not
                // be used for intersection, union, and difference.

                let ab = abcd.intersection(&abefg);
                check_invariants(ab.deref())?;
                // should not be "<spans ...>"
                assert_eq!(
                    dbg(&ab),
                    "<and <spans [A:D+0:3]> <spans [E:G+4:6, A:B+0:1]>>"
                );

                let abcdefg = abcd.union(&abefg);
                check_invariants(abcd.deref())?;
                // should not be "<spans ...>"
                assert_eq!(
                    dbg(&abcdefg),
                    "<or <spans [A:D+0:3]> <spans [E:G+4:6, A:B+0:1]>>"
                );

                let cd = abcd.difference(&abefg);
                check_invariants(cd.deref())?;
                // should not be "<spans ...>"
                assert_eq!(
                    dbg(&cd),
                    "<diff <spans [A:D+0:3]> <spans [E:G+4:6, A:B+0:1]>>"
                );

                // Should not use FULL hint fast paths for "&, |, -" operations, because
                // dag1 and dag2 are not considered compatible.
                let a1 = || r(dag1.all()).unwrap();
                let a2 = || r(dag2.all()).unwrap();
                assert_eq!(f(a1() & a2()), "<and <spans [A:G+0:6]> <spans [A:G+0:6]>>");
                assert_eq!(f(a1() | a2()), "<or <spans [A:G+0:6]> <spans [A:G+0:6]>>");
                assert_eq!(f(a1() - a2()), "<diff <spans [A:G+0:6]> <spans [A:G+0:6]>>");

                // No fast path for manually constructed StaticSet either, because
                // the StaticSets do not have DAG associated to test compatibility.
                // However, "all & z" is changed to "z & all" for performance.
                let z = || Set::from("Z");
                assert_eq!(f(z() & a2()), "<and <static [Z]> <spans [A:G+0:6]>>");
                assert_eq!(f(z() | a2()), "<or <static [Z]> <spans [A:G+0:6]>>");
                assert_eq!(f(z() - a2()), "<diff <static [Z]> <spans [A:G+0:6]>>");
                assert_eq!(f(a1() & z()), "<and <static [Z]> <spans [A:G+0:6]>>");
                assert_eq!(f(a1() | z()), "<or <spans [A:G+0:6]> <static [Z]>>");
                assert_eq!(f(a1() - z()), "<diff <spans [A:G+0:6]> <static [Z]>>");

                // EMPTY fast paths can still be used.
                let e = Set::empty;
                assert_eq!(f(e() & a1()), "<empty>");
                assert_eq!(f(e() | a1()), "<spans [A:G+0:6]>");
                assert_eq!(f(e() - a1()), "<empty>");
                assert_eq!(f(a1() & e()), "<empty>");
                assert_eq!(f(a1() | e()), "<spans [A:G+0:6]>");
                assert_eq!(f(a1() - e()), "<spans [A:G+0:6]>");

                // dag.sort() has to use slow path for an incompatible set.
                let count1 = dag1.internal_stats.sort_slow_path_count.load(Acquire);
                let _ = r(dag1.sort(&abefg))?;
                let count2 = dag1.internal_stats.sort_slow_path_count.load(Acquire);
                assert_eq!(
                    count1 + 1,
                    count2,
                    "dag.sort() should use slow path for incompatible set"
                );

                Ok(())
            })
        })
    }

    #[test]
    fn test_to_id_set_no_fast_paths() -> Result<()> {
        let dag1 = build_segments("Z-A", "A", 2).name_dag;
        let dag2 = build_segments("A", "A", 2).name_dag;
        let dag1_a_set = r(dag1.sort(&"A".into()))?;
        let dag2_a_set = r(dag2.sort(&"A".into()))?;
        // For dag1, "A" is assigned Id(1). For dag2, "A" is assigned Id(0).
        let dag1_a_id_set = r(dag1.to_id_set(&dag1_a_set))?;
        let dag2_a_id_set = r(dag2.to_id_set(&dag2_a_set))?;
        assert_eq!(dbg(dag1_a_id_set), "1");
        assert_eq!(dbg(dag2_a_id_set), "0");
        // If we use dag2.to_id_set, it should report "A" in dag2, aka. "0".
        // If we use dag1.to_id_set, it should report "A" in dag1, aka. "1".
        let dag21_a_id_set = r(dag2.to_id_set(&dag1_a_set))?;
        let dag12_a_id_set = r(dag1.to_id_set(&dag2_a_set))?;
        assert_eq!(dbg(dag21_a_id_set), "0");
        assert_eq!(dbg(dag12_a_id_set), "1");
        Ok(())
    }

    #[test]
    fn test_dag_all() -> Result<()> {
        with_dag(|dag| {
            let all = r(dag.all())?;
            assert_eq!(dbg(&all), "<spans [A:G+0:6]>");

            let ac: Set = "A C".into();
            let ac = r(dag.sort(&ac))?;

            let intersection = all.intersection(&ac);
            // should not be "<and ...>"
            assert_eq!(dbg(&intersection), "<spans [C+2, A+0]>");
            Ok(())
        })
    }

    #[test]
    fn test_sort() -> Result<()> {
        with_dag(|dag| -> Result<()> {
            let set = "G C A E".into();
            let sorted = r(dag.sort(&set))?;
            assert_eq!(dbg(&sorted), "<spans [G+6, E+4, C+2] + 1 span>");
            Ok(())
        })
    }

    #[test]
    fn test_reversed() -> Result<()> {
        with_dag(|dag| {
            let desc = r(dag.all())?;
            let asc = desc
                .as_any()
                .downcast_ref::<IdStaticSet>()
                .unwrap()
                .clone()
                .reversed();
            check_invariants(&asc)?;
            assert_eq!(dbg(&asc), "<spans [A:G+0:6] +>");
            assert_eq!(
                dbg(r(r(asc.iter())?.try_collect::<Vec<_>>())?),
                "[A, B, C, D, E, F, G]"
            );

            Ok(())
        })
    }

    #[test]
    fn test_intersect_difference_preserve_reverse_order() -> Result<()> {
        with_dag(|dag| -> Result<()> {
            let abc = "A B C".into();
            let cd = "C D".into();
            let cba = r(dag.sort(&abc))?; // DESC
            let dc = r(dag.sort(&cd))?;
            let abc = cba.reverse();

            let ab = abc.clone() - dc.clone();
            check_invariants(&*ab)?;
            assert_eq!(fmt_iter(&ab), "[A, B]");

            let abc2 = abc.clone() & cba.clone();
            check_invariants(&*abc2)?;
            assert_eq!(fmt_iter(&abc2), "[A, B, C]");

            let cba2 = cba & abc;
            check_invariants(&*cba2)?;
            assert_eq!(fmt_iter(&cba2), "[C, B, A]");
            Ok(())
        })
    }

    #[test]
    fn test_skip_take_reverse() -> Result<()> {
        with_dag(|dag| {
            let set = r(dag.sort(&Set::from("A B C")))?;
            check_skip_take_reverse(set)
        })
    }

    #[test]
    fn test_dag_hints_ancestors() -> Result<()> {
        with_dag(|dag| -> Result<()> {
            let abc = r(dag.ancestors("B C".into()))?;
            let abe = r(dag.common_ancestors("E".into()))?;
            let f: Set = "F".into();
            let all = r(dag.all())?;

            assert!(has_ancestors_flag(abc.clone()));
            assert!(has_ancestors_flag(abe.clone()));
            assert!(has_ancestors_flag(all.clone()));
            assert!(has_ancestors_flag(r(dag.roots(abc.clone()))?));
            assert!(has_ancestors_flag(r(dag.parents(all.clone()))?));

            assert!(!has_ancestors_flag(f.clone()));
            assert!(!has_ancestors_flag(r(dag.roots(f.clone()))?));
            assert!(!has_ancestors_flag(r(dag.parents(f.clone()))?));

            Ok(())
        })
    }

    #[test]
    fn test_dag_hints_ancestors_inheritance() -> Result<()> {
        with_dag(|dag1| -> Result<()> {
            with_dag(|dag2| -> Result<()> {
                let abc = r(dag1.ancestors("B C".into()))?;

                // The ANCESTORS flag is kept by 'sort', 'parents', 'roots' on
                // the same dag.
                assert!(has_ancestors_flag(r(dag1.sort(&abc))?));
                assert!(has_ancestors_flag(r(dag1.parents(abc.clone()))?));
                assert!(has_ancestors_flag(r(dag1.roots(abc.clone()))?));

                // The ANCESTORS flag is removed on a different dag, since the
                // different dag does not assume same graph / ancestry
                // relationship.
                assert!(!has_ancestors_flag(r(dag2.sort(&abc))?));
                assert!(!has_ancestors_flag(r(dag2.parents(abc.clone()))?));
                assert!(!has_ancestors_flag(r(dag2.roots(abc.clone()))?));

                Ok(())
            })
        })
    }

    #[test]
    fn test_dag_hints_ancestors_fast_paths() -> Result<()> {
        with_dag(|dag| -> Result<()> {
            let bfg: Set = "B F G".into();

            // Set the ANCESTORS flag. It's incorrect but make it easier to test fast paths.
            bfg.hints().add_flags(Flags::ANCESTORS);

            // Fast paths are not used if the set is not "bound" to the dag.
            assert_eq!(dbg(r(dag.ancestors(bfg.clone()))?), "<static [B, F, G]>");
            assert_eq!(dbg(r(dag.heads(bfg.clone()))?), "<spans [G+6]>");

            // Binding to the Dag enables fast paths.
            let bfg = r(dag.sort(&bfg))?;
            bfg.hints().add_flags(Flags::ANCESTORS);
            assert_eq!(
                dbg(r(dag.ancestors(bfg.clone()))?),
                "<spans [F:G+5:6, B+1]>"
            );

            // 'heads' has a fast path that uses 'heads_ancestors' to do the calculation.
            // (in this case the result is incorrect because the hints are wrong).
            assert_eq!(dbg(r(dag.heads(bfg.clone()))?), "<spans [G+6]>");

            // 'ancestors' has a fast path that returns set as-is.
            // (in this case the result is incorrect because the hints are wrong).
            assert_eq!(
                dbg(r(dag.ancestors(bfg.clone()))?),
                "<spans [F:G+5:6, B+1]>"
            );

            let g: Set = "G".into();
            let g_ancestors = r(dag.ancestors(g))?;
            assert!(g_ancestors.hints().contains(Flags::ANCESTORS));

            // g_ancestors.take(1) should lose the ANCESTORS hint.
            assert!(!g_ancestors.take(1).hints().contains(Flags::ANCESTORS));
            assert_eq!(
                dbg(r(dag.first_ancestors(g_ancestors.take(1)))?),
                "<spans [E:G+4:6, A:B+0:1]>",
            );

            // g_ancestors.take(99) keeps the ANCESTORS hint because it preserves the entire set.
            assert!(g_ancestors.take(99).hints().contains(Flags::ANCESTORS));

            Ok(())
        })
    }

    #[test]
    fn test_custom_order_hints() -> Result<()> {
        with_dag(|dag| -> Result<()> {
            // Valid Ids are 0 to 6.
            let map = dag.id_map_snapshot()?;
            let dag = dag.dag_snapshot()?;
            let show_hints = move |ids: &[u64]| {
                let list = IdList::from_ids(ids.iter().map(|&id| Id(id)));
                let set = IdStaticSet::from_id_list_idmap_dag(list, map.clone(), dag.clone());
                check_invariants(&set).unwrap();
                let set_reversed = set.clone().reversed();
                check_invariants(&set_reversed).unwrap();
                let hints = set.hints();
                format!(
                    "Order: {:?} Min: {:?} Max: {:?} {:?}",
                    set.iteration_order(),
                    hints.min_id(),
                    hints.max_id(),
                    hints.flags(),
                )
            };

            assert_eq!(
                show_hints(&[]),
                "Order: Some(Desc) Min: None Max: None Flags(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS)"
            );
            assert_eq!(
                show_hints(&[2]),
                "Order: Some(Desc) Min: Some(2) Max: Some(2) Flags(ID_DESC | ID_ASC | TOPO_DESC | HAS_MIN_ID | HAS_MAX_ID)"
            );
            assert_eq!(
                show_hints(&[1, 2]),
                "Order: Some(Asc) Min: Some(1) Max: Some(2) Flags(ID_ASC | HAS_MIN_ID | HAS_MAX_ID)"
            );
            assert_eq!(
                show_hints(&[2, 1]),
                "Order: Some(Desc) Min: Some(1) Max: Some(2) Flags(ID_DESC | TOPO_DESC | HAS_MIN_ID | HAS_MAX_ID)"
            );
            assert_eq!(
                show_hints(&[1, 2, 4, 5]),
                "Order: Some(Asc) Min: Some(1) Max: Some(5) Flags(ID_ASC | HAS_MIN_ID | HAS_MAX_ID)"
            );
            assert_eq!(
                show_hints(&[5, 4, 2, 1]),
                "Order: Some(Desc) Min: Some(1) Max: Some(5) Flags(ID_DESC | TOPO_DESC | HAS_MIN_ID | HAS_MAX_ID)"
            );
            assert_eq!(
                show_hints(&[4, 5, 1, 2]),
                "Order: None Min: Some(1) Max: Some(5) Flags(HAS_MIN_ID | HAS_MAX_ID)"
            );
            assert_eq!(
                show_hints(&[2, 1, 5, 4]),
                "Order: None Min: Some(1) Max: Some(5) Flags(HAS_MIN_ID | HAS_MAX_ID)"
            );
            assert_eq!(
                show_hints(&[4, 5, 1]),
                "Order: None Min: Some(1) Max: Some(5) Flags(HAS_MIN_ID | HAS_MAX_ID)"
            );
            assert_eq!(
                show_hints(&[2, 1, 5]),
                "Order: None Min: Some(1) Max: Some(5) Flags(HAS_MIN_ID | HAS_MAX_ID)"
            );

            Ok(())
        })
    }

    #[test]
    fn test_custom_order_debug_fmt() -> Result<()> {
        with_dag(|dag| -> Result<()> {
            let map = dag.id_map_snapshot()?;
            let dag = dag.dag_snapshot()?;
            let list = move |ids: &[u64]| {
                let list = IdList::from_ids(ids.iter().map(|&id| Id(id)));
                IdStaticSet::from_id_list_idmap_dag(list, map.clone(), dag.clone())
            };

            assert_eq!(dbg(list(&[1, 3, 2])), "<spans [B+1, D:C+3:2] ?>");
            assert_eq!(dbg(list(&[1, 3, 2]).reversed()), "<spans [B+1, D:C+3:2] ¿>");

            Ok(())
        })
    }

    fn has_ancestors_flag(set: Set) -> bool {
        set.hints().contains(Flags::ANCESTORS)
    }

    /// Break <nested <nested <nested ... >>> into multi-lines.
    fn wrap_dbg_lines(value: &dyn fmt::Debug) -> String {
        #[derive(Default, Debug)]
        struct Fmt<'a> {
            head: &'a str,
            tail: &'a str,
            body: Vec<Fmt<'a>>,
            len: usize,
        }

        fn indent(s: &str, prefix: &str) -> String {
            format!(
                "\n{}{}",
                prefix,
                s.trim().replace('\n', &format!("\n{}", prefix))
            )
        }

        impl<'a> Fmt<'a> {
            // to_parse -> (Fmt, rest)
            fn parse(mut s: &'a str) -> (Self, &'a str) {
                let mut out = Self::default();
                let mut seen_left = false;
                let mut i = 0;
                while i < s.len() {
                    let ch = s.as_bytes()[i];
                    match ch {
                        b'<' => {
                            if seen_left {
                                if out.head.is_empty() {
                                    out.head = s[..i].trim();
                                    out.len += out.head.len();
                                }
                                let (nested, rest) = Self::parse(&s[i..]);
                                out.len += nested.len;
                                out.body.push(nested);
                                s = rest;
                                i = 0;
                                continue;
                            } else {
                                seen_left = true;
                                i += 1;
                            }
                        }
                        b'>' => {
                            out.tail = s[..i + 1].trim();
                            out.len += out.tail.len();
                            if out.head.is_empty() {
                                (out.head, out.tail) = out.tail.split_once(' ').unwrap();
                            }
                            let rest = &s[i + 1..];
                            return (out, rest);
                        }
                        _ => i += 1,
                    }
                }
                panic!("unbalanced <> in fmt string");
            }

            fn pretty(&self) -> String {
                let mut out = String::new();
                let need_wrap = !self.body.is_empty() && self.len > 80;
                out.push_str(self.head);
                for f in &self.body {
                    let mut s = f.pretty();
                    if need_wrap {
                        s = indent(&s, "  ");
                    } else {
                        s = format!(" {}", s);
                    }
                    out.push_str(&s);
                }
                if self.tail != ">" {
                    out.push(' ');
                }
                out.push_str(self.tail);
                out
            }
        }

        let s = dbg(value);
        let f = Fmt::parse(&s).0;
        indent(&f.pretty(), "                ")
    }
}
