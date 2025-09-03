/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # set
//!
//! See [`Set`] for the main structure.

use std::any::Any;
use std::borrow::Cow;
use std::cmp;
use std::fmt;
use std::fmt::Debug;
use std::ops::Add;
use std::ops::BitAnd;
use std::ops::BitOr;
use std::ops::Deref;
use std::ops::Sub;
use std::pin::Pin;
use std::sync::Arc;

use futures::Stream;
use futures::StreamExt;
use futures::future::BoxFuture;
use nonblocking::non_blocking;

use crate::Id;
use crate::IdList;
use crate::IdSet;
use crate::Result;
use crate::Vertex;
use crate::default_impl;
use crate::ops::DagAlgorithm;
use crate::ops::IdConvert;
use crate::ops::IdMapSnapshot;
use crate::ops::Parents;

pub mod difference;
pub mod hints;
pub mod id_lazy;
pub mod id_static;
pub mod intersection;
pub mod lazy;
#[cfg(any(test, feature = "indexedlog-backend"))]
pub mod legacy;
pub mod meta;
pub mod reverse;
pub mod slice;
pub mod r#static;
pub mod union;

use self::hints::Flags;
use self::hints::Hints;
use self::id_static::BasicIterationOrder;
use self::id_static::IdStaticSet;
use self::meta::MetaSet;
use self::reverse::ReverseSet;
use self::r#static::StaticSet;

/// A [`Set`] contains an immutable list of names.
///
/// It provides order-preserving iteration and set operations,
/// and is cheaply clonable.
#[derive(Clone)]
pub struct Set(Arc<dyn AsyncSetQuery>);

impl Set {
    pub(crate) fn from_query(query: impl AsyncSetQuery) -> Self {
        Self(Arc::new(query))
    }

    /// Creates an empty set.
    pub fn empty() -> Self {
        Self::from_query(r#static::StaticSet::empty())
    }

    /// Creates from a (short) list of known names.
    pub fn from_static_names(names: impl IntoIterator<Item = Vertex>) -> Set {
        Self::from_query(r#static::StaticSet::from_names(names))
    }

    /// Creates from a (lazy) iterator of names.
    pub fn from_iter<I>(iter: I, hints: Hints) -> Set
    where
        I: IntoIterator<Item = Result<Vertex>> + 'static,
        <I as IntoIterator>::IntoIter: Send + Sync,
    {
        Self::from_query(lazy::LazySet::from_iter(iter, hints))
    }

    /// Creates from a (lazy) stream of names with hints.
    pub fn from_stream(stream: BoxVertexStream, hints: Hints) -> Set {
        Self::from_query(lazy::LazySet::from_stream(stream, hints))
    }

    /// Creates from a (lazy) iterator of Ids, an IdMap, and a Dag.
    pub fn from_id_iter_idmap_dag<I>(
        iter: I,
        map: Arc<dyn IdConvert + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
    ) -> Set
    where
        I: IntoIterator<Item = Result<Id>> + 'static,
        <I as IntoIterator>::IntoIter: Send + Sync,
    {
        Self::from_query(id_lazy::IdLazySet::from_iter_idmap_dag(iter, map, dag))
    }

    /// Creates from a (lazy) iterator of Ids and a struct with snapshot abilities.
    pub fn from_id_iter_dag<I>(iter: I, dag: &(impl DagAlgorithm + IdMapSnapshot)) -> Result<Set>
    where
        I: IntoIterator<Item = Result<Id>> + 'static,
        <I as IntoIterator>::IntoIter: Send + Sync,
    {
        let map = dag.id_map_snapshot()?;
        let dag = dag.dag_snapshot()?;
        Ok(Self::from_id_iter_idmap_dag(iter, map, dag))
    }

    /// Creates from [`IdSet`], [`IdMap`] and [`DagAlgorithm`].
    /// Callsite must make sure `spans`, `map`, `dag` are using the same `Id` mappings.
    /// Prefer `from_id_set_dag` if possible.
    pub fn from_id_set_idmap_dag(
        spans: IdSet,
        map: Arc<dyn IdConvert + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
    ) -> Set {
        Self::from_id_set_idmap_dag_order(spans, map, dag, None)
    }

    /// Creates from [`IdSet`], [`IdMap`], [`DagAlgorithm`], and [`BasicIterationOrder`].
    /// Callsite must make sure `spans`, `map`, `dag` are using the same `Id` mappings.
    pub fn from_id_set_idmap_dag_order(
        spans: IdSet,
        map: Arc<dyn IdConvert + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
        iteration_order: Option<BasicIterationOrder>,
    ) -> Set {
        let mut set = IdStaticSet::from_id_set_idmap_dag(spans, map, dag);
        if let Some(order) = iteration_order {
            set.set_iteration_order(order);
        }
        Self::from_query(set)
    }

    /// Creates from [`IdSet`] and a struct with snapshot abilities.
    /// Callsite must make sure `spans`, `dag` are using the same `Id` mappings.
    pub fn from_id_set_dag(
        spans: IdSet,
        dag: &(impl DagAlgorithm + IdMapSnapshot),
    ) -> Result<Self> {
        let map = dag.id_map_snapshot()?;
        let dag = dag.dag_snapshot()?;
        Ok(Self::from_id_set_idmap_dag(spans, map, dag))
    }

    /// Creates from [`IdList`] and a struct with snapshot abilities.
    /// Unlike [`Self::from_id_set_dag`], the iteration order of `list` will be preserved.
    /// Callsite must make sure `list`, `dag` are using the same `Id` mappings.
    pub fn from_id_list_dag(
        list: IdList,
        dag: &(impl DagAlgorithm + IdMapSnapshot),
    ) -> Result<Self> {
        let map = dag.id_map_snapshot()?;
        let dag = dag.dag_snapshot()?;
        let set = IdStaticSet::from_id_list_idmap_dag(list, map, dag);
        Ok(Self::from_query(set))
    }

    /// Creates from a function that evaluates to a [`Set`], and a
    /// `contains` fast path.
    pub fn from_evaluate_contains<C>(
        evaluate: impl Fn() -> Result<Set> + Send + Sync + 'static,
        contains: C,
        hints: Hints,
    ) -> Set
    where
        C: for<'a> Fn(&'a MetaSet, &'a Vertex) -> Result<bool>,
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
            hints,
        )
    }

