/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::borrow::Cow;
use std::fmt;

use super::AsyncSetQuery;
use super::BoxVertexStream;
use super::Hints;
use super::Set;
use super::hints::Flags;
use super::id_static::IdStaticSet;
use crate::Result;
use crate::Vertex;

/// Set with a reversed iteration order.
#[derive(Clone)]
pub struct ReverseSet {
    inner: Set,
    hints: Hints,
}

impl ReverseSet {
    pub fn new(set: Set) -> Self {
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
impl AsyncSetQuery for ReverseSet {
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

    async fn contains(&self, name: &Vertex) -> Result<bool> {
        self.inner.contains(name).await
    }

    async fn contains_fast(&self, name: &Vertex) -> Result<Option<bool>> {
        self.inner.contains_fast(name).await
    }

    async fn first(&self) -> Result<Option<Vertex>> {
        self.inner.last().await
    }

    async fn last(&self) -> Result<Option<Vertex>> {
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

    fn specialized_reverse(&self) -> Option<Set> {
        Some(self.inner.clone())
    }

    fn specialized_flatten_id(&self) -> Option<Cow<'_, IdStaticSet>> {
        let inner = self.inner.specialized_flatten_id()?;
        let result = inner.into_owned().reversed();
        Some(Cow::Owned(result))
    }
}

impl fmt::Debug for ReverseSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("<reverse ")?;
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
        let orig = Set::from("a b c d");
        let set = ReverseSet::new(orig);
        check_invariants(&set)?;

        let iter = nb(set.iter())?;
        assert_eq!(dbg(nb(iter.try_collect::<Vec<_>>())?), "[d, c, b, a]");

        Ok(())
    }
}
