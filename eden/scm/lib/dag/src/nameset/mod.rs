/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # nameset
//!
//! See [`NameSet`] for the main structure.

use crate::idmap::IdMap;
use crate::spanset::SpanSet;
use crate::VertexName;
use anyhow::Result;
use std::any::Any;
use std::fmt;
use std::fmt::Debug;
use std::ops::Deref;
use std::sync::Arc;

pub mod dag;
pub mod difference;
pub mod intersection;
pub mod lazy;
pub mod legacy;
pub mod sorted;
pub mod r#static;
pub mod union;

use self::dag::DagSet;

/// A [`NameSet`] contains an immutable list of names.
///
/// It provides order-preserving iteration and set operations,
/// and is cheaply clonable.
#[derive(Clone)]
pub struct NameSet(Arc<dyn NameSetQuery>);

impl NameSet {
    pub(crate) fn from_query(query: impl NameSetQuery) -> Self {
        Self(Arc::new(query))
    }

    /// Creates from a (short) list of known names.
    pub fn from_static_names(names: impl IntoIterator<Item = VertexName>) -> NameSet {
        Self::from_query(r#static::StaticSet::from_names(names))
    }

    /// Creates from a (lazy) iterator of names.
    pub fn from_iter<I>(iter: I) -> NameSet
    where
        I: IntoIterator<Item = Result<VertexName>> + 'static,
        <I as IntoIterator>::IntoIter: Send + Sync,
    {
        Self::from_query(lazy::LazySet::from_iter(iter))
    }

    /// Creates from [`SpanSet`] and [`IdMap`]. Used by [`NameDag`].
    pub(crate) fn from_spans_idmap(spans: SpanSet, map: Arc<IdMap>) -> NameSet {
        Self::from_query(dag::DagSet::from_spans_idmap(spans, map))
    }

    /// Calculates the subset that is only in self, not in other.
    pub fn difference(&self, other: &NameSet) -> NameSet {
        if let (Some(this), Some(other)) = (
            self.as_any().downcast_ref::<DagSet>(),
            other.as_any().downcast_ref::<DagSet>(),
        ) {
            if Arc::ptr_eq(&this.map, &other.map) {
                // Fast path for DagSet
                return Self::from_spans_idmap(
                    this.spans.difference(&other.spans),
                    this.map.clone(),
                );
            }
        }
        Self::from_query(difference::DifferenceSet::new(self.clone(), other.clone()))
    }

    /// Calculates the intersection of two sets.
    pub fn intersection(&self, other: &NameSet) -> NameSet {
        if self.is_all() {
            return other.clone();
        }
        if let (Some(this), Some(other)) = (
            self.as_any().downcast_ref::<DagSet>(),
            other.as_any().downcast_ref::<DagSet>(),
        ) {
            if Arc::ptr_eq(&this.map, &other.map) {
                // Fast path for DagSet
                return Self::from_spans_idmap(
                    this.spans.intersection(&other.spans),
                    this.map.clone(),
                );
            }
        }
        Self::from_query(intersection::IntersectionSet::new(
            self.clone(),
            other.clone(),
        ))
    }

    /// Calculates the union of two sets.
    pub fn union(&self, other: &NameSet) -> NameSet {
        if let (Some(this), Some(other)) = (
            self.as_any().downcast_ref::<DagSet>(),
            other.as_any().downcast_ref::<DagSet>(),
        ) {
            if Arc::ptr_eq(&this.map, &other.map) {
                // Fast path for DagSet
                return Self::from_spans_idmap(this.spans.union(&other.spans), this.map.clone());
            }
        }
        Self::from_query(union::UnionSet::new(self.clone(), other.clone()))
    }

    /// Mark the set as "topologically sorted".
    /// Useful to mark a [`LazySet`] as sorted to avoid actual sorting
    /// (and keep the set lazy).
    pub fn mark_sorted(&self) -> NameSet {
        if self.is_topo_sorted() {
            self.clone()
        } else {
            Self::from_query(sorted::SortedSet::from_set(self.clone()))
        }
    }
}

impl Deref for NameSet {
    type Target = dyn NameSetQuery;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl fmt::Debug for NameSet {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Read-only queries required by [`NameSet`]: Iteration, length and contains.
///
/// Types implementating this trait should rewrite methods to use fast paths
/// when possible.
pub trait NameSetQuery: Any + Debug + Send + Sync {
    /// Iterate through the set in defined order.
    fn iter(&self) -> Result<Box<dyn NameIter>>;

    /// Iterate through the set in the reversed order.
    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        let names = self.iter()?.collect::<Result<Vec<VertexName>, _>>()?;
        let iter: RevVecNameIter = names.into_iter().rev().map(Ok);
        Ok(Box::new(iter))
    }

    /// Number of names in this set.
    fn count(&self) -> Result<usize> {
        self.iter()?.try_fold(0, |count, result| {
            result?;
            Ok(count + 1)
        })
    }

    /// The first name in the set.
    fn first(&self) -> Result<Option<VertexName>> {
        self.iter()?.nth(0).transpose()
    }

    /// The last name in the set.
    fn last(&self) -> Result<Option<VertexName>> {
        self.iter_rev()?.nth(0).transpose()
    }

    /// Test if this set is empty.
    fn is_empty(&self) -> Result<bool> {
        self.first().map(|n| n.is_none())
    }

    /// Test if this set contains a given name.
    fn contains(&self, name: &VertexName) -> Result<bool> {
        for iter_name in self.iter()? {
            if &iter_name? == name {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Returns true if this set is known topologically sorted (head first, root
    /// last).
    fn is_topo_sorted(&self) -> bool {
        false
    }

    /// Returns true if this set is an "all" set.
    ///
    /// An "all" set will return X when intersection with X.
    /// Otherwise it's not different from a normal set.
    fn is_all(&self) -> bool {
        false
    }

    /// For downcasting.
    fn as_any(&self) -> &dyn Any;
}

/// Iterator of [`NameSet`].
/// Types implementing this should consider replacing `iter_rev` with a fast
/// path if possible.
pub trait NameIter: Iterator<Item = Result<VertexName>> + Send {}

type VecNameIter =
    std::iter::Map<std::vec::IntoIter<VertexName>, fn(VertexName) -> Result<VertexName>>;
type RevVecNameIter = std::iter::Map<
    std::iter::Rev<std::vec::IntoIter<VertexName>>,
    fn(VertexName) -> Result<VertexName>,
>;

impl NameIter for VecNameIter {}
impl NameIter for RevVecNameIter {}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // For easier testing.
    impl From<&str> for NameSet {
        fn from(name: &str) -> NameSet {
            NameSet::from_static_names(
                name.split_whitespace()
                    .map(|n| VertexName::copy_from(n.as_bytes())),
            )
        }
    }

    #[derive(Debug)]
    pub(crate) struct VecQuery(Vec<VertexName>);

    impl NameSetQuery for VecQuery {
        fn iter(&self) -> Result<Box<dyn NameIter>> {
            let iter: VecNameIter = self.0.clone().into_iter().map(Ok);
            Ok(Box::new(iter))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    impl VecQuery {
        /// Quickly crate [`VecQuery`] that contains `len(bytes)` items.
        pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
            let mut used = [false; 255];
            Self(
                bytes
                    .iter()
                    .filter_map(|&b| {
                        if used[b as usize] {
                            None
                        } else {
                            used[b as usize] = true;
                            Some(to_name(b))
                        }
                    })
                    .collect(),
            )
        }
    }

    /// Create a [`VertexName`] from `u8` by repeating them.
    pub(crate) fn to_name(value: u8) -> VertexName {
        VertexName::from(vec![value; 2])
    }

    /// Shorten a [`VertexName`] result.
    pub(crate) fn shorten_name(name: VertexName) -> String {
        name.to_hex()[..2].to_string()
    }

    /// Shorten a [`NameIter`] result.
    pub(crate) fn shorten_iter(iter: Result<Box<dyn NameIter>>) -> Vec<String> {
        iter.unwrap()
            .map(|v| shorten_name(v.unwrap()))
            .collect::<Vec<_>>()
    }

    #[test]
    fn test_empty_query() -> Result<()> {
        let query = VecQuery(Vec::new());
        check_invariants(&query)?;
        assert_eq!(query.iter()?.count(), 0);
        assert_eq!(query.iter_rev()?.count(), 0);
        assert_eq!(query.first()?, None);
        assert_eq!(query.last()?, None);
        assert_eq!(query.count()?, 0);
        assert!(query.is_empty()?);
        assert!(!query.contains(&to_name(0))?);
        Ok(())
    }

    #[test]
    fn test_vec_query() -> Result<()> {
        let query = VecQuery::from_bytes(b"\xab\xef\xcd");
        check_invariants(&query)?;
        assert_eq!(shorten_iter(query.iter()), ["ab", "ef", "cd"]);
        assert_eq!(shorten_iter(query.iter_rev()), ["cd", "ef", "ab"]);
        assert_eq!(shorten_name(query.first()?.unwrap()), "ab");
        assert_eq!(shorten_name(query.last()?.unwrap()), "cd");
        assert!(!query.is_empty()?);
        assert!(query.contains(&to_name(0xef))?);
        assert!(!query.contains(&to_name(0))?);
        Ok(())
    }

    #[test]
    fn test_debug() {
        let set = NameSet::from_static_names(vec![to_name(2)])
            .union(&NameSet::from_static_names(vec![to_name(1)]))
            .difference(
                &NameSet::from_static_names(vec![to_name(3)])
                    .intersection(&NameSet::from_static_names(vec![to_name(2)])),
            );
        assert_eq!(
            format!("{:?}", set),
            "<difference <or <[0202]> <[0101]>> <and <[0303]> <[0202]>>>"
        );
    }

    /// Check consistency of a `NameSetQuery`, such as `iter().nth(0)` matches
    /// `first()` etc.
    pub(crate) fn check_invariants(query: &dyn NameSetQuery) -> Result<()> {
        let is_empty = query.is_empty()?;
        let count = query.count()?;
        let first = query.first()?;
        let last = query.last()?;
        let names: Vec<VertexName> = query.iter()?.collect::<Result<Vec<_>>>()?;
        assert_eq!(
            first,
            names.first().cloned(),
            "first() should match iter().first() (set: {:?})",
            &query
        );
        assert_eq!(
            last,
            names.last().cloned(),
            "last() should match iter().last() (set: {:?})",
            &query
        );
        assert_eq!(
            count,
            names.len(),
            "count() should match iter().count() (set: {:?})",
            &query
        );
        assert_eq!(
            is_empty,
            count == 0,
            "is_empty() should match count() == 0 (set: {:?})",
            &query
        );
        assert!(
            names
                .iter()
                .all(|name| query.contains(name).ok() == Some(true)),
            "contains() should return true for names returned by iter() (set: {:?})",
            &query
        );
        assert!(
            (0..=0xff).all(|b| {
                let name = VertexName::from(vec![b; 20]);
                query.contains(&name).ok() == Some(names.contains(&name))
            }),
            "contains() should return false for names not returned by iter() (set: {:?})",
            &query
        );
        let reversed: Vec<VertexName> = query.iter_rev()?.collect::<Result<Vec<_>>>()?;
        assert_eq!(
            names,
            reversed.into_iter().rev().collect::<Vec<VertexName>>(),
            "iter() should match iter_rev().rev() (set: {:?})",
            &query
        );
        Ok(())
    }
}