    /// Creates from an async function that evaluates to a [`Set`], and a
    /// async `contains` fast path.
    pub fn from_async_evaluate_contains(
        evaluate: Box<dyn Fn() -> BoxFuture<'static, Result<Set>> + Send + Sync>,
        contains: Box<
            dyn for<'a> Fn(&'a MetaSet, &'a Vertex) -> BoxFuture<'a, Result<bool>> + Send + Sync,
        >,
        hints: Hints,
    ) -> Set {
        Self::from_query(MetaSet::from_evaluate_hints(evaluate, hints).with_contains(contains))
    }

    /// Reverse the iteration order of the `Set`.
    pub fn reverse(&self) -> Set {
        match self.0.specialized_reverse() {
            Some(set) => set,
            None => Self::from_query(ReverseSet::new(self.clone())),
        }
    }

    /// Calculates the subset that is only in self, not in other.
    pub fn difference(&self, other: &Set) -> Set {
        if other.hints().contains(Flags::FULL)
            && other.hints().dag_version() >= self.hints().dag_version()
            && self.hints().dag_version() > None
        {
            tracing::debug!(
                target: "dag::algo::difference",
                "difference(x={:.6?}, y={:.6?}) = () (fast path 1)",
                self,
                other
            );
            return Self::empty();
        }
        if self.hints().contains(Flags::EMPTY) || other.hints().contains(Flags::EMPTY) {
            tracing::debug!(
                target: "dag::algo::difference",
                "difference(x={:.6?}, y={:.6?}) = x (fast path 2)",
                self,
                other
            );
            return self.clone();
        }
        if let (Some(this), Some(other)) = (
            self.as_any().downcast_ref::<IdStaticSet>(),
            // xs - ys; the order of ys does not matter
            other.specialized_flatten_id(),
        ) {
            let order = this.map.map_version().partial_cmp(other.map.map_version());
            if let (Some(_order), Some(this_id_set)) = (order, this.id_set_try_preserving_order()) {
                // Fast path for IdStaticSet.
                // The order is preserved, `this.is_id_sorted` is true.
                let result = Self::from_id_set_idmap_dag_order(
                    this_id_set.difference(other.id_set_losing_order()),
                    this.map.clone(),
                    this.dag.clone(),
                    this.iteration_order(),
                );
                tracing::debug!(
                    target: "dag::algo::difference",
                    "difference(x={:.6?}, y={:.6?}) = {:.6?} (fast path 3)",
                    self,
                    other,
                    &result
                );
                return result;
            }
        }
        tracing::warn!(
                target: "dag::algo::difference",
            "difference(x={:.6?}, y={:.6?}) (slow path)", self, other);
        Self::from_query(difference::DifferenceSet::new(self.clone(), other.clone()))
    }

    /// Calculates the intersection of two sets.
    pub fn intersection(&self, other: &Set) -> Set {
        if self.hints().contains(Flags::FULL)
            && self.hints().dag_version() >= other.hints().dag_version()
            && other.hints().dag_version() > None
        {
            tracing::debug!(
                target: "dag::algo::intersection",
                "intersection(x={:.6?}, y={:.6?}) = y (fast path 1)",
                self,
                other
            );
            return other.clone();
        }
        if other.hints().contains(Flags::FULL)
            && other.hints().dag_version() >= self.hints().dag_version()
            && self.hints().dag_version() > None
        {
            tracing::debug!(
                target: "dag::algo::intersection",
                "intersection(x={:.6?}, y={:.6?}) = x (fast path 2)",
                self,
                other
            );
            return self.clone();
        }
        if self.hints().contains(Flags::EMPTY) || other.hints().contains(Flags::EMPTY) {
            tracing::debug!(
                target: "dag::algo::intersection",
                "intersection(x={:.6?}, y={:.6?}) = () (fast path 3)",
                self,
                other
            );
            return Self::empty();
        }
        if let (Some(this), Some(other)) = (
            self.as_any().downcast_ref::<IdStaticSet>(),
            // xs & ys; the order of ys does not matter
            other.specialized_flatten_id(),
        ) {
            let order = this.map.map_version().partial_cmp(other.map.map_version());
            if let (Some(order), Some(this_id_set)) = (order, this.id_set_try_preserving_order()) {
                // Fast path for IdStaticSet
                let result = Self::from_id_set_idmap_dag_order(
                    this_id_set.intersection(other.id_set_losing_order()),
                    pick(order, &this.map, &other.map).clone(),
                    pick(order, &this.dag, &other.dag).clone(),
                    this.iteration_order(),
                );
                tracing::debug!(
                    target: "dag::algo::intersection",
                    "intersection(x={:.6?}, y={:.6?}) = {:?} (IdStatic fast path)",
                    self,
                    other,
                    &result
                );
                return result;
            }
        }
        tracing::warn!(
            target: "dag::algo::intersection",
            "intersection(x={:.6?}, y={:.6?}) (slow path)", self, other);
        Self::from_query(intersection::IntersectionSet::new(
            self.clone(),
            other.clone(),
        ))
    }

    /// Union fast paths. Handles when one set is "FULL" or "EMPTY".
    fn union_fast_paths(&self, other: &Self) -> Option<Self> {
        if (self.hints().contains(Flags::FULL)
            && self.hints().dag_version() >= other.hints().dag_version()
            && other.hints().dag_version() > None)
            || other.hints().contains(Flags::EMPTY)
        {
            tracing::debug!(
                target: "dag::algo::union",
                "union(x={:.6?}, y={:.6?}) = x (fast path 1)", self, other);
            return Some(self.clone());
        }
        if self.hints().contains(Flags::EMPTY)
            || (other.hints().contains(Flags::FULL)
                && other.hints().dag_version() >= self.hints().dag_version()
                && self.hints().dag_version() > None)
        {
            tracing::debug!(
                target: "dag::algo::union",
                "union(x={:.6?}, y={:.6?}) = y (fast path 2)", self, other);
            return Some(other.clone());
        }
        None
    }

