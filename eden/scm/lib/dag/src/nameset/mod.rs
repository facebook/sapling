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
use futures::future::BoxFuture;
use futures::Stream;
use futures::StreamExt;
use nonblocking::non_blocking;
use std::any::Any;
use std::fmt;
use std::fmt::Debug;
use std::ops::{BitAnd, BitOr, Deref, Sub};
use std::pin::Pin;
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
pub struct NameSet(Arc<dyn AsyncNameSetQuery>);

impl NameSet {
    pub(crate) fn from_query(query: impl AsyncNameSetQuery) -> Self {
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
    pub fn from_evaluate_contains<C>(
        evaluate: impl Fn() -> Result<NameSet> + Send + Sync + 'static,
        contains: C,
    ) -> NameSet
    where
        C: for<'a> Fn(&'a MetaSet, &'a VertexName) -> Result<bool>,
        C: Send + Sync + 'static,
    {
        let evaluate = move || -> BoxFuture<_> {
            let result = evaluate();
            Box::pin(async move { result })
        };
        let contains = Arc::new(contains);
        Self::from_async_evaluate_contains(
            Box::new(evaluate),
            Box::new(move |m, v| {
                let contains = contains.clone();
                Box::pin(async move { contains(m, v) })
            }),
        )
    }

    /// Creates from an async function that evaluates to a [`NameSet`], and a
    /// async `contains` fast path.
    pub fn from_async_evaluate_contains(
        evaluate: Box<dyn Fn() -> BoxFuture<'static, Result<NameSet>> + Send + Sync>,
        contains: Box<
            dyn for<'a> Fn(&'a MetaSet, &'a VertexName) -> BoxFuture<'a, Result<bool>>
                + Send
                + Sync,
        >,
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
            if this.map.map_id() == other.map.map_id() {
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
            if this.map.map_id() == other.map.map_id() {
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
            if this.map.map_id() == other.map.map_id() {
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
    pub async fn flatten(&self) -> Result<NameSet> {
        match (self.id_map(), self.dag()) {
            (Some(id_map), Some(dag)) => {
                // Convert to IdStaticSet
                self.flatten_id(id_map, dag).await
            }
            _ => {
                // Convert to StaticSet
                self.flatten_names().await
            }
        }
    }

    /// Convert this set to a static id set.
    pub async fn flatten_id(
        &self,
        id_map: Arc<dyn IdConvert + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
    ) -> Result<NameSet> {
        if self.as_any().is::<IdStaticSet>() {
            return Ok(self.clone());
        }
        let mut ids = Vec::with_capacity(self.count()?);
        for vertex in self.iter()? {
            let id = id_map.vertex_id(vertex?).await?;
            ids.push(id);
        }
        ids.sort_unstable_by_key(|i| u64::MAX - i.0);
        let spans = SpanSet::from_sorted_spans(ids);
        let flat_set = NameSet::from_spans_idmap_dag(spans, id_map, dag);
        flat_set.hints().inherit_flags_min_max_id(self.hints());
        Ok(flat_set)
    }

    /// Convert this set to a static name set.
    pub async fn flatten_names(&self) -> Result<NameSet> {
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
    type Target = dyn AsyncNameSetQuery;

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
#[async_trait::async_trait]
pub trait AsyncNameSetQuery: Any + Debug + Send + Sync {
    /// Iterate through the set in defined order.
    async fn iter(&self) -> Result<BoxVertexStream>;

    /// Iterate through the set in the reversed order.
    async fn iter_rev(&self) -> Result<BoxVertexStream> {
        let mut iter = self.iter().await?;
        let mut items = Vec::new();
        while let Some(item) = iter.next().await {
            items.push(item);
        }
        Ok(Box::pin(futures::stream::iter(items.into_iter().rev())))
    }

    /// Number of names in this set.
    async fn count(&self) -> Result<usize> {
        let mut iter = self.iter().await?;
        let mut count = 0;
        while let Some(item) = iter.next().await {
            item?;
            count += 1;
        }
        Ok(count)
    }

    /// The first name in the set.
    async fn first(&self) -> Result<Option<VertexName>> {
        self.iter().await?.next().await.transpose()
    }

    /// The last name in the set.
    async fn last(&self) -> Result<Option<VertexName>> {
        self.iter_rev().await?.next().await.transpose()
    }

    /// Test if this set is empty.
    async fn is_empty(&self) -> Result<bool> {
        self.first().await.map(|n| n.is_none())
    }

    /// Test if this set contains a given name.
    async fn contains(&self, name: &VertexName) -> Result<bool> {
        let mut iter = self.iter().await?;
        while let Some(item) = iter.next().await {
            if &item? == name {
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

/// Sync version of `AsyncNameSetQuery`.
pub trait SyncNameSetQuery {
    /// Iterate through the set in defined order.
    fn iter(&self) -> Result<Box<dyn NameIter>>;

    /// Iterate through the set in the reversed order.
    fn iter_rev(&self) -> Result<Box<dyn NameIter>>;

    /// Number of names in this set.
    fn count(&self) -> Result<usize>;

    /// The first name in the set.
    fn first(&self) -> Result<Option<VertexName>>;

    /// The last name in the set.
    fn last(&self) -> Result<Option<VertexName>>;

    /// Test if this set is empty.
    fn is_empty(&self) -> Result<bool>;

    /// Test if this set contains a given name.
    fn contains(&self, name: &VertexName) -> Result<bool>;

    /// For downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Get or set optimization hints.
    fn hints(&self) -> &Hints;

    /// Get an optional IdConvert interface to check hints.
    fn id_convert(&self) -> Option<&dyn IdConvert>;
}

impl<T: AsyncNameSetQuery> SyncNameSetQuery for T {
    fn iter(&self) -> Result<Box<dyn NameIter>> {
        non_blocking(AsyncNameSetQuery::iter(self))?.map(to_iter)
    }

    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        non_blocking(AsyncNameSetQuery::iter_rev(self))?.map(to_iter)
    }

    fn count(&self) -> Result<usize> {
        non_blocking(AsyncNameSetQuery::count(self))?
    }

    fn first(&self) -> Result<Option<VertexName>> {
        non_blocking(AsyncNameSetQuery::first(self))?
    }

    fn last(&self) -> Result<Option<VertexName>> {
        non_blocking(AsyncNameSetQuery::last(self))?
    }

    fn is_empty(&self) -> Result<bool> {
        non_blocking(AsyncNameSetQuery::is_empty(self))?
    }

    fn contains(&self, name: &VertexName) -> Result<bool> {
        non_blocking(AsyncNameSetQuery::contains(self, name))?
    }

    fn as_any(&self) -> &dyn Any {
        AsyncNameSetQuery::as_any(self)
    }

    fn hints(&self) -> &Hints {
        AsyncNameSetQuery::hints(self)
    }

    fn id_convert(&self) -> Option<&dyn IdConvert> {
        AsyncNameSetQuery::id_convert(self)
    }
}

impl SyncNameSetQuery for NameSet {
    fn iter(&self) -> Result<Box<dyn NameIter>> {
        non_blocking(AsyncNameSetQuery::iter(self.0.deref()))?.map(to_iter)
    }

    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        non_blocking(AsyncNameSetQuery::iter_rev(self.0.deref()))?.map(to_iter)
    }

    fn count(&self) -> Result<usize> {
        non_blocking(AsyncNameSetQuery::count(self.0.deref()))?
    }

    fn first(&self) -> Result<Option<VertexName>> {
        non_blocking(AsyncNameSetQuery::first(self.0.deref()))?
    }

    fn last(&self) -> Result<Option<VertexName>> {
        non_blocking(AsyncNameSetQuery::last(self.0.deref()))?
    }

    fn is_empty(&self) -> Result<bool> {
        non_blocking(AsyncNameSetQuery::is_empty(self.0.deref()))?
    }

    fn contains(&self, name: &VertexName) -> Result<bool> {
        non_blocking(AsyncNameSetQuery::contains(self.0.deref(), name))?
    }

    fn as_any(&self) -> &dyn Any {
        AsyncNameSetQuery::as_any(self.0.deref())
    }

    fn hints(&self) -> &Hints {
        AsyncNameSetQuery::hints(self.0.deref())
    }

    fn id_convert(&self) -> Option<&dyn IdConvert> {
        AsyncNameSetQuery::id_convert(self.0.deref())
    }
}

/// Iterator of [`NameSet`].
/// Types implementing this should consider replacing `iter_rev` with a fast
/// path if possible.
pub trait NameIter: Iterator<Item = Result<VertexName>> + Send {}
impl<T> NameIter for T where T: Iterator<Item = Result<VertexName>> + Send {}

/// Abstract async iterator that yields `Vertex`es.
pub trait VertexStream: Stream<Item = Result<VertexName>> + Send {}
impl<T> VertexStream for T where T: Stream<Item = Result<VertexName>> + Send {}

/// Boxed async iterator that yields `Vertex`es.
pub type BoxVertexStream = Pin<Box<dyn VertexStream>>;

/// A wrapper that converts `VertexStream` to `NameIter`.
struct NonblockingNameIter(BoxVertexStream);

impl Iterator for NonblockingNameIter {
    type Item = Result<VertexName>;

    fn next(&mut self) -> Option<Self::Item> {
        match non_blocking(self.0.next()) {
            Err(e) => Some(Err(e.into())),
            Ok(v) => v,
        }
    }
}

fn to_iter(stream: BoxVertexStream) -> Box<dyn NameIter> {
    Box::new(NonblockingNameIter(stream))
}

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
    use nonblocking::non_blocking_result as r;

    pub(crate) fn nb<F, R>(future: F) -> R
    where
        F: std::future::Future<Output = R>,
    {
        non_blocking(future).unwrap()
    }

    // Converts async Stream to Iterator.
    pub(crate) fn ni<F>(future: F) -> Result<Box<dyn NameIter>>
    where
        F: std::future::Future<Output = Result<BoxVertexStream>>,
    {
        nb(future).map(to_iter)
    }

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

    #[async_trait::async_trait]
    impl AsyncNameSetQuery for VecQuery {
        async fn iter(&self) -> Result<BoxVertexStream> {
            let iter = self.0.clone().into_iter().map(Ok);
            Ok(Box::pin(futures::stream::iter(iter)))
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
        assert_eq!(SyncNameSetQuery::iter(&query)?.count(), 0);
        assert_eq!(SyncNameSetQuery::iter_rev(&query)?.count(), 0);
        assert_eq!(SyncNameSetQuery::first(&query)?, None);
        assert_eq!(SyncNameSetQuery::last(&query)?, None);
        assert_eq!(SyncNameSetQuery::count(&query)?, 0);
        assert!(SyncNameSetQuery::is_empty(&query)?);
        assert!(!SyncNameSetQuery::contains(&query, &to_name(0))?);
        Ok(())
    }

    #[test]
    fn test_vec_query() -> Result<()> {
        let query = VecQuery::from_bytes(b"\xab\xef\xcd");
        check_invariants(&query)?;
        assert_eq!(
            shorten_iter(SyncNameSetQuery::iter(&query)),
            ["ab", "ef", "cd"]
        );
        assert_eq!(
            shorten_iter(SyncNameSetQuery::iter_rev(&query)),
            ["cd", "ef", "ab"]
        );
        assert_eq!(
            shorten_name(SyncNameSetQuery::first(&query)?.unwrap()),
            "ab"
        );
        assert_eq!(shorten_name(SyncNameSetQuery::last(&query)?.unwrap()), "cd");
        assert!(!SyncNameSetQuery::is_empty(&query)?);
        assert!(SyncNameSetQuery::contains(&query, &to_name(0xef))?);
        assert!(!SyncNameSetQuery::contains(&query, &to_name(0))?);
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
            format!("{:?}", r(set.flatten()).unwrap()),
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

    /// Check consistency of a `AsyncNameSetQuery`, such as `iter().nth(0)` matches
    /// `first()` etc.
    pub(crate) fn check_invariants(query: &dyn AsyncNameSetQuery) -> Result<()> {
        let is_empty = nb(query.is_empty())?;
        let count = nb(query.count())?;
        let first = nb(query.first())?;
        let last = nb(query.last())?;
        let names: Vec<VertexName> = ni(query.iter())?.collect::<Result<Vec<_>>>()?;
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
                .all(|name| nb(query.contains(name)).ok() == Some(true)),
            "contains() should return true for names returned by iter() (set: {:?})",
            &query
        );
        assert!(
            (0..=0xff).all(|b| {
                let name = VertexName::from(vec![b; 20]);
                nb(query.contains(&name)).ok() == Some(names.contains(&name))
            }),
            "contains() should return false for names not returned by iter() (set: {:?})",
            &query
        );
        let reversed: Vec<VertexName> = ni(query.iter_rev())?.collect::<Result<Vec<_>>>()?;
        assert_eq!(
            names,
            reversed.into_iter().rev().collect::<Vec<VertexName>>(),
            "iter() should match iter_rev().rev() (set: {:?})",
            &query
        );
        Ok(())
    }
}
