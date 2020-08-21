/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::Hints;
use super::NameIter;
use super::NameSet;
use super::NameSetQuery;
use crate::Result;
use crate::VertexName;
use once_cell::sync::OnceCell;
use std::any::Any;
use std::fmt;

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
    evaluate: Box<dyn Fn() -> Result<NameSet> + Send + Sync>,
    evaluated: OnceCell<NameSet>,

    /// Optional "contains" fast path.
    contains: Option<Box<dyn Fn(&MetaSet, &VertexName) -> Result<bool> + Send + Sync>>,

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
    pub fn from_evaluate(evaluate: impl Fn() -> Result<NameSet> + Send + Sync + 'static) -> Self {
        Self {
            evaluate: Box::new(evaluate),
            evaluated: Default::default(),
            contains: None,
            hints: Hints::default(),
        }
    }

    /// Provides a fast path for `contains`. Be careful to make sure "contains"
    /// matches "evaluate".
    pub fn with_contains(
        mut self,
        contains: impl Fn(&MetaSet, &VertexName) -> Result<bool> + Send + Sync + 'static,
    ) -> Self {
        self.contains = Some(Box::new(contains));
        self
    }

    /// Evaluate the set. Returns a new set.
    pub fn evaluate(&self) -> Result<NameSet> {
        self.evaluated
            .get_or_try_init(&self.evaluate)
            .map(|s| s.clone())
    }

    /// Returns the evaluated set if it was evaluated.
    /// Returns None if the set was not evaluated.
    pub fn evaluated(&self) -> Option<NameSet> {
        self.evaluated.get().cloned()
    }
}

impl NameSetQuery for MetaSet {
    fn iter(&self) -> Result<Box<dyn NameIter>> {
        self.evaluate()?.iter()
    }

    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        self.evaluate()?.iter_rev()
    }

    fn count(&self) -> Result<usize> {
        self.evaluate()?.count()
    }

    fn last(&self) -> Result<Option<VertexName>> {
        self.evaluate()?.last()
    }

    fn contains(&self, name: &VertexName) -> Result<bool> {
        match self.evaluated() {
            Some(set) => set.contains(name),
            None => match &self.contains {
                Some(f) => f(self, name),
                None => self.evaluate()?.contains(name),
            },
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
    use super::super::tests::*;
    use super::*;

    fn meta_set(v: &[impl ToString]) -> MetaSet {
        let v: Vec<_> = v.iter().map(|s| s.to_string()).collect();
        let f = move || {
            Ok(NameSet::from_static_names(
                v.clone().into_iter().map(Into::into),
            ))
        };
        MetaSet::from_evaluate(f)
    }

    #[test]
    fn test_meta_basic() -> Result<()> {
        let set = meta_set(&["1", "3", "2", "7", "5"]);

        assert!(set.evaluated().is_none());
        assert!(set.contains(&"2".into())?);
        // The set is evaluated after a "contains" check without a "contains" fast path.
        assert!(set.evaluated().is_some());

        check_invariants(&set)?;
        Ok(())
    }

    #[test]
    fn test_meta_contains() -> Result<()> {
        let set = meta_set(&["1", "3", "2", "7", "5"])
            .with_contains(|_, v| Ok(v.as_ref().len() == 1 && b"12357".contains(&v.as_ref()[0])));

        assert!(set.contains(&"2".into())?);
        assert!(!set.contains(&"6".into())?);
        // The set is not evaluated - contains fast path takes care of the checks.
        assert!(set.evaluated().is_none());

        check_invariants(&set)?;
        Ok(())
    }

    #[test]
    fn test_debug() {
        let set = meta_set(&["1", "3", "2", "7", "5"]);
        assert_eq!(format!("{:?}", &set), "<meta ?>");
        set.evaluate().unwrap();
        assert_eq!(format!("{:5?}", &set), "<meta <static [1, 3, 2, 7, 5]>>");
    }

    quickcheck::quickcheck! {
        fn test_meta_quickcheck(v: Vec<String>) -> bool {
            let set = meta_set(&v);
            check_invariants(&set).unwrap();
            true
        }
    }
}
