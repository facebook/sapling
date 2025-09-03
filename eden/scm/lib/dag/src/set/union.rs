/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::borrow::Cow;
use std::fmt;
use std::task::Poll;

use futures::Stream;
use futures::StreamExt;
use serde::Deserialize;

use super::AsyncSetQuery;
use super::BoxVertexStream;
use super::Hints;
use super::Set;
use super::hints::Flags;
use super::id_static::IdStaticSet;
use crate::Result;
use crate::Vertex;
use crate::fmt::write_debug;

#[derive(Copy, Clone, Debug, Eq, PartialEq, Deserialize)]
pub enum UnionOrder {
    /// The first set is iterated first using its own order.
    /// Then the second set is iterated, with duplications skipped.
    FirstSecond,

    /// Take one item from the first set, then one item from the second set
    /// (if not exist in the first set), and repeat. Note this is slightly
    /// different from "zip" as the second set is treated as not having
    /// items duplicated with the first set.
    Zip,
}

/// Union of 2 sets.
///
/// See [`UnionOrder`] for iteration order.
pub struct UnionSet {
    sets: [Set; 2],
    hints: Hints,
    order: UnionOrder,
    // Count of the "count_slow" calls.
    #[cfg(test)]
    pub(crate) test_slow_count: std::sync::atomic::AtomicU64,
}

