/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::fmt;

use futures::future::BoxFuture;
use parking_lot::RwLock;

use super::AsyncNameSetQuery;
use super::BoxVertexStream;
use super::Hints;
use super::NameSet;
use crate::Result;
use crate::VertexName;

/// A set that is lazily evaluated to another set on access, with an
/// optional fast path for "contains".
///
/// This can be useful for various cases. Especially when "contains" and "iter"
/// have separate fast paths. For example, `merge()` (merge commits) obviously
/// has a cheap "contains" fast path by checking if a given commit has
/// multiple parents. However, if we want to iterating through the set,
/// segmented changelog has a fast path by iterating flat segments, instead
/// of testing commits one by one using the "contains" check.
///
/// `MetaSet` is different from `LazySet`: `MetaSet`'s `evaluate` function can
/// return static or lazy or meta sets. `LazySet` does not support a "contains"
/// fast path (yet).
///
/// `MetaSet` is different from a pure filtering set (ex. only has "contains"
/// fast path), as `MetaSet` supports fast path for iteration.
pub struct MetaSet {
    evaluate: Box<dyn Fn() -> BoxFuture<'static, Result<NameSet>> + Send + Sync>,
    evaluated: RwLock<Option<NameSet>>,

    /// Optional "contains" fast path.
    contains: Option<
        Box<
            dyn for<'a> Fn(&'a MetaSet, &'a VertexName) -> BoxFuture<'a, Result<bool>>
                + Send
                + Sync,
        >,
    >,

    hints: Hints,
}

impl fmt::Debug for MetaSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("<meta ")?;
        if let Some(set) = self.evaluated() {
            set.fmt(f)?;
        } else {
            f.write_str("?")?;
        }
        f.write_str(">")?;
        Ok(())
    }
}

impl MetaSet {
    /// Constructs `MetaSet` from an `evaluate` function that returns a
    /// `NameSet`. The `evaluate` function is not called immediately.
    pub fn from_evaluate_hints(
        evaluate: Box<dyn Fn() -> BoxFuture<'static, Result<NameSet>> + Send + Sync + 'static>,
        hints: Hints,
    ) -> Self {
        Self {
            evaluate,
            evaluated: Default::default(),
            contains: None,
            hints,
        }
    }

    /// Provides a fast path for `contains`. Be careful to make sure "contains"
    /// matches "evaluate".
    pub fn with_contains(
        mut self,
        contains: Box<
            dyn for<'a> Fn(&'a MetaSet, &'a VertexName) -> BoxFuture<'a, Result<bool>>
                + Send
                + Sync,
        >,
    ) -> Self {
        self.contains = Some(contains);
        self
    }

    /// Evaluate the set. Returns a new set.
    pub async fn evaluate(&self) -> Result<NameSet> {
        if let Some(s) = &*self.evaluated.read() {
            return Ok(s.clone());
        }
        let s = (self.evaluate)().await?;
        *self.evaluated.write() = Some(s.clone());
        Ok(s)
    }

    /// Returns the evaluated set if it was evaluated.
    /// Returns None if the set was not evaluated.
    pub fn evaluated(&self) -> Option<NameSet> {
        self.evaluated.read().clone()
    }
}

#[async_trait::async_trait]
impl AsyncNameSetQuery for MetaSet {
    async fn iter(&self) -> Result<BoxVertexStream> {
        self.evaluate().await?.iter().await
    }

    async fn iter_rev(&self) -> Result<BoxVertexStream> {
        self.evaluate().await?.iter_rev().await
    }

    async fn count(&self) -> Result<usize> {
        self.evaluate().await?.count().await
    }

    async fn last(&self) -> Result<Option<VertexName>> {
        self.evaluate().await?.last().await
    }

    async fn contains(&self, name: &VertexName) -> Result<bool> {
        match self.evaluated() {
            Some(set) => set.contains(name).await,
            None => match &self.contains {
                Some(f) => f(self, name).await,
                None => self.evaluate().await?.contains(name).await,
            },
        }
    }

    async fn contains_fast(&self, name: &VertexName) -> Result<Option<bool>> {
        match &self.contains {
            Some(f) => Ok(Some(f(self, name).await?)),
            None => Ok(None),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn hints(&self) -> &Hints {
        &self.hints
    }
}

#[cfg(test)]
mod tests {
    use nonblocking::non_blocking_result as r;

    use super::super::tests::*;
    use super::*;

    fn meta_set(v: &[impl ToString]) -> MetaSet {
        let v: Vec<_> = v.iter().map(|s| s.to_string()).collect();
        let f = move || -> BoxFuture<_> {
            let s = NameSet::from_static_names(v.clone().into_iter().map(Into::into));
            Box::pin(async move { Ok(s) })
        };
        MetaSet::from_evaluate_hints(Box::new(f), Hints::default())
    }

    #[test]
    fn test_meta_basic() -> Result<()> {
        let set = meta_set(&["1", "3", "2", "7", "5"]);

        assert!(set.evaluated().is_none());
        assert!(nb(set.contains(&"2".into()))?);
        // The set is evaluated after a "contains" check without a "contains" fast path.
        assert!(set.evaluated().is_some());

        check_invariants(&set)?;
        Ok(())
    }

    #[test]
    fn test_meta_contains() -> Result<()> {
        let set = meta_set(&["1", "3", "2", "7", "5"]).with_contains(Box::new(|_, v| {
            let r = Ok(v.as_ref().len() == 1 && b"12357".contains(&v.as_ref()[0]));
            Box::pin(async move { r })
        }));

        assert!(nb(set.contains(&"2".into()))?);
        assert!(!nb(set.contains(&"6".into()))?);
        // The set is not evaluated - contains fast path takes care of the checks.
        assert!(set.evaluated().is_none());

        check_invariants(&set)?;
        Ok(())
    }

    #[test]
    fn test_debug() {
        let set = meta_set(&["1", "3", "2", "7", "5"]);
        assert_eq!(format!("{:?}", &set), "<meta ?>");
        r(set.evaluate()).unwrap();
        assert_eq!(
            format!("{:5?}", &set),
            "<meta <static [31, 33, 32, 37, 35]>>"
        );
    }

    quickcheck::quickcheck! {
        fn test_meta_quickcheck(v: Vec<String>) -> bool {
            let set = meta_set(&v);
            check_invariants(&set).unwrap();
            true
        }
    }
}
