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

/// Subset of `lhs` that does not overlap with `rhs`.
///
/// The iteration order is defined by `lhs`.
pub struct DifferenceSet {
    lhs: NameSet,
    rhs: NameSet,
    hints: Hints,
}

struct Iter {
    iter: BoxVertexStream,
    rhs: NameSet,
}

impl DifferenceSet {
    pub fn new(lhs: NameSet, rhs: NameSet) -> Self {
        let hints = Hints::new_inherit_idmap_dag(lhs.hints());
        // Inherit flags, min/max Ids from lhs.
        hints.add_flags(
            lhs.hints().flags()
                & (Flags::EMPTY
                    | Flags::ID_DESC
                    | Flags::ID_ASC
                    | Flags::TOPO_DESC
                    | Flags::FILTER),
        );
        if let Some(id) = lhs.hints().min_id() {
            hints.set_min_id(id);
        }
        if let Some(id) = lhs.hints().max_id() {
            hints.set_max_id(id);
        }
        Self { lhs, rhs, hints }
    }
}

#[async_trait::async_trait]
impl AsyncNameSetQuery for DifferenceSet {
    async fn iter(&self) -> Result<BoxVertexStream> {
        let iter = Iter {
            iter: self.lhs.iter().await?,
            rhs: self.rhs.clone(),
        };
        Ok(iter.into_stream())
    }

    async fn iter_rev(&self) -> Result<BoxVertexStream> {
        let iter = Iter {
            iter: self.lhs.iter_rev().await?,
            rhs: self.rhs.clone(),
        };
        Ok(iter.into_stream())
    }

    async fn contains(&self, name: &VertexName) -> Result<bool> {
        Ok(self.lhs.contains(name).await? && !self.rhs.contains(name).await?)
    }

    async fn contains_fast(&self, name: &VertexName) -> Result<Option<bool>> {
        let lhs_contains = self.lhs.contains_fast(name).await?;
        if lhs_contains == Some(false) {
            return Ok(Some(false));
        }
        let rhs_contains = self.rhs.contains_fast(name).await?;
        let result = match (lhs_contains, rhs_contains) {
            (Some(true), Some(false)) => Some(true),
            (_, Some(true)) | (Some(false), _) => Some(false),
            (Some(true), None) | (None, _) => None,
        };
        Ok(result)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn hints(&self) -> &Hints {
        &self.hints
    }
}

impl fmt::Debug for DifferenceSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "<diff")?;
        write_debug(f, &self.lhs)?;
        write_debug(f, &self.rhs)?;
        write!(f, ">")
    }
}

impl Iter {
    async fn next(&mut self) -> Option<Result<VertexName>> {
        loop {
            let result = self.iter.as_mut().next().await;
            if let Some(Ok(ref name)) = result {
                match self.rhs.contains(&name).await {
                    Err(err) => break Some(Err(err)),
                    Ok(true) => continue,
                    _ => {}
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

#[cfg(test)]
mod tests {
    use nonblocking::non_blocking as nb;

    use super::super::tests::*;
    use super::*;

    fn difference(a: &[u8], b: &[u8]) -> DifferenceSet {
        let a = NameSet::from_query(VecQuery::from_bytes(a));
        let b = NameSet::from_query(VecQuery::from_bytes(b));
        DifferenceSet::new(a, b)
    }

    #[test]
    fn test_difference_basic() -> Result<()> {
        let set = difference(b"\x11\x33\x55\x22\x44", b"\x44\x33\x66");
        check_invariants(&set)?;
        assert_eq!(shorten_iter(ni(set.iter())), ["11", "55", "22"]);
        assert_eq!(shorten_iter(ni(set.iter_rev())), ["22", "55", "11"]);
        assert!(!nb(set.is_empty())??);
        assert_eq!(nb(set.count())??, 3);
        assert_eq!(shorten_name(nb(set.first())??.unwrap()), "11");
        assert_eq!(shorten_name(nb(set.last())??.unwrap()), "22");
        for &b in b"\x11\x22\x55".iter() {
            assert!(nb(set.contains(&to_name(b)))??);
        }
        for &b in b"\x33\x44\x66".iter() {
            assert!(!nb(set.contains(&to_name(b)))??);
        }
        Ok(())
    }

    quickcheck::quickcheck! {
        fn test_difference_quickcheck(a: Vec<u8>, b: Vec<u8>) -> bool {
            let set = difference(&a, &b);
            check_invariants(&set).unwrap();

            let count = nb(set.count()).unwrap().unwrap();
            assert!(count <= a.len());

            assert!(b.iter().all(|&b| nb(set.contains(&to_name(b))).unwrap().ok() == Some(false)));

            true
        }
    }
}