    /// Calculates the union of two sets. Iteration order might get lost.
    pub fn union(&self, other: &Set) -> Set {
        if let Some(set) = self.union_fast_paths(other) {
            return set;
        }

        // This fast path aggressively flatten the sets. It does not preserve order.
        if let (Some(this), Some(other)) = (
            self.specialized_flatten_id(),
            other.specialized_flatten_id(),
        ) {
            let order = this.map.map_version().partial_cmp(other.map.map_version());
            if let Some(order) = order {
                // Fast path for IdStaticSet
                let result = Self::from_id_set_idmap_dag_order(
                    this.id_set_losing_order()
                        .union(other.id_set_losing_order()),
                    pick(order, &this.map, &other.map).clone(),
                    pick(order, &this.dag, &other.dag).clone(),
                    this.iteration_order(),
                );
                tracing::debug!(
                    target: "dag::algo::union",
                    "union(x={:.6?}, y={:.6?}) = {:.6?} (fast path 3)",
                    self,
                    other,
                    &result
                );
                return result;
            }
        }
        tracing::warn!(
            target: "dag::algo::union",
            "union(x={:.6?}, y={:.6?}) (slow path)", self, other);
        Self::from_query(union::UnionSet::new(self.clone(), other.clone()))
    }

    /// Union, but preserve the iteration order (self first, other next).
    pub fn union_preserving_order(&self, other: &Self) -> Self {
        if let Some(set) = self.union_fast_paths(other) {
            return set;
        }
        tracing::debug!(target: "dag::algo::union_preserving_order", "union(x={:.6?}, y={:.6?})", self, other);
        Self::from_query(union::UnionSet::new(self.clone(), other.clone()))
    }

    /// Similar to `union`, but without showfast paths, and force a "flatten zip" order.
    /// For example `[1,2,3,4].union_zip([5,6])` produces this order: `[1,5,2,6,3,4]`.
    pub fn union_zip(&self, other: &Set) -> Set {
        tracing::debug!(
            target: "dag::algo::union_zip",
            "union_zip(x={:.6?}, y={:.6?}) (slow path)", self, other);
        Self::from_query(
            union::UnionSet::new(self.clone(), other.clone()).with_order(union::UnionOrder::Zip),
        )
    }

    /// Filter using the given async function. If `filter_func` returns `true`
    /// for a vertex, then the vertex will be taken, other it will be skipped.
    pub fn filter(
        &self,
        filter_func: Box<dyn Fn(&Vertex) -> BoxFuture<Result<bool>> + Send + Sync + 'static>,
    ) -> Self {
        let filter_func = Arc::new(filter_func);
        let this = self.clone();
        let hints = {
            // Drop ANCESTORS | FULL and add FILTER.
            let hints = self.hints().clone();
            hints.update_flags_with(|f| (f - Flags::ANCESTORS - Flags::FULL) | Flags::FILTER);
            hints
        };
        let result = Self::from_async_evaluate_contains(
            Box::new({
                let filter_func = filter_func.clone();
                let this = this.clone();
                let hints = hints.clone();
                move || {
                    let filter_func = filter_func.clone();
                    let this = this.clone();
                    let hints = hints.clone();
                    Box::pin(async move {
                        let stream = this.0.iter().await?;
                        let stream = stream.filter_map(move |v| {
                            let filter_func = filter_func.clone();
                            async move {
                                match v {
                                    Ok(v) => match filter_func(&v).await {
                                        Ok(true) => Some(Ok(v)),
                                        Ok(false) => None,
                                        Err(e) => Some(Err(e)),
                                    },
                                    Err(e) => Some(Err(e)),
                                }
                            }
                        });
                        Ok(Self::from_stream(Box::pin(stream), hints))
                    })
                }
            }),
            Box::new(move |_, v| {
                let filter_func = filter_func.clone();
                let this = this.clone();
                Box::pin(async move { Ok(this.0.contains(v).await? && filter_func(v).await?) })
            }),
            hints,
        );
        result.hints().add_flags(Flags::FILTER);
        result
    }

    /// Convert the set to a graph containing only the vertexes in the set. This can be slow on
    /// larger sets.
    pub async fn to_parents(&self) -> Result<Option<impl Parents + use<>>> {
        default_impl::set_to_parents(self).await
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
    ///
    /// By flattening, the iteration order might be lost.
    pub async fn flatten(&self) -> Result<Set> {
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
    ///
    /// By flattening, the iteration order might be lost.
    pub async fn flatten_id(
        &self,
        id_map: Arc<dyn IdConvert + Send + Sync>,
        dag: Arc<dyn DagAlgorithm + Send + Sync>,
    ) -> Result<Set> {
        if self.as_any().is::<IdStaticSet>() {
            return Ok(self.clone());
        }
        let mut ids = Vec::with_capacity(self.count()?.try_into()?);
        for vertex in self.iter()? {
            let id = id_map.vertex_id(vertex?).await?;
            ids.push(id);
        }
        ids.sort_unstable_by_key(|i| u64::MAX - i.0);
        let spans = IdSet::from_sorted_spans(ids);
        let flat_set = Set::from_id_set_idmap_dag(spans, id_map, dag);
        flat_set.hints().inherit_flags_min_max_id(self.hints());
        Ok(flat_set)
    }

    /// Convert this set to a static name set.
    pub async fn flatten_names(&self) -> Result<Set> {
        if self.as_any().is::<StaticSet>() {
            return Ok(self.clone());
        }
        let names = self.iter()?.collect::<Result<Vec<_>>>()?;
        let flat_set = Self::from_static_names(names);
        flat_set.hints().inherit_flags_min_max_id(self.hints());
        Ok(flat_set)
    }

    /// Take the first `n` items.
    pub fn take(&self, n: u64) -> Set {
        match self.specialized_take(n) {
            Some(set) => {
                tracing::debug!("take(x={:.6?}, {}) (specialized path)", self, n);
                set
            }
            _ => {
                tracing::debug!("take(x={:.6?}, {}) (universal path)", self, n);
                let set = slice::SliceSet::new(self.clone(), 0, Some(n));
                Self::from_query(set)
            }
        }
    }

    /// Skip the first `n` items.
    pub fn skip(&self, n: u64) -> Set {
        if n == 0 {
            return self.clone();
        }
        match self.specialized_skip(n) {
            Some(set) => {
                tracing::debug!("skip(x={:.6?}, {}) (specialized path)", self, n);
                set
            }
            _ => {
                tracing::debug!("skip(x={:.6?}, {}) (universal path)", self, n);
                let set = slice::SliceSet::new(self.clone(), n, None);
                Self::from_query(set)
            }
        }
    }

    /// Converts to `(IdSet, IdConvert)` pair in O(1). If the underlying set
    /// cannot provide such information in O(1), return `None`.
    ///
    /// Useful if the callsite wants to have random access (ex.pathhistory)
    /// and control how to resolve in batches.
    pub fn to_id_set_and_id_map_in_o1(&self) -> Option<(IdSet, Arc<dyn IdConvert + Send + Sync>)> {
        let id_set = self.specialized_flatten_id()?.into_owned();
        Some((id_set.id_set_losing_order().clone(), id_set.map))
    }
}

impl BitAnd for Set {
    type Output = Self;

    fn bitand(self, other: Self) -> Self {
        self.intersection(&other)
    }
}

impl BitOr for Set {
    type Output = Self;

    fn bitor(self, other: Self) -> Self {
        self.union(&other)
    }
}

impl Add for Set {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        self.union_preserving_order(&rhs)
    }
}

impl Sub for Set {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        self.difference(&other)
    }
}