impl UnionSet {
    pub fn new(lhs: Set, rhs: Set) -> Self {
        let hints = Hints::union(&[lhs.hints(), rhs.hints()]);
        if hints.id_map().is_some() {
            if let (Some(id1), Some(id2)) = (lhs.hints().min_id(), rhs.hints().min_id()) {
                hints.set_min_id(id1.min(id2));
            }
            if let (Some(id1), Some(id2)) = (lhs.hints().max_id(), rhs.hints().max_id()) {
                hints.set_max_id(id1.max(id2));
            }
        };
        hints.add_flags(lhs.hints().flags() & rhs.hints().flags() & Flags::ANCESTORS);
        if lhs.hints().contains(Flags::FILTER) || rhs.hints().contains(Flags::FILTER) {
            hints.add_flags(Flags::FILTER);
        }
        Self {
            sets: [lhs, rhs],
            hints,
            order: UnionOrder::FirstSecond,
            #[cfg(test)]
            test_slow_count: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub fn with_order(mut self, order: UnionOrder) -> Self {
        self.order = order;
        self
    }
}

#[async_trait::async_trait]
impl AsyncSetQuery for UnionSet {
    async fn iter(&self) -> Result<BoxVertexStream> {
        debug_assert_eq!(self.sets.len(), 2);
        let diff = self.sets[1].clone() - self.sets[0].clone();
        let diff_iter = diff.iter().await?;
        let set0_iter = self.sets[0].iter().await?;
        let iter: BoxVertexStream = match self.order {
            UnionOrder::FirstSecond => Box::pin(set0_iter.chain(diff_iter)),
            UnionOrder::Zip => Box::pin(ZipStream::new(set0_iter, diff_iter)),
        };
        Ok(iter)
    }

    async fn iter_rev(&self) -> Result<BoxVertexStream> {
        debug_assert_eq!(self.sets.len(), 2);
        let diff = self.sets[1].clone() - self.sets[0].clone();
        let diff_iter = diff.iter_rev().await?;
        let set0_iter = self.sets[0].iter_rev().await?;
        let iter: BoxVertexStream = match self.order {
            UnionOrder::FirstSecond => Box::pin(diff_iter.chain(set0_iter)),
            UnionOrder::Zip => {
                // note: cannot use ZipStream::new(diff_iter, set_iter) when two iters have
                // different lengths.
                let mut iter = self.iter().await?;
                let mut items = Vec::new();
                while let Some(item) = iter.next().await {
                    items.push(item);
                }
                Box::pin(futures::stream::iter(items.into_iter().rev()))
            }
        };
        Ok(iter)
    }

    async fn size_hint(&self) -> (u64, Option<u64>) {
        let mut min_size = 0;
        let mut max_size = Some(0u64);
        for set in &self.sets {
            let (min, max) = set.size_hint().await;
            min_size = min.min(min_size);
            max_size = match (max_size, max) {
                (Some(max_size), Some(max)) => max_size.checked_add(max),
                _ => None,
            };
        }
        (min_size, max_size)
    }

    async fn count_slow(&self) -> Result<u64> {
        #[cfg(test)]
        self.test_slow_count
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        debug_assert_eq!(self.sets.len(), 2);
        // This is more efficient if sets[0] is a large set that has a fast path
        // for "count()".
        let mut count = self.sets[0].count().await?;
        let mut iter = self.sets[1].iter().await?;
        while let Some(item) = iter.next().await {
            let name = item?;
            if !self.sets[0].contains(&name).await? {
                count += 1;
            }
        }
        Ok(count)
    }

    async fn is_empty(&self) -> Result<bool> {
        for set in &self.sets {
            if !set.is_empty().await? {
                return Ok(false);
            }
        }
        Ok(true)
    }

    async fn contains(&self, name: &Vertex) -> Result<bool> {
        for set in &self.sets {
            if set.contains(name).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn contains_fast(&self, name: &Vertex) -> Result<Option<bool>> {
        for set in &self.sets {
            if let Some(result) = set.contains_fast(name).await? {
                return Ok(Some(result));
            }
        }
        Ok(None)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn hints(&self) -> &Hints {
        &self.hints
    }

    fn specialized_flatten_id(&self) -> Option<Cow<'_, IdStaticSet>> {
        let mut result = self.sets[0].specialized_flatten_id()?;
        for set in &self.sets[1..] {
            let other = set.specialized_flatten_id()?;
            result = Cow::Owned(IdStaticSet::from_edit_spans(&result, &other, |a, b| {
                a.union(b)
            })?);
        }
        Some(result)
    }
}

/// Iterate through iter1 and iter2 in turn until both iters end.
/// For example, ZipStream([1,2], [3,4,5,6]) produces: [1,3,2,4,5,6].
struct ZipStream {
    // note: iters[1] should not overlap with iter[0]
    iters: [BoxVertexStream; 2],
    // Whether the stream has ended.
    iter_ended: [bool; 2],
    // Which to pull next, 0 or 1.
    next_iter: usize,
}

impl ZipStream {
    fn new(iter1: BoxVertexStream, iter2: BoxVertexStream) -> Self {
        Self {
            iters: [iter1, iter2],
            iter_ended: [false, false],
            next_iter: 0,
        }
    }
}

impl Stream for ZipStream {
    type Item = Result<Vertex>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        'again: loop {
            let index = self.next_iter;
            if self.iter_ended[index] {
                return Poll::Ready(None);
            }
            match self.iters[index].as_mut().poll_next(cx) {
                Poll::Ready(v) => {
                    if v.is_none() {
                        // Mark the current iterator as ended.
                        self.iter_ended[index] = true;
                    }
                    if !self.iter_ended[index ^ 1] {
                        // Switch to the other iterator if it hasn't ended.
                        self.next_iter = index ^ 1;
                    }
                    if v.is_none() {
                        // Try the other iterator.
                        continue 'again;
                    }
                    return Poll::Ready(v);
                }
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

impl fmt::Debug for UnionSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<or")?;
        write_debug(f, &self.sets[0])?;
        write_debug(f, &self.sets[1])?;
        match self.order {
            UnionOrder::FirstSecond => {}
            order => write!(f, " (order={:?})", order)?,
        }
        write!(f, ">")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::super::tests::*;
    use super::*;

    fn union(a: &[u8], b: &[u8]) -> UnionSet {
        let a = Set::from_query(VecQuery::from_bytes(a));
        let b = Set::from_query(VecQuery::from_bytes(b));
        UnionSet::new(a, b)
    }

    #[test]
    fn test_union_basic() -> Result<()> {
        // 'a' overlaps with 'b'. UnionSet should de-duplicate items.
        let set = union(b"\x11\x33\x22", b"\x44\x11\x55\x33");
        check_invariants(&set)?;
        assert_eq!(shorten_iter(ni(set.iter())), ["11", "33", "22", "44", "55"]);
        assert_eq!(
            shorten_iter(ni(set.iter_rev())),
            ["55", "44", "22", "33", "11"]
        );
        assert!(!nb(set.is_empty())?);
        assert_eq!(nb(set.count())?, 5);
        assert_eq!(shorten_name(nb(set.first())?.unwrap()), "11");
        assert_eq!(shorten_name(nb(set.last())?.unwrap()), "55");
        for &b in b"\x11\x22\x33\x44\x55".iter() {
            assert!(nb(set.contains(&to_name(b)))?);
        }
        for &b in b"\x66\x77\x88".iter() {
            assert!(!nb(set.contains(&to_name(b)))?);
        }
        Ok(())
    }

    #[test]
    fn test_union_zip_order() -> Result<()> {
        let set = union(b"\x33\x44\x55", b"").with_order(UnionOrder::Zip);
        check_invariants(&set)?;
        assert_eq!(shorten_iter(ni(set.iter())), ["33", "44", "55"]);

        let set = union(b"", b"\x33\x44\x55").with_order(UnionOrder::Zip);
        check_invariants(&set)?;
        assert_eq!(shorten_iter(ni(set.iter())), ["33", "44", "55"]);

        let set = union(b"\x33\x44\x55", b"\x55\x33\x22\x11").with_order(UnionOrder::Zip);
        assert_eq!(shorten_iter(ni(set.iter())), ["33", "22", "44", "11", "55"]);
        check_invariants(&set)?;

        Ok(())
    }

    #[test]
    fn test_size_hint_sets() {
        check_size_hint_sets(UnionSet::new);
        check_size_hint_sets(|a, b| UnionSet::new(a, b).with_order(UnionOrder::Zip));
    }

    quickcheck::quickcheck! {
        fn test_union_quickcheck(a: Vec<u8>, b: Vec<u8>) -> bool {
            let set = union(&a, &b);
            check_invariants(&set).unwrap();

            let count = nb(set.count()).unwrap() as usize;
            assert!(count <= a.len() + b.len());

            let set2: HashSet<_> = a.iter().chain(b.iter()).cloned().collect();
            assert_eq!(count, set2.len());

            assert!(a.iter().all(|&b| nb(set.contains(&to_name(b))).ok() == Some(true)));
            assert!(b.iter().all(|&b| nb(set.contains(&to_name(b))).ok() == Some(true)));

            true
        }
    }
}
