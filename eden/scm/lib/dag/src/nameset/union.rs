/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::fmt;

use futures::StreamExt;

use super::hints::Flags;
use super::AsyncNameSetQuery;
use super::BoxVertexStream;
use super::Hints;
use super::NameSet;
use crate::fmt::write_debug;
use crate::Result;
use crate::VertexName;

/// Union of 2 sets.
///
/// The order is preserved. The first set is iterated first, then the second set
/// is iterated, with duplicated names skipped.
pub struct UnionSet {
    sets: [NameSet; 2],
    hints: Hints,
}

impl UnionSet {
    pub fn new(lhs: NameSet, rhs: NameSet) -> Self {
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
        }
    }
}

#[async_trait::async_trait]
impl AsyncNameSetQuery for UnionSet {
    async fn iter(&self) -> Result<BoxVertexStream> {
        debug_assert_eq!(self.sets.len(), 2);
        let diff = self.sets[1].clone() - self.sets[0].clone();
        let diff_iter = diff.iter().await?;
        let set0_iter = self.sets[0].iter().await?;
        let iter = set0_iter.chain(diff_iter);
        Ok(Box::pin(iter))
    }

    async fn iter_rev(&self) -> Result<BoxVertexStream> {
        debug_assert_eq!(self.sets.len(), 2);
        let diff = self.sets[1].clone() - self.sets[0].clone();
        let diff_iter = diff.iter_rev().await?;
        let set0_iter = self.sets[0].iter_rev().await?;
        let iter = diff_iter.chain(set0_iter);
        Ok(Box::pin(iter))
    }

    async fn count(&self) -> Result<usize> {
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

    async fn contains(&self, name: &VertexName) -> Result<bool> {
        for set in &self.sets {
            if set.contains(name).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    async fn contains_fast(&self, name: &VertexName) -> Result<Option<bool>> {
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
}

impl fmt::Debug for UnionSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<or")?;
        write_debug(f, &self.sets[0])?;
        write_debug(f, &self.sets[1])?;
        write!(f, ">")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::super::tests::*;
    use super::*;

    fn union(a: &[u8], b: &[u8]) -> UnionSet {
        let a = NameSet::from_query(VecQuery::from_bytes(a));
        let b = NameSet::from_query(VecQuery::from_bytes(b));
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

    quickcheck::quickcheck! {
        fn test_union_quickcheck(a: Vec<u8>, b: Vec<u8>) -> bool {
            let set = union(&a, &b);
            check_invariants(&set).unwrap();

            let count = nb(set.count()).unwrap();
            assert!(count <= a.len() + b.len());

            let set2: HashSet<_> = a.iter().chain(b.iter()).cloned().collect();
            assert_eq!(count, set2.len());

            assert!(a.iter().all(|&b| nb(set.contains(&to_name(b))).ok() == Some(true)));
            assert!(b.iter().all(|&b| nb(set.contains(&to_name(b))).ok() == Some(true)));

            true
        }
    }
}
