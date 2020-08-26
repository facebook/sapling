/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # nameset
//!
//! See [`NameSet`] for the main structure.

use crate::ops::DagAlgorithm;
use crate::ops::IdConvert;
use crate::ops::IdMapSnapshot;
use crate::spanset::SpanSet;
use crate::Id;
use crate::Result;
use crate::VertexName;
use std::any::Any;
use std::fmt;
use std::fmt::Debug;
use std::ops::{BitAnd, BitOr, Deref, Sub};
use std::sync::Arc;

pub mod difference;
pub mod hints;
pub mod id_lazy;
pub mod id_static;
pub mod intersection;
pub mod lazy;
pub mod legacy;
pub mod meta;
pub mod r#static;
pub mod union;

use self::hints::Flags;
use self::hints::Hints;
use self::id_static::IdStaticSet;
use self::meta::MetaSet;
use self::r#static::StaticSet;

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

    /// Creates an empty set.
    pub fn empty() -> Self {
        Self::from_query(r#static::StaticSet::empty())
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

    /// Creates from a (lazy) iterator of Ids, an IdMap, and a Dag.
    pub fn from_id_iter_idmap_dag<I>(
        iter: I,
        map: Arc<dyn IdConvert + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
    ) -> NameSet
    where
        I: IntoIterator<Item = Result<Id>> + 'static,
        <I as IntoIterator>::IntoIter: Send + Sync,
    {
        Self::from_query(id_lazy::IdLazySet::from_iter_idmap_dag(iter, map, dag))
    }

    /// Creates from a (lazy) iterator of Ids and a struct with snapshot abilities.
    pub fn from_id_iter_dag<I>(
        iter: I,
        dag: &(impl DagAlgorithm + IdMapSnapshot),
    ) -> Result<NameSet>
    where
        I: IntoIterator<Item = Result<Id>> + 'static,
        <I as IntoIterator>::IntoIter: Send + Sync,
    {
        let map = dag.id_map_snapshot()?;
        let dag = dag.dag_snapshot()?;
        Ok(Self::from_id_iter_idmap_dag(iter, map, dag))
    }

    /// Creates from [`SpanSet`], [`IdMap`] and [`DagAlgorithm`].
    pub fn from_spans_idmap_dag(
        spans: SpanSet,
        map: Arc<dyn IdConvert + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
    ) -> NameSet {
        Self::from_query(IdStaticSet::from_spans_idmap_dag(spans, map, dag))
    }

    /// Creates from [`SpanSet`] and a struct with snapshot abilities.
    pub fn from_spans_dag(
        spans: SpanSet,
        dag: &(impl DagAlgorithm + IdMapSnapshot),
    ) -> Result<Self> {
        let map = dag.id_map_snapshot()?;
        let dag = dag.dag_snapshot()?;
        Ok(Self::from_spans_idmap_dag(spans, map, dag))
    }

    /// Creates from a function that evaluates to a [`NameSet`], and a
    /// `contains` fast path.
    pub fn from_evaluate_contains(
        evaluate: impl Fn() -> Result<NameSet> + Send + Sync + 'static,
        contains: impl Fn(&MetaSet, &VertexName) -> Result<bool> + Send + Sync + 'static,
    ) -> NameSet {
        Self::from_query(MetaSet::from_evaluate(evaluate).with_contains(contains))
    }

    /// Calculates the subset that is only in self, not in other.
    pub fn difference(&self, other: &NameSet) -> NameSet {
        if other.hints().contains(Flags::FULL) && other.hints().is_dag_compatible(self.hints()) {
            return Self::empty();
        }
        if self.hints().contains(Flags::EMPTY) || other.hints().contains(Flags::EMPTY) {
            return self.clone();
        }
        if let (Some(this), Some(other)) = (
            self.as_any().downcast_ref::<IdStaticSet>(),
            other.as_any().downcast_ref::<IdStaticSet>(),
        ) {
            if Arc::ptr_eq(&this.map, &other.map) {
                // Fast path for IdStaticSet
                let result = Self::from_spans_idmap_dag(
                    this.spans.difference(&other.spans),
                    this.map.clone(),
                    this.dag.clone(),
                );
                return result;
            }
        }
        Self::from_query(difference::DifferenceSet::new(self.clone(), other.clone()))
    }

    /// Calculates the intersection of two sets.
    pub fn intersection(&self, other: &NameSet) -> NameSet {
        if self.hints().contains(Flags::FULL) && other.hints().is_dag_compatible(self.hints()) {
            return other.clone();
        }
        if other.hints().contains(Flags::FULL) && other.hints().is_dag_compatible(self.hints()) {
            return self.clone();
        }
        if self.hints().contains(Flags::EMPTY) || other.hints().contains(Flags::EMPTY) {
            return Self::empty();
        }
        if let (Some(this), Some(other)) = (
            self.as_any().downcast_ref::<IdStaticSet>(),
            other.as_any().downcast_ref::<IdStaticSet>(),
        ) {
            if Arc::ptr_eq(&this.map, &other.map) {
                // Fast path for IdStaticSet
                let result = Self::from_spans_idmap_dag(
                    this.spans.intersection(&other.spans),
                    this.map.clone(),
                    this.dag.clone(),
                );
                return result;
            }
        }
        Self::from_query(intersection::IntersectionSet::new(
            self.clone(),
            other.clone(),
        ))
    }

    /// Calculates the union of two sets.
    pub fn union(&self, other: &NameSet) -> NameSet {
        if (self.hints().contains(Flags::FULL) && self.hints().is_dag_compatible(other.hints()))
            || other.hints().contains(Flags::EMPTY)
        {
            return self.clone();
        }
        if self.hints().contains(Flags::EMPTY)
            || (other.hints().contains(Flags::FULL)
                && self.hints().is_dag_compatible(other.hints()))
        {
            return other.clone();
        }
        if let (Some(this), Some(other)) = (
            self.as_any().downcast_ref::<IdStaticSet>(),
            other.as_any().downcast_ref::<IdStaticSet>(),
        ) {
            if Arc::ptr_eq(&this.map, &other.map) {
                // Fast path for IdStaticSet
                let result = Self::from_spans_idmap_dag(
                    this.spans.union(&other.spans),
                    this.map.clone(),
                    this.dag.clone(),
                );
                return result;
            }
        }
        Self::from_query(union::UnionSet::new(self.clone(), other.clone()))
    }

    /// Obtain the attached dag if available.
    pub fn dag(&self) -> Option<Arc<dyn DagAlgorithm + Send + Sync>> {
        self.hints().dag()
    }

    /// Obtain the attached IdMap if available.
    pub fn id_map(&self) -> Option<Arc<dyn IdConvert + Send + Sync>> {
        self.hints().id_map()
    }

    /// Convert the current set into a flat static set so it can be used in some
    /// fast paths. This is useful for some common sets like `obsolete()` that
    /// might be represented by a complex expression.
    pub fn flatten(&self) -> Result<NameSet> {
        match (self.id_map(), self.dag()) {
            (Some(id_map), Some(dag)) => {
                // Convert to IdStaticSet
                self.flatten_id(id_map, dag)
            }
            _ => {
                // Convert to StaticSet
                self.flatten_names()
            }
        }
    }

    /// Convert this set to a static id set.
    pub fn flatten_id(
        &self,
        id_map: Arc<dyn IdConvert + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
    ) -> Result<NameSet> {
        if self.as_any().is::<IdStaticSet>() {
            return Ok(self.clone());
        }
        let mut ids = Vec::with_capacity(self.count()?);
        for vertex in self.iter()? {
            let id = id_map.vertex_id(vertex?)?;
            ids.push(id);
        }
        ids.sort_unstable_by_key(|i| u64::MAX - i.0);
        let spans = SpanSet::from_sorted_spans(ids);
        let flat_set = NameSet::from_spans_idmap_dag(spans, id_map, dag);
        flat_set.hints().inherit_flags_min_max_id(self.hints());
        Ok(flat_set)
    }

    /// Convert this set to a static name set.
    pub fn flatten_names(&self) -> Result<NameSet> {
        if self.as_any().is::<StaticSet>() {
            return Ok(self.clone());
        }
        let names = self.iter()?.collect::<Result<Vec<_>>>()?;
        let flat_set = Self::from_static_names(names);
        flat_set.hints().inherit_flags_min_max_id(self.hints());
        Ok(flat_set)
    }
}

impl BitAnd for NameSet {
    type Output = Self;

    fn bitand(self, other: Self) -> Self {
        self.intersection(&other)
    }
}

impl BitOr for NameSet {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(&other)
    }
}

