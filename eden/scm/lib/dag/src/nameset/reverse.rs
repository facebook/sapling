/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::fmt;

use super::hints::Flags;
use super::AsyncNameSetQuery;
use super::BoxVertexStream;
use super::Hints;
use super::NameSet;
use crate::Result;
use crate::VertexName;

/// Set with a reversed iteration order.
#[derive(Clone)]
pub struct ReverseSet {
    inner: NameSet,
    hints: Hints,
}

impl ReverseSet {
    pub fn new(set: NameSet) -> Self {
        let hints = set.hints().clone();
        hints.update_flags_with(|flags| {
            let mut new_flags = flags - (Flags::TOPO_DESC | Flags::ID_DESC | Flags::ID_ASC);
            if flags.contains(Flags::ID_DESC) {
                new_flags |= Flags::ID_ASC;
            }
            if flags.contains(Flags::ID_ASC) {
                new_flags |= Flags::ID_DESC;
            }
            new_flags
        });
        Self { inner: set, hints }
    }
}

#[async_trait::async_trait]
impl AsyncNameSetQuery for ReverseSet {
    async fn iter(&self) -> Result<BoxVertexStream> {
        self.inner.iter_rev().await
    }

    async fn iter_rev(&self) -> Result<BoxVertexStream> {
        self.inner.iter().await
    }

    async fn count(&self) -> Result<u64> {
        self.inner.count().await
    }

    async fn size_hint(&self) -> (u64, Option<u64>) {
        self.inner.size_hint().await
    }

    async fn contains(&self, name: &VertexName) -> Result<bool> {
        self.inner.contains(name).await
    }

    async fn contains_fast(&self, name: &VertexName) -> Result<Option<bool>> {
        self.inner.contains_fast(name).await
    }

    async fn first(&self) -> Result<Option<VertexName>> {
        self.inner.last().await
    }

    async fn last(&self) -> Result<Option<VertexName>> {
        self.inner.first().await
    }

    async fn is_empty(&self) -> Result<bool> {
        self.inner.is_empty().await
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn hints(&self) -> &Hints {
        &self.hints
    }

    fn id_convert(&self) -> Option<&dyn crate::ops::IdConvert> {
        self.inner.id_convert()
    }

    fn specialized_reverse(&self) -> Option<NameSet> {
        Some(self.inner.clone())
    }
}

impl fmt::Debug for ReverseSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("<reverse")?;
        self.inner.fmt(f)?;
        f.write_str(">")
    }
}

#[cfg(test)]
#[allow(clippy::redundant_clone)]
mod tests {
    use futures::TryStreamExt;

    use super::super::tests::*;
    use super::*;

    #[test]
    fn test_basic() -> Result<()> {
        let orig = NameSet::from("a b c d");
        let set = ReverseSet::new(orig);
        check_invariants(&set)?;

        let iter = nb(set.iter())?;
        assert_eq!(
            format!("{:?}", nb(iter.try_collect::<Vec<_>>())?),
            "[d, c, b, a]"
        );

        Ok(())
    }
}