impl Deref for Set {
    type Target = dyn AsyncSetQuery;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl fmt::Debug for Set {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Read-only queries required by [`Set`]: Iteration, length and contains.
///
/// Types implementing this trait should rewrite methods to use fast paths
/// when possible.
#[async_trait::async_trait]
pub trait AsyncSetQuery: Any + Debug + Send + Sync {
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

    /// Number of names in this set. Do not override.
    ///
    /// This function has some built-in fast paths.
    /// For individual set types, override count_slow, size_hint instead of count.
    async fn count(&self) -> Result<u64> {
        if let Some(flat) = self.specialized_flatten_id() {
            return flat.count_slow().await;
        }
        self.count_slow().await
    }

    /// "Slow" count implementation. Intended to be overridden.
    ///
    /// This is intended to be implemented by individual set types as fallbacks
    /// when the universal fast paths do not work.
    async fn count_slow(&self) -> Result<u64> {
        let mut iter = self.iter().await?;
        let mut count = 0;
        while let Some(item) = iter.next().await {
            item?;
            count += 1;
        }
        Ok(count)
    }

    /// Returns the bounds on the length of the set as a hint.
    /// The first item is the lower bound.
    /// The second item is the upper bound.
    /// This method should not block on long operations like waiting for network.
    async fn size_hint(&self) -> (u64, Option<u64>) {
        (0, None)
    }

    /// The first name in the set.
    async fn first(&self) -> Result<Option<Vertex>> {
        self.iter().await?.next().await.transpose()
    }

    /// The last name in the set.
    async fn last(&self) -> Result<Option<Vertex>> {
        self.iter_rev().await?.next().await.transpose()
    }

    /// Test if this set is empty.
    async fn is_empty(&self) -> Result<bool> {
        self.first().await.map(|n| n.is_none())
    }

    /// Test if this set contains a given name.
    async fn contains(&self, name: &Vertex) -> Result<bool> {
        let mut iter = self.iter().await?;
        while let Some(item) = iter.next().await {
            if &item? == name {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Test contains in less than O(N) time.
    /// Returns None if cannot achieve in less than O(N) time.
    async fn contains_fast(&self, name: &Vertex) -> Result<Option<bool>> {
        let _ = name;
        Ok(None)
    }

    /// For downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Get or set optimization hints.
    fn hints(&self) -> &Hints;

    /// Get an optional IdConvert interface to check hints.
    fn id_convert(&self) -> Option<&dyn IdConvert> {
        None
    }

    /// Specialized "reverse" implementation.
    /// Returns `None` to use the general purpose reverse implementation.
    fn specialized_reverse(&self) -> Option<Set> {
        None
    }

    /// Specialized "take" implementation.
    /// Returns `None` to use the general purpose implementation.
    fn specialized_take(&self, n: u64) -> Option<Set> {
        let _ = n;
        None
    }

    /// Specialized "take" implementation.
    /// Returns `None` to use the general purpose implementation.
    fn specialized_skip(&self, n: u64) -> Option<Set> {
        let _ = n;
        None
    }

    /// Specialized "flatten_id" implementation.
    fn specialized_flatten_id(&self) -> Option<Cow<'_, IdStaticSet>> {
        None
    }
}

/// Sync version of `AsyncSetQuery`.
pub trait SyncSetQuery {
    /// Iterate through the set in defined order.
    fn iter(&self) -> Result<Box<dyn NameIter>>;

    /// Iterate through the set in the reversed order.
    fn iter_rev(&self) -> Result<Box<dyn NameIter>>;

    /// Number of names in this set.
    fn count(&self) -> Result<u64>;

    /// The first name in the set.
    fn first(&self) -> Result<Option<Vertex>>;

    /// The last name in the set.
    fn last(&self) -> Result<Option<Vertex>>;

    /// Test if this set is empty.
    fn is_empty(&self) -> Result<bool>;

    /// Test if this set contains a given name.
    fn contains(&self, name: &Vertex) -> Result<bool>;

    /// For downcasting.
    fn as_any(&self) -> &dyn Any;

    /// Get or set optimization hints.
    fn hints(&self) -> &Hints;

    /// Get an optional IdConvert interface to check hints.
    fn id_convert(&self) -> Option<&dyn IdConvert>;
}

impl<T: AsyncSetQuery> SyncSetQuery for T {
    fn iter(&self) -> Result<Box<dyn NameIter>> {
        non_blocking(AsyncSetQuery::iter(self))?.map(to_iter)
    }

    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        non_blocking(AsyncSetQuery::iter_rev(self))?.map(to_iter)
    }

    fn count(&self) -> Result<u64> {
        non_blocking(AsyncSetQuery::count_slow(self))?
    }

    fn first(&self) -> Result<Option<Vertex>> {
        non_blocking(AsyncSetQuery::first(self))?
    }

    fn last(&self) -> Result<Option<Vertex>> {
        non_blocking(AsyncSetQuery::last(self))?
    }

    fn is_empty(&self) -> Result<bool> {
        non_blocking(AsyncSetQuery::is_empty(self))?
    }

    fn contains(&self, name: &Vertex) -> Result<bool> {
        non_blocking(AsyncSetQuery::contains(self, name))?
    }

    fn as_any(&self) -> &dyn Any {
        AsyncSetQuery::as_any(self)
    }

    fn hints(&self) -> &Hints {
        AsyncSetQuery::hints(self)
    }

    fn id_convert(&self) -> Option<&dyn IdConvert> {
        AsyncSetQuery::id_convert(self)
    }
}

impl SyncSetQuery for Set {
    fn iter(&self) -> Result<Box<dyn NameIter>> {
        non_blocking(AsyncSetQuery::iter(self.0.deref()))?.map(to_iter)
    }

    fn iter_rev(&self) -> Result<Box<dyn NameIter>> {
        non_blocking(AsyncSetQuery::iter_rev(self.0.deref()))?.map(to_iter)
    }

    fn count(&self) -> Result<u64> {
        non_blocking(AsyncSetQuery::count_slow(self.0.deref()))?
    }

    fn first(&self) -> Result<Option<Vertex>> {
        non_blocking(AsyncSetQuery::first(self.0.deref()))?
    }

    fn last(&self) -> Result<Option<Vertex>> {
        non_blocking(AsyncSetQuery::last(self.0.deref()))?
    }

    fn is_empty(&self) -> Result<bool> {
        non_blocking(AsyncSetQuery::is_empty(self.0.deref()))?
    }

    fn contains(&self, name: &Vertex) -> Result<bool> {
        non_blocking(AsyncSetQuery::contains(self.0.deref(), name))?
    }

    fn as_any(&self) -> &dyn Any {
        AsyncSetQuery::as_any(self.0.deref())
    }

    fn hints(&self) -> &Hints {
        AsyncSetQuery::hints(self.0.deref())
    }

    fn id_convert(&self) -> Option<&dyn IdConvert> {
        AsyncSetQuery::id_convert(self.0.deref())
    }
}

/// Iterator of [`Set`].
/// Types implementing this should consider replacing `iter_rev` with a fast
/// path if possible.
pub trait NameIter: Iterator<Item = Result<Vertex>> + Send {}
impl<T> NameIter for T where T: Iterator<Item = Result<Vertex>> + Send {}

/// Abstract async iterator that yields `Vertex`es.
pub trait VertexStream: Stream<Item = Result<Vertex>> + Send {}
impl<T> VertexStream for T where T: Stream<Item = Result<Vertex>> + Send {}

/// Boxed async iterator that yields `Vertex`es.
pub type BoxVertexStream = Pin<Box<dyn VertexStream>>;

/// A wrapper that converts `VertexStream` to `NameIter`.
struct NonblockingNameIter(BoxVertexStream);

impl Iterator for NonblockingNameIter {
    type Item = Result<Vertex>;

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

impl From<Vertex> for Set {
    fn from(name: Vertex) -> Set {
        Set::from_static_names(std::iter::once(name))
    }
}

impl From<&Vertex> for Set {
    fn from(name: &Vertex) -> Set {
        Set::from_static_names(std::iter::once(name.clone()))
    }
}

/// Pick `left` if `order` is "greater or equal".
/// Pick `right` otherwise.
fn pick<T>(order: cmp::Ordering, left: T, right: T) -> T {
    match order {
        cmp::Ordering::Greater | cmp::Ordering::Equal => left,
        cmp::Ordering::Less => right,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use futures::TryStreamExt;
    use nonblocking::non_blocking_result as r;

    use super::*;
    use crate::Id;
    pub(crate) use crate::tests::dbg;

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
    impl From<&str> for Set {
        fn from(name: &str) -> Set {
            Set::from_static_names(
                name.split_whitespace()
                    .map(|n| Vertex::copy_from(n.as_bytes())),
            )
        }
    }

    impl Set {
        pub(crate) fn assert_eq(&self, other: Set) {
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

    type SizeHint = (u64, Option<u64>);

    #[derive(Default, Debug)]
    pub(crate) struct VecQuery(Vec<Vertex>, Hints, SizeHint);

    #[async_trait::async_trait]
    impl AsyncSetQuery for VecQuery {
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

        async fn size_hint(&self) -> SizeHint {
            self.2
        }
    }

    impl VecQuery {
        /// Quickly create [`VecQuery`] that contains `len(bytes)` items.
        pub(crate) fn from_bytes(bytes: &[u8]) -> Self {
            let mut used = [false; 256];
            let v: Vec<Vertex> = bytes
                .iter()
                .filter_map(|&b| {
                    if used[b as usize] {
                        None
                    } else {
                        used[b as usize] = true;
                        Some(to_name(b))
                    }
                })
                .collect();
            let size_hint: SizeHint = (v.len() as u64, Some(v.len() as u64));
            Self(v, Hints::default(), size_hint)
        }

        /// Adjust the "size_hint" to test various logic.
        /// - "size_min" will be reduced by the 1st bit of `adjust` (0 to 1).
        /// - "size_max" will be increased by the 2nd bit  of `adjust` (0 to 1).
        /// - If `adjust` is greater than 3, the "size_max" will be set to `None`.
        pub(crate) fn adjust_size_hint(mut self, adjust: u64) -> Self {
            assert!(adjust <= 6);
            self.2.0 = self.2.0.saturating_sub(adjust & 0b1);
            self.2.1 = self.2.1.map(|v| v + ((adjust >> 1) & 0b1));
            if adjust >= 4 {
                self.2.1 = None;
            }
            self
        }
    }

    /// Create a [`Vertex`] from `u8` by repeating them.
    pub(crate) fn to_name(value: u8) -> Vertex {
        Vertex::from(vec![value; 2])
    }

    /// Shorten a [`Vertex`] result.
    pub(crate) fn shorten_name(name: Vertex) -> String {
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
        assert_eq!(SyncSetQuery::iter(&query)?.count(), 0);
        assert_eq!(SyncSetQuery::iter_rev(&query)?.count(), 0);
        assert_eq!(SyncSetQuery::first(&query)?, None);
        assert_eq!(SyncSetQuery::last(&query)?, None);
        assert_eq!(SyncSetQuery::count(&query)?, 0);
        assert!(SyncSetQuery::is_empty(&query)?);
        assert!(!SyncSetQuery::contains(&query, &to_name(0))?);
        Ok(())
    }

    #[test]
    fn test_vec_query() -> Result<()> {
        let query = VecQuery::from_bytes(b"\xab\xef\xcd");
        check_invariants(&query)?;
        assert_eq!(shorten_iter(SyncSetQuery::iter(&query)), ["ab", "ef", "cd"]);
        assert_eq!(
            shorten_iter(SyncSetQuery::iter_rev(&query)),
            ["cd", "ef", "ab"]
        );
        assert_eq!(shorten_name(SyncSetQuery::first(&query)?.unwrap()), "ab");
        assert_eq!(shorten_name(SyncSetQuery::last(&query)?.unwrap()), "cd");
        assert!(!SyncSetQuery::is_empty(&query)?);
        assert!(SyncSetQuery::contains(&query, &to_name(0xef))?);
        assert!(!SyncSetQuery::contains(&query, &to_name(0))?);
        Ok(())
    }

    #[test]
    fn test_debug() {
        let set = Set::from_static_names(vec![to_name(2)])
            .union(&Set::from_static_names(vec![to_name(1)]))
            .difference(
                &Set::from_static_names(vec![to_name(3)])
                    .intersection(&Set::from_static_names(vec![to_name(2), to_name(3)])),
            );
        assert_eq!(
            dbg(&set),
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
        let set = Set::from_static_names(vec![to_name(2)])
            .union(&Set::from_static_names(vec![to_name(1)]))
            .difference(
                &Set::from_static_names(vec![to_name(3)])
                    .intersection(&Set::from_static_names(vec![to_name(2), to_name(3)])),
            );
        assert_eq!(dbg(r(set.flatten()).unwrap()), "<static [0202, 0101]>");
    }

    #[test]
    fn test_union_zip() {
        let set1 = Set::from_static_names(vec![to_name(1), to_name(2), to_name(3), to_name(4)]);
        let set2 = Set::from_static_names(vec![to_name(5), to_name(6)]);
        let unioned = set1.union_zip(&set2);
        let names = unioned.iter().unwrap().collect::<Result<Vec<_>>>().unwrap();
        assert_eq!(dbg(names), "[0101, 0505, 0202, 0606, 0303, 0404]");
    }

    #[test]
    fn test_ops() {
        let ab: Set = "a b".into();
        let bc: Set = "b c".into();
        let s = |set: Set| -> Vec<String> { shorten_iter(set.iter()) };
        assert_eq!(s(ab.clone() | bc.clone()), ["61", "62", "63"]);
        assert_eq!(s(ab.clone() & bc.clone()), ["62"]);
        assert_eq!(s(ab - bc), ["61"]);
    }

    #[test]
    fn test_skip_take_slow_path() {
        let s: Set = "a b c d".into();
        let d = |set: Set| -> String { dbg(r(set.flatten_names()).unwrap()) };
        assert_eq!(d(s.take(2)), "<static [a, b]>");
        assert_eq!(d(s.skip(2)), "<static [c, d]>");
        assert_eq!(d(s.skip(1).take(2)), "<static [b, c]>");
    }

    #[test]
    fn test_hints_empty_full_fast_paths() {
        let partial: Set = "a".into();
        partial.hints().add_flags(Flags::ID_ASC);
        let empty: Set = "".into();
        let full: Set = "f".into();
        full.hints().add_flags(Flags::FULL | Flags::ID_DESC);

        assert_eq!(
            hints_ops(&partial, &empty),
            [
                "- Hints(Flags(ID_ASC))",
                "  Hints(Flags(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS))",
                "& Hints(Flags(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS))",
                "  Hints(Flags(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS))",
                "| Hints(Flags(ID_ASC))",
                "  Hints(Flags(ID_ASC))"
            ]
        );
        // Fast paths are not used for "|" because there is no dag associated.
        assert_eq!(
            hints_ops(&partial, &full),
            [
                "- Hints(Flags(ID_ASC))",
                "  Hints(Flags(ID_DESC))",
                "& Hints(Flags(ID_ASC))",
                "  Hints(Flags(ID_ASC))",
                "| Hints(Flags(0x0))",
                "  Hints(Flags(0x0))"
            ]
        );
        assert_eq!(
            hints_ops(&empty, &full),
            [
                "- Hints(Flags(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS))",
                "  Hints(Flags(FULL | ID_DESC | ANCESTORS))",
                "& Hints(Flags(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS))",
                "  Hints(Flags(EMPTY | ID_DESC | ID_ASC | TOPO_DESC | ANCESTORS))",
                "| Hints(Flags(FULL | ID_DESC | ANCESTORS))",
                "  Hints(Flags(FULL | ID_DESC | ANCESTORS))"
            ]
        );
    }

    #[test]
    fn test_hints_full_subset() {
        let mut t = crate::tests::TestDag::new();
        let a = r(t.dag.all()).unwrap(); // [] FULL EMPTY
        t.drawdag("X", &[]);
        let b = r(t.dag.all()).unwrap(); // [X] FULL
        t.drawdag("X--Y--Z", &[]);
        let c = r(t.dag.all()).unwrap(); // [X Y Z] FULL
        let d = r(t.dag.heads(r(t.dag.all()).unwrap())).unwrap(); // [Z]

        let a = move || a.clone();
        let b = move || b.clone();
        let c = move || c.clone();
        let d = move || d.clone();
        let f = |set: Set| {
            let s = dbg(&set);
            let v = set
                .iter()
                .unwrap()
                .map(|i| String::from_utf8(i.unwrap().as_ref().to_vec()).unwrap())
                .collect::<Vec<String>>()
                .join(" ");
            format!("{} = [{}]", s, v)
        };

        assert_eq!(f(a()), "<spans []> = []");
        assert_eq!(f(b()), "<spans [X+N0]> = [X]");
        assert_eq!(f(c()), "<spans [X:Z+N0:N2]> = [Z Y X]");
        assert_eq!(f(d()), "<spans [Z+N2]> = [Z]");

        assert_eq!(f(a() - c()), "<empty> = []");
        assert_eq!(f(a() - d()), "<spans []> = []");
        assert_eq!(f(b() - c()), "<empty> = []");
        assert_eq!(f(b() - d()), "<spans [X+N0]> = [X]");
        assert_eq!(f(c() - b()), "<spans [Y:Z+N1:N2]> = [Z Y]");
        assert_eq!(f(c() - a()), "<spans [X:Z+N0:N2]> = [Z Y X]");
        assert_eq!(f(c() - d()), "<spans [X:Y+N0:N1]> = [Y X]");
        assert_eq!(f(d() - a()), "<spans [Z+N2]> = [Z]");
        assert_eq!(f(d() - b()), "<spans [Z+N2]> = [Z]");
        assert_eq!(f(d() - c()), "<empty> = []");

        assert_eq!(f(a() | c()), "<spans [X:Z+N0:N2]> = [Z Y X]");
        assert_eq!(f(a() | b()), "<spans [X+N0]> = [X]");
        assert_eq!(f(a() | d()), "<spans [Z+N2]> = [Z]");
        assert_eq!(f(b() | a()), "<spans [X+N0]> = [X]");
        assert_eq!(f(b() | c()), "<spans [X:Z+N0:N2]> = [Z Y X]");
        assert_eq!(f(b() | d()), "<spans [Z+N2, X+N0]> = [Z X]");
        assert_eq!(f(c() | a()), "<spans [X:Z+N0:N2]> = [Z Y X]");
        assert_eq!(f(c() | b()), "<spans [X:Z+N0:N2]> = [Z Y X]");
        assert_eq!(f(c() | d()), "<spans [X:Z+N0:N2]> = [Z Y X]");
        assert_eq!(f(d() | a()), "<spans [Z+N2]> = [Z]");
        assert_eq!(f(d() | b()), "<spans [Z+N2, X+N0]> = [Z X]");
        assert_eq!(f(d() | c()), "<spans [X:Z+N0:N2]> = [Z Y X]");

        assert_eq!(f(a() & c()), "<spans []> = []");
        assert_eq!(f(a() & d()), "<empty> = []");
        assert_eq!(f(b() & c()), "<spans [X+N0]> = [X]");
        assert_eq!(f(c() & a()), "<spans []> = []");
        assert_eq!(f(c() & b()), "<spans [X+N0]> = [X]");
        assert_eq!(f(c() & d()), "<spans [Z+N2]> = [Z]");
        assert_eq!(f(d() & a()), "<empty> = []");
        assert_eq!(f(d() & b()), "<spans []> = []");
        assert_eq!(f(d() & c()), "<spans [Z+N2]> = [Z]");
    }

    #[test]
    fn test_hints_min_max_id() {
        let bc: Set = "b c".into();
        bc.hints().set_min_id(Id(20));
        bc.hints().add_flags(Flags::ID_DESC);

        let ad: Set = "a d".into();
        ad.hints().set_max_id(Id(40));
        ad.hints().add_flags(Flags::ID_ASC);

        assert_eq!(
            hints_ops(&bc, &ad),
            [
                "- Hints(Flags(ID_DESC), 20..)",
                "  Hints(Flags(ID_ASC), ..=40)",
                "& Hints(Flags(ID_DESC), 20..=40)",
                "  Hints(Flags(ID_ASC), 20..=40)",
                "| Hints(Flags(0x0))",
                "  Hints(Flags(0x0))"
            ]
        );

        ad.hints().set_min_id(Id(10));
        bc.hints().set_max_id(Id(30));
        assert_eq!(
            hints_ops(&bc, &ad),
            [
                "- Hints(Flags(ID_DESC), 20..=30)",
                "  Hints(Flags(ID_ASC), 10..=40)",
                "& Hints(Flags(ID_DESC), 20..=30)",
                "  Hints(Flags(ID_ASC), 20..=30)",
                "| Hints(Flags(0x0))",
                "  Hints(Flags(0x0))"
            ]
        );
    }

    #[test]
    fn test_hints_ancestors() {
        let a: Set = "a".into();
        a.hints().add_flags(Flags::ANCESTORS);

        let b: Set = "b".into();
        assert_eq!(
            hints_ops(&a, &b),
            [
                "- Hints(Flags(0x0))",
                "  Hints(Flags(0x0))",
                "& Hints(Flags(0x0))",
                "  Hints(Flags(0x0))",
                "| Hints(Flags(0x0))",
                "  Hints(Flags(0x0))"
            ]
        );

        b.hints().add_flags(Flags::ANCESTORS);
        assert_eq!(
            hints_ops(&a, &b),
            [
                "- Hints(Flags(0x0))",
                "  Hints(Flags(0x0))",
                "& Hints(Flags(ANCESTORS))",
                "  Hints(Flags(ANCESTORS))",
                "| Hints(Flags(ANCESTORS))",
                "  Hints(Flags(ANCESTORS))"
            ]
        );
    }

    #[test]
    fn test_filter() {
        id_static::tests::with_dag(|dag| {
            let sets: Vec<Set> = vec!["C B A".into(), nb(dag.ancestors("C".into())).unwrap()];
            for abc in sets {
                let filter: Set = abc.filter(Box::new(|v: &Vertex| {
                    Box::pin(async move { Ok(v.as_ref() != b"A") })
                }));
                check_invariants(filter.0.as_ref()).unwrap();
                assert_eq!(abc.hints().dag_version(), filter.hints().dag_version());
                assert_eq!(
                    abc.hints().id_map_version(),
                    filter.hints().id_map_version()
                );
                assert!(filter.hints().flags().contains(Flags::FILTER));
                assert!(!filter.hints().flags().contains(Flags::ANCESTORS));
                assert_eq!(dbg(r(filter.flatten_names())), "Ok(<static [C, B]>)");
            }
        })
    }

    #[test]
    fn test_reverse() {
        let ab: Set = "a b".into();
        let ba = ab.reverse();
        check_invariants(&*ba).unwrap();
        let names = ba.iter().unwrap().collect::<Result<Vec<_>>>().unwrap();
        assert_eq!(dbg(names), "[b, a]");
    }

    // Print hints for &, |, - operations.
    fn hints_ops(lhs: &Set, rhs: &Set) -> Vec<String> {
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

    /// Check consistency of a `AsyncSetQuery`, such as `iter().nth(0)` matches
    /// `first()` etc.
    pub(crate) fn check_invariants(query: &dyn AsyncSetQuery) -> Result<()> {
        // Collect contains_fast result before calling other functions which might
        // change the internal set state.
        let contains_fast_vec: Vec<Option<bool>> = (0..=0xff)
            .map(|b| {
                let name = Vertex::from(vec![b; 20]);
                nb(query.contains_fast(&name)).unwrap_or(None)
            })
            .collect();
        let is_empty = nb(query.is_empty())?;
        let count = nb(query.count_slow())?;
        let (size_hint_min, size_hint_max) = nb(query.size_hint());
        let first = nb(query.first())?;
        let last = nb(query.last())?;
        let names: Vec<Vertex> = ni(query.iter())?.collect::<Result<Vec<_>>>()?;
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
            names.len() as u64,
            "count() should match iter().count() (set: {:?})",
            &query
        );
        assert!(
            size_hint_min <= count,
            "size_hint().0 ({}) must <= count ({}) (set: {:?})",
            size_hint_min,
            count,
            &query
        );
        if let Some(size_hint_max) = size_hint_max {
            assert!(
                size_hint_max >= count,
                "size_hint().1 ({}) must >= count ({}) (set: {:?})",
                size_hint_max,
                count,
                &query
            );
        }
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
            names
                .iter()
                .all(|name| nb(query.contains_fast(name)).unwrap_or(None) != Some(false)),
            "contains_fast() should not return false for names returned by iter() (set: {:?})",
            &query
        );
        assert!(
            (0..=0xff).all(|b| {
                let name = Vertex::from(vec![b; 20]);
                nb(query.contains(&name)).ok() == Some(names.contains(&name))
            }),
            "contains() should return false for names not returned by iter() (set: {:?})",
            &query
        );
        assert!(
            (0..=0xff)
                .zip(contains_fast_vec.into_iter())
                .all(|(b, old_contains)| {
                    let name = Vertex::from(vec![b; 20]);
                    let contains = nb(query.contains_fast(&name)).unwrap_or(None);
                    old_contains.is_none() || contains == old_contains
                }),
            "contains_fast() should be consistent (set: {:?})",
            &query
        );
        assert!(
            (0..=0xff).all(|b| {
                let name = Vertex::from(vec![b; 20]);
                let contains = nb(query.contains_fast(&name)).unwrap_or(None);
                contains.is_none() || contains == Some(names.contains(&name))
            }),
            "contains_fast() should not return true for names not returned by iter() (set: {:?})",
            &query
        );
        if let Some(flatten_id) = query.specialized_flatten_id() {
            let iter = r(AsyncSetQuery::iter(&*flatten_id))?;
            let mut flatten_names = r(iter.try_collect::<Vec<_>>())?;
            flatten_names.sort_unstable();
            let mut sorted_names = names.clone();
            sorted_names.sort_unstable();
            assert_eq!(
                &sorted_names, &flatten_names,
                "specialized_flatten_id() should return a same set, order could be different (set: {:?})",
                &query
            );
        }
        let reversed: Vec<Vertex> = ni(query.iter_rev())?.collect::<Result<Vec<_>>>()?;
        if let Some(reversed_set) = query.specialized_reverse() {
            let iter = reversed_set.iter()?;
            let names = iter.collect::<Result<Vec<_>>>()?;
            assert_eq!(&names, &reversed);
        }
        assert_eq!(
            names,
            reversed.into_iter().rev().collect::<Vec<Vertex>>(),
            "iter() should match iter_rev().rev() (set: {:?})",
            &query
        );
        Ok(())
    }

    /// Generate 2 sets in a loop to test container set (intersection, union, difference) types.
    /// Focus on extra "size_hint" test.
    pub(crate) fn check_size_hint_sets<Q: AsyncSetQuery>(build_set: impl Fn(Set, Set) -> Q) {
        let lhs = b"\x11\x22\x33";
        let rhs = b"\x33\x55\x77";
        for lhs_start in 0..lhs.len() {
            for lhs_end in lhs_start..lhs.len() {
                for rhs_start in 0..rhs.len() {
                    for rhs_end in rhs_start..rhs.len() {
                        for lhs_size_hint_adjust in 0..7 {
                            for rhs_size_hint_adjust in 0..7 {
                                let lhs_set = VecQuery::from_bytes(&lhs[lhs_start..lhs_end])
                                    .adjust_size_hint(lhs_size_hint_adjust);
                                let rhs_set = VecQuery::from_bytes(&rhs[rhs_start..rhs_end])
                                    .adjust_size_hint(rhs_size_hint_adjust);
                                let set =
                                    build_set(Set::from_query(lhs_set), Set::from_query(rhs_set));
                                check_invariants(&set).unwrap();
                            }
                        }
                    }
                }
            }
        }
    }

    pub(crate) fn check_skip_take_reverse(set: Set) -> Result<()> {
        let names: Vec<Vertex> = set.iter()?.collect::<Result<Vec<_>>>()?;
        let len = names.len();
        for reverse in [false, true] {
            for skip in 0..=(len + 2) {
                for take in 0..=(len + 2) {
                    for skip_first in [false, true] {
                        let mut test_set = set.clone();
                        let mut expected_names = names.clone();
                        if reverse {
                            test_set = test_set.reverse();
                            expected_names.reverse();
                        }
                        if skip_first {
                            test_set = test_set.skip(skip as _).take(take as _);
                            expected_names =
                                expected_names.into_iter().skip(skip).take(take).collect();
                        } else {
                            test_set = test_set.take(take as _).skip(skip as _);
                            expected_names =
                                expected_names.into_iter().take(take).skip(skip).collect();
                        }
                        let actual_names = test_set.iter()?.collect::<Result<Vec<_>>>()?;
                        assert_eq!(
                            actual_names, expected_names,
                            "check_skip_take_reverse {:?} failed at reverse={reverse} skip={skip} take={take}",
                            &set
                        );
                    }
                }
            }
        }
        Ok(())
    }

    pub(crate) fn fmt_iter(set: &Set) -> String {
        let iter = r(AsyncSetQuery::iter(set.deref())).unwrap();
        let names = r(iter.try_collect::<Vec<_>>()).unwrap();
        dbg(names)
    }
}