impl Sub for NameSet {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        self.difference(&other)
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
        let names = self
            .iter()?
            .collect::<std::result::Result<Vec<VertexName>, _>>()?;
        let iter = names.into_iter().rev().map(Ok);
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

    /// For downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Get or set optimization hints.
    fn hints(&self) -> &Hints;

    /// Get an optional IdConvert interface to check hints.
    fn id_convert(&self) -> Option<&dyn IdConvert> {
        None
    }
}

/// Iterator of [`NameSet`].
/// Types implementing this should consider replacing `iter_rev` with a fast
/// path if possible.
pub trait NameIter: Iterator<Item = Result<VertexName>> + Send {}
impl<T> NameIter for T where T: Iterator<Item = Result<VertexName>> + Send {}

impl From<VertexName> for NameSet {
    fn from(name: VertexName) -> NameSet {
        NameSet::from_static_names(std::iter::once(name))
    }
}

impl From<&VertexName> for NameSet {
    fn from(name: &VertexName) -> NameSet {
        NameSet::from_static_names(std::iter::once(name.clone()))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::Id;

    // For easier testing.
    impl From<&str> for NameSet {
        fn from(name: &str) -> NameSet {
            NameSet::from_static_names(
                name.split_whitespace()
                    .map(|n| VertexName::copy_from(n.as_bytes())),
            )
        }
    }

    impl NameSet {
        pub(crate) fn assert_eq(&self, other: NameSet) {
            assert!(
                other.count().unwrap() == self.count().unwrap()
                    && (other.clone() & self.clone()).count().unwrap() == self.count().unwrap(),
                "set {:?} ({:?}) != {:?} ({:?})",
                self,
                self.iter().unwrap().map(|i| i.unwrap()).collect::<Vec<_>>(),
                &other,
                other
                    .iter()
                    .unwrap()
                    .map(|i| i.unwrap())
                    .collect::<Vec<_>>(),
            );
        }
    }

    #[derive(Default, Debug)]
    pub(crate) struct VecQuery(Vec<VertexName>, Hints);

    impl NameSetQuery for VecQuery {
        fn iter(&self) -> Result<Box<dyn NameIter>> {
            let iter = self.0.clone().into_iter().map(Ok);
            Ok(Box::new(iter))
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn hints(&self) -> &Hints {
            &self.1
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
                Hints::default(),
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
        let query = VecQuery::default();
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
                    .intersection(&NameSet::from_static_names(vec![to_name(2), to_name(3)])),
            );
        assert_eq!(
            format!("{:?}", &set),
            "<diff <or <static [0202]> <static [0101]>> <and <static [0303]> <static [0202, 0303]>>>"
        );
        assert_eq!(
            format!("\n{:#?}", &set),
            r#"
<diff
  <or
    <static [
        0202,
    ]>
    <static [
        0101,
    ]>>
  <and
    <static [
        0303,
    ]>
    <static [
        0202,
        0303,
    ]>>>"#
        );
    }

    #[test]
    fn test_flatten() {
        let set = NameSet::from_static_names(vec![to_name(2)])
            .union(&NameSet::from_static_names(vec![to_name(1)]))
            .difference(
                &NameSet::from_static_names(vec![to_name(3)])
                    .intersection(&NameSet::from_static_names(vec![to_name(2), to_name(3)])),
            );
        assert_eq!(
            format!("{:?}", set.flatten().unwrap()),
            "<static [0202, 0101]>"
        );
    }

    #[test]
    fn test_ops() {
        let ab: NameSet = "a b".into();
        let bc: NameSet = "b c".into();
        let s = |set: NameSet| -> Vec<String> { shorten_iter(set.iter()) };
        assert_eq!(s(ab.clone() | bc.clone()), ["61", "62", "63"]);
        assert_eq!(s(ab.clone() & bc.clone()), ["62"]);
        assert_eq!(s(ab.clone() - bc.clone()), ["61"]);
    }

    #[test]
    fn test_hints_empty_full_fast_paths() {
        let partial: NameSet = "a".into();
        partial.hints().add_flags(Flags::ID_ASC);
        let empty: NameSet = "".into();
        let full: NameSet = "f".into();
        full.hints().add_flags(Flags::FULL | Flags::ID_DESC);

        assert_eq!(
            hints_ops(&partial, &empty),
            [
                "- Hints(ID_ASC)",
                "  Hints(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS)",
                "& Hints(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS)",
                "  Hints(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS)",
                "| Hints(ID_ASC)",
                "  Hints(ID_ASC)"
            ]
        );
        assert_eq!(
            hints_ops(&partial, &full),
            [
                "- Hints(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS)",
                "  Hints(ID_DESC)",
                "& Hints(ID_ASC)",
                "  Hints(ID_ASC)",
                "| Hints(FULL | ID_DESC | ANCESTORS)",
                "  Hints(FULL | ID_DESC | ANCESTORS)"
            ]
        );
        assert_eq!(
            hints_ops(&empty, &full),
            [
                "- Hints(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS)",
                "  Hints(FULL | ID_DESC | ANCESTORS)",
                "& Hints(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS)",
                "  Hints(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS)",
                "| Hints(FULL | ID_DESC | ANCESTORS)",
                "  Hints(FULL | ID_DESC | ANCESTORS)"
            ]
        );
    }

    #[test]
    fn test_hints_min_max_id() {
        let bc: NameSet = "b c".into();
        bc.hints().set_min_id(Id(20));
        bc.hints().add_flags(Flags::ID_DESC);

        let ad: NameSet = "a d".into();
        ad.hints().set_max_id(Id(40));
        ad.hints().add_flags(Flags::ID_ASC);

        assert_eq!(
            hints_ops(&bc, &ad),
            [
                "- Hints(ID_DESC, 20..)",
                "  Hints(ID_ASC, ..=40)",
                "& Hints(ID_DESC, 20..=40)",
                "  Hints(ID_ASC, 20..=40)",
                "| Hints((empty))",
                "  Hints((empty))"
            ]
        );

        ad.hints().set_min_id(Id(10));
        bc.hints().set_max_id(Id(30));
        assert_eq!(
            hints_ops(&bc, &ad),
            [
                "- Hints(ID_DESC, 20..=30)",
                "  Hints(ID_ASC, 10..=40)",
                "& Hints(ID_DESC, 20..=30)",
                "  Hints(ID_ASC, 20..=30)",
                "| Hints((empty), 10..=40)",
                "  Hints((empty), 10..=40)"
            ]
        );
    }

    #[test]
    fn test_hints_ancestors() {
        let a: NameSet = "a".into();
        a.hints().add_flags(Flags::ANCESTORS);

        let b: NameSet = "b".into();
        assert_eq!(
            hints_ops(&a, &b),
            [
                "- Hints((empty))",
                "  Hints((empty))",
                "& Hints((empty))",
                "  Hints((empty))",
                "| Hints((empty))",
                "  Hints((empty))"
            ]
        );

        b.hints().add_flags(Flags::ANCESTORS);
        assert_eq!(
            hints_ops(&a, &b),
            [
                "- Hints((empty))",
                "  Hints((empty))",
                "& Hints(ANCESTORS)",
                "  Hints(ANCESTORS)",
                "| Hints(ANCESTORS)",
                "  Hints(ANCESTORS)"
            ]
        );
    }

    // Print hints for &, |, - operations.
    fn hints_ops(lhs: &NameSet, rhs: &NameSet) -> Vec<String> {
        vec![
            (lhs.clone() - rhs.clone(), rhs.clone() - lhs.clone()),
            (lhs.clone() & rhs.clone(), rhs.clone() & lhs.clone()),
            (lhs.clone() | rhs.clone(), rhs.clone() | lhs.clone()),
        ]
        .into_iter()
        .zip("-&|".chars())
        .flat_map(|((set1, set2), ch)| {
            vec![
                format!("{} {:?}", ch, set1.hints()),
                format!("  {:?}", set2.hints()),
            ]
        })
        .collect()
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
