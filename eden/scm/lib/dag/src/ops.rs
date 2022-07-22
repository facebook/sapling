/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! DAG and Id operations (mostly traits)

use std::sync::Arc;

use futures::StreamExt;
use futures::TryStreamExt;

use crate::clone::CloneData;
use crate::default_impl;
use crate::errors::NotFoundError;
use crate::id::Group;
use crate::id::Id;
use crate::id::VertexName;
pub use crate::iddag::IdDagAlgorithm;
use crate::namedag::MemNameDag;
use crate::nameset::id_lazy::IdLazySet;
use crate::nameset::id_static::IdStaticSet;
use crate::nameset::NameSet;
use crate::IdSet;
use crate::Result;
use crate::VerLink;
use crate::VertexListWithOptions;

/// DAG related read-only algorithms.
#[async_trait::async_trait]
pub trait DagAlgorithm: Send + Sync {
    /// Sort a `NameSet` topologically.
    async fn sort(&self, set: &NameSet) -> Result<NameSet>;

    /// Re-create the graph so it looks better when rendered.
    async fn beautify(&self, main_branch: Option<NameSet>) -> Result<MemNameDag> {
        default_impl::beautify(self, main_branch).await
    }

    /// Extract a sub graph containing only specified vertexes.
    async fn subdag(&self, set: NameSet) -> Result<MemNameDag> {
        default_impl::subdag(self, set).await
    }

    /// Get ordered parent vertexes.
    async fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>>;

    /// Returns a set that covers all vertexes tracked by this DAG.
    async fn all(&self) -> Result<NameSet>;

    /// Returns a set that covers all vertexes in the master group.
    async fn master_group(&self) -> Result<NameSet>;

    /// Calculates all ancestors reachable from any name from the given set.
    async fn ancestors(&self, set: NameSet) -> Result<NameSet>;

    /// Calculates parents of the given set.
    ///
    /// Note: Parent order is not preserved. Use [`NameDag::parent_names`]
    /// to preserve order.
    async fn parents(&self, set: NameSet) -> Result<NameSet> {
        default_impl::parents(self, set).await
    }

    /// Calculates the n-th first ancestor.
    async fn first_ancestor_nth(&self, name: VertexName, n: u64) -> Result<Option<VertexName>> {
        default_impl::first_ancestor_nth(self, name, n).await
    }

    /// Calculates ancestors but only follows the first parent.
    async fn first_ancestors(&self, set: NameSet) -> Result<NameSet> {
        default_impl::first_ancestors(self, set).await
    }

    /// Calculates heads of the given set.
    async fn heads(&self, set: NameSet) -> Result<NameSet> {
        default_impl::heads(self, set).await
    }

    /// Calculates children of the given set.
    async fn children(&self, set: NameSet) -> Result<NameSet>;

    /// Calculates roots of the given set.
    async fn roots(&self, set: NameSet) -> Result<NameSet> {
        default_impl::roots(self, set).await
    }

    /// Calculates merges of the selected set (vertexes with >=2 parents).
    async fn merges(&self, set: NameSet) -> Result<NameSet> {
        default_impl::merges(self, set).await
    }

    /// Calculates one "greatest common ancestor" of the given set.
    ///
    /// If there are no common ancestors, return None.
    /// If there are multiple greatest common ancestors, pick one arbitrarily.
    /// Use `gca_all` to get all of them.
    async fn gca_one(&self, set: NameSet) -> Result<Option<VertexName>> {
        default_impl::gca_one(self, set).await
    }

    /// Calculates all "greatest common ancestor"s of the given set.
    /// `gca_one` is faster if an arbitrary answer is ok.
    async fn gca_all(&self, set: NameSet) -> Result<NameSet> {
        default_impl::gca_all(self, set).await
    }

    /// Calculates all common ancestors of the given set.
    async fn common_ancestors(&self, set: NameSet) -> Result<NameSet> {
        default_impl::common_ancestors(self, set).await
    }

    /// Tests if `ancestor` is an ancestor of `descendant`.
    async fn is_ancestor(&self, ancestor: VertexName, descendant: VertexName) -> Result<bool> {
        default_impl::is_ancestor(self, ancestor, descendant).await
    }

    /// Calculates "heads" of the ancestors of the given set. That is,
    /// Find Y, which is the smallest subset of set X, where `ancestors(Y)` is
    /// `ancestors(X)`.
    ///
    /// This is faster than calculating `heads(ancestors(set))` in certain
    /// implementations like segmented changelog.
    ///
    /// This is different from `heads`. In case set contains X and Y, and Y is
    /// an ancestor of X, but not the immediate ancestor, `heads` will include
    /// Y while this function won't.
    async fn heads_ancestors(&self, set: NameSet) -> Result<NameSet> {
        default_impl::heads_ancestors(self, set).await
    }

    /// Calculates the "dag range" - vertexes reachable from both sides.
    async fn range(&self, roots: NameSet, heads: NameSet) -> Result<NameSet>;

    /// Calculates `ancestors(reachable) - ancestors(unreachable)`.
    async fn only(&self, reachable: NameSet, unreachable: NameSet) -> Result<NameSet> {
        default_impl::only(self, reachable, unreachable).await
    }

    /// Calculates `ancestors(reachable) - ancestors(unreachable)`, and
    /// `ancestors(unreachable)`.
    /// This might be faster in some implementations than calculating `only` and
    /// `ancestors` separately.
    async fn only_both(
        &self,
        reachable: NameSet,
        unreachable: NameSet,
    ) -> Result<(NameSet, NameSet)> {
        default_impl::only_both(self, reachable, unreachable).await
    }

    /// Calculates the descendants of the given set.
    async fn descendants(&self, set: NameSet) -> Result<NameSet>;

    /// Calculates `roots` that are reachable from `heads` without going
    /// through other `roots`. For example, given the following graph:
    ///
    /// ```plain,ignore
    ///   F
    ///   |\
    ///   C E
    ///   | |
    ///   B D
    ///   |/
    ///   A
    /// ```
    ///
    /// `reachable_roots(roots=[A, B, C], heads=[F])` returns `[A, C]`.
    /// `B` is not included because it cannot be reached without going
    /// through another root `C` from `F`. `A` is included because it
    /// can be reached via `F -> E -> D -> A` that does not go through
    /// other roots.
    ///
    /// The can be calculated as
    /// `roots & (heads | parents(only(heads, roots & ancestors(heads))))`.
    /// Actual implementation might have faster paths.
    ///
    /// The `roots & ancestors(heads)` portion filters out bogus roots for
    /// compatibility, if the callsite does not provide bogus roots, it
    /// could be simplified to just `roots`.
    async fn reachable_roots(&self, roots: NameSet, heads: NameSet) -> Result<NameSet> {
        default_impl::reachable_roots(self, roots, heads).await
    }

    /// Vertexes buffered in memory, not yet written to disk.
    async fn dirty(&self) -> Result<NameSet>;

    /// Returns true if the vertex names might need to be resolved remotely.
    fn is_vertex_lazy(&self) -> bool;

    /// Get a snapshot of the current graph that can operate separately.
    ///
    /// This makes it easier to fight with borrowck.
    fn dag_snapshot(&self) -> Result<Arc<dyn DagAlgorithm + Send + Sync>>;

    /// Get a snapshot of the `IdDag` that can operate separately.
    ///
    /// This is for advanced use-cases. For example, if callsite wants to
    /// do some graph calculation without network, and control how to
    /// batch the vertex name lookups precisely.
    fn id_dag_snapshot(&self) -> Result<Arc<dyn IdDagAlgorithm + Send + Sync>> {
        Err(crate::errors::BackendError::Generic(format!(
            "id_dag_snapshot() is not supported for {}",
            std::any::type_name::<Self>()
        ))
        .into())
    }

    /// Identity of the dag.
    fn dag_id(&self) -> &str;

    /// Version of the dag. Useful to figure out compatibility between two dags.
    fn dag_version(&self) -> &VerLink;
}

#[async_trait::async_trait]
pub trait Parents: Send + Sync {
    async fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>>;

    /// A hint of a sub-graph for inserting `heads`.
    ///
    /// This is used to reduce remote fetches in a lazy graph.
    /// The roots will be checked first, if a root is unknown locally then
    /// all its descendants will be considered unknown locally.
    ///
    /// The returned graph is only used to optimize network fetches in
    /// `assign_head`. It is not used to be actually inserted to the graph. So
    /// returning an empty or "incorrect" graph does not hurt correctness. But
    /// might hurt performance.
    async fn hint_subdag_for_insertion(&self, _heads: &[VertexName]) -> Result<MemNameDag>;
}

#[async_trait::async_trait]
impl Parents for Arc<dyn DagAlgorithm + Send + Sync> {
    async fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>> {
        DagAlgorithm::parent_names(self, name).await
    }

    async fn hint_subdag_for_insertion(&self, heads: &[VertexName]) -> Result<MemNameDag> {
        let scope = self.dirty().await?;
        default_impl::hint_subdag_for_insertion(self, &scope, heads).await
    }
}

#[async_trait::async_trait]
impl Parents for &(dyn DagAlgorithm + Send + Sync) {
    async fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>> {
        DagAlgorithm::parent_names(*self, name).await
    }

    async fn hint_subdag_for_insertion(&self, heads: &[VertexName]) -> Result<MemNameDag> {
        let scope = self.dirty().await?;
        default_impl::hint_subdag_for_insertion(self, &scope, heads).await
    }
}

#[async_trait::async_trait]
impl<'a> Parents for Box<dyn Fn(VertexName) -> Result<Vec<VertexName>> + Send + Sync + 'a> {
    async fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>> {
        (self)(name)
    }

    async fn hint_subdag_for_insertion(&self, _heads: &[VertexName]) -> Result<MemNameDag> {
        // No clear way to detect the "dirty" scope.
        Ok(MemNameDag::new())
    }
}

#[async_trait::async_trait]
impl Parents for std::collections::HashMap<VertexName, Vec<VertexName>> {
    async fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>> {
        match self.get(&name) {
            Some(v) => Ok(v.clone()),
            None => name.not_found(),
        }
    }

    async fn hint_subdag_for_insertion(&self, heads: &[VertexName]) -> Result<MemNameDag> {
        let mut keys: Vec<VertexName> = self.keys().cloned().collect();
        keys.sort_unstable();
        let scope = NameSet::from_static_names(keys);
        default_impl::hint_subdag_for_insertion(self, &scope, heads).await
    }
}

/// Add vertexes recursively to the DAG.
#[async_trait::async_trait]
pub trait DagAddHeads {
    /// Add vertexes and their ancestors to the DAG. This does not persistent
    /// changes to disk.
    async fn add_heads(
        &mut self,
        parents: &dyn Parents,
        heads: &VertexListWithOptions,
    ) -> Result<bool>;
}

/// Remove vertexes and their descendants from the DAG.
#[async_trait::async_trait]
pub trait DagStrip {
    /// Remove the given `set` and their descendants.
    ///
    /// This will reload the DAG from its source (ex. filesystem) and writes
    /// changes back with a lock so there are no other processes adding
    /// new descendants of the stripped set.
    ///
    /// After strip, the `self` graph might contain new vertexes because of
    /// the reload.
    async fn strip(&mut self, set: &NameSet) -> Result<()>;
}

/// Import a generated `CloneData` object into an empty DAG.
#[async_trait::async_trait]
pub trait DagImportCloneData {
    /// Updates the DAG using a `CloneData` object.
    async fn import_clone_data(&mut self, clone_data: CloneData<VertexName>) -> Result<()>;
}

/// Import a generated incremental `CloneData` object into an existing DAG.
/// Ids in the passed CloneData might not match ids in existing DAG.
#[async_trait::async_trait]
pub trait DagImportPullData {
    /// Updates the DAG using a `CloneData` object.
    ///
    /// Only import the given `heads`.
    async fn import_pull_data(
        &mut self,
        clone_data: CloneData<VertexName>,
        heads: &VertexListWithOptions,
    ) -> Result<()>;
}

#[async_trait::async_trait]
pub trait DagExportCloneData {
    /// Export `CloneData` for vertexes in the master group.
    async fn export_clone_data(&self) -> Result<CloneData<VertexName>>;
}

#[async_trait::async_trait]
pub trait DagExportPullData {
    /// Export `CloneData` for vertexes in the given set.
    /// The set is typcially calculated by `only(heads, common)`.
    async fn export_pull_data(&self, set: &NameSet) -> Result<CloneData<VertexName>>;
}

/// Persistent the DAG on disk.
#[async_trait::async_trait]
pub trait DagPersistent {
    /// Write in-memory DAG to disk. This might also pick up changes to
    /// the DAG by other processes.
    async fn flush(&mut self, master_heads: &VertexListWithOptions) -> Result<()>;

    /// Write in-memory IdMap that caches Id <-> Vertex translation from
    /// remote service to disk.
    async fn flush_cached_idmap(&self) -> Result<()>;

    /// A faster path for add_heads, followed by flush.
    async fn add_heads_and_flush(
        &mut self,
        parent_names_func: &dyn Parents,
        heads: &VertexListWithOptions,
    ) -> Result<()>;

    /// Import from another (potentially large) DAG. Write to disk immediately.
    async fn import_and_flush(
        &mut self,
        dag: &dyn DagAlgorithm,
        master_heads: NameSet,
    ) -> Result<()> {
        let heads = dag.heads(dag.all().await?).await?;
        let non_master_heads = heads - master_heads.clone();
        let master_heads: Vec<VertexName> =
            master_heads.iter().await?.try_collect::<Vec<_>>().await?;
        let non_master_heads: Vec<VertexName> = non_master_heads
            .iter()
            .await?
            .try_collect::<Vec<_>>()
            .await?;
        let heads = VertexListWithOptions::from(master_heads)
            .with_highest_group(Group::MASTER)
            .chain(non_master_heads);
        self.add_heads_and_flush(&dag.dag_snapshot()?, &heads).await
    }
}

/// Import ASCII graph to DAG.
pub trait ImportAscii {
    /// Import vertexes described in an ASCII graph.
    /// `heads` optionally specifies the order of heads to insert.
    /// Useful for testing. Panic if the input is invalid.
    fn import_ascii_with_heads(
        &mut self,
        text: &str,
        heads: Option<&[impl AsRef<str>]>,
    ) -> Result<()>;

    /// Import vertexes described in an ASCII graph.
    fn import_ascii(&mut self, text: &str) -> Result<()> {
        self.import_ascii_with_heads(text, <Option<&[&str]>>::None)
    }
}

/// Lookup vertexes by prefixes.
#[async_trait::async_trait]
pub trait PrefixLookup {
    /// Lookup vertexes by hex prefix.
    async fn vertexes_by_hex_prefix(
        &self,
        hex_prefix: &[u8],
        limit: usize,
    ) -> Result<Vec<VertexName>>;
}

/// Convert between `Vertex` and `Id`.
#[async_trait::async_trait]
pub trait IdConvert: PrefixLookup + Sync {
    async fn vertex_id(&self, name: VertexName) -> Result<Id>;
    async fn vertex_id_with_max_group(
        &self,
        name: &VertexName,
        max_group: Group,
    ) -> Result<Option<Id>>;
    async fn vertex_name(&self, id: Id) -> Result<VertexName>;
    async fn contains_vertex_name(&self, name: &VertexName) -> Result<bool>;

    /// Test if an `id` is present locally. Do not trigger remote fetching.
    async fn contains_vertex_id_locally(&self, id: &[Id]) -> Result<Vec<bool>>;

    /// Test if an `name` is present locally. Do not trigger remote fetching.
    async fn contains_vertex_name_locally(&self, name: &[VertexName]) -> Result<Vec<bool>>;

    async fn vertex_id_optional(&self, name: &VertexName) -> Result<Option<Id>> {
        self.vertex_id_with_max_group(name, Group::NON_MASTER).await
    }

    /// Convert [`Id`]s to [`VertexName`]s in batch.
    async fn vertex_name_batch(&self, ids: &[Id]) -> Result<Vec<Result<VertexName>>> {
        // This is not an efficient implementation in an async context.
        let mut names = Vec::with_capacity(ids.len());
        for &id in ids {
            names.push(self.vertex_name(id).await);
        }
        Ok(names)
    }

    /// Convert [`VertexName`]s to [`Id`]s in batch.
    async fn vertex_id_batch(&self, names: &[VertexName]) -> Result<Vec<Result<Id>>> {
        // This is not an efficient implementation in an async context.
        let mut ids = Vec::with_capacity(names.len());
        for name in names {
            ids.push(self.vertex_id(name.clone()).await);
        }
        Ok(ids)
    }

    /// Identity of the map.
    fn map_id(&self) -> &str;

    /// Version of the map. Useful to figure out compatibility between two maps.
    fn map_version(&self) -> &VerLink;
}

/// Integrity check functions.
#[async_trait::async_trait]
pub trait CheckIntegrity {
    /// Verify that universally known `Id`s (parents of merges) are actually
    /// known locally.
    ///
    /// Returns set of `Id`s that should be universally known but missing.
    /// An empty set means all universally known `Id`s are known locally.
    ///
    /// Check `FirstAncestorConstraint::KnownUniversally` for concepts of
    /// "universally known".
    async fn check_universal_ids(&self) -> Result<Vec<Id>>;

    /// Check segment properties: no cycles, no overlaps, no gaps etc.
    /// This is only about the `Id`s, not about the vertex names.
    ///
    /// Returns human readable messages about problems.
    /// No messages indicates there are no problems detected.
    async fn check_segments(&self) -> Result<Vec<String>>;

    /// Check that the subset of the current graph (ancestors of `heads`)
    /// is isomorphic with the subset in the `other` graph.
    ///
    /// Returns messages about where two graphs are different.
    /// No messages indicates two graphs are likely isomorphic.
    ///
    /// Note: For performance, this function only checks the "shape"
    /// of the graph, without checking the (potentially lazy) vertex
    /// names.
    async fn check_isomorphic_graph(
        &self,
        other: &dyn DagAlgorithm,
        heads: NameSet,
    ) -> Result<Vec<String>>;
}

impl<T> ImportAscii for T
where
    T: DagAddHeads,
{
    fn import_ascii_with_heads(
        &mut self,
        text: &str,
        heads: Option<&[impl AsRef<str>]>,
    ) -> Result<()> {
        let parents = drawdag::parse(&text);
        let heads: Vec<_> = match heads {
            Some(heads) => heads
                .iter()
                .map(|s| VertexName::copy_from(s.as_ref().as_bytes()))
                .collect(),
            None => {
                let mut heads: Vec<_> = parents
                    .keys()
                    .map(|s| VertexName::copy_from(s.as_bytes()))
                    .collect();
                heads.sort();
                heads
            }
        };

        let v = |s: String| VertexName::copy_from(s.as_bytes());
        let parents: std::collections::HashMap<VertexName, Vec<VertexName>> = parents
            .into_iter()
            .map(|(k, vs)| (v(k), vs.into_iter().map(v).collect()))
            .collect();
        nonblocking::non_blocking_result(self.add_heads(&parents, &heads[..].into()))?;
        Ok(())
    }
}

#[async_trait::async_trait]
pub trait ToIdSet {
    /// Converts [`NameSet`] to [`IdSet`].
    async fn to_id_set(&self, set: &NameSet) -> Result<IdSet>;
}

pub trait ToSet {
    /// Converts [`IdSet`] to [`NameSet`].
    fn to_set(&self, set: &IdSet) -> Result<NameSet>;
}

pub trait IdMapSnapshot {
    /// Get a snapshot of IdMap.
    fn id_map_snapshot(&self) -> Result<Arc<dyn IdConvert + Send + Sync>>;
}

/// Describes how to persist state to disk.
pub trait Persist {
    /// Return type of `lock()`.
    type Lock: Send + Sync;

    /// Obtain an exclusive lock for writing.
    /// This should prevent other writers.
    fn lock(&mut self) -> Result<Self::Lock>;

    /// Reload from the source of truth. Drop pending changes.
    ///
    /// This requires a lock and is usually called before `persist()`.
    fn reload(&mut self, _lock: &Self::Lock) -> Result<()>;

    /// Write pending changes to the source of truth.
    ///
    /// This requires a lock.
    fn persist(&mut self, _lock: &Self::Lock) -> Result<()>;
}

/// Address that can be used to open things.
///
/// The address type decides the return type of `open`.
pub trait Open: Clone {
    type OpenTarget;

    fn open(&self) -> Result<Self::OpenTarget>;
}

/// Has an integer tuple version that can be used to test if the data was
/// changed. If the first number changes, it means incompatible changes.
/// If only the second number increases, it means append-only changes.
pub trait IntVersion {
    fn int_version(&self) -> (u64, u64);
}

/// Fallible clone.
pub trait TryClone {
    fn try_clone(&self) -> Result<Self>
    where
        Self: Sized;
}

impl<T: Clone> TryClone for T {
    fn try_clone(&self) -> Result<Self> {
        Ok(self.clone())
    }
}

#[async_trait::async_trait]
impl<T: IdConvert + IdMapSnapshot> ToIdSet for T {
    /// Converts [`NameSet`] to [`IdSet`].
    async fn to_id_set(&self, set: &NameSet) -> Result<IdSet> {
        let version = set.hints().id_map_version();

        // Fast path: extract IdSet from IdStaticSet.
        if let Some(set) = set.as_any().downcast_ref::<IdStaticSet>() {
            if None < version && version <= Some(self.map_version()) {
                return Ok(set.spans.clone());
            }
        }

        // Convert IdLazySet to IdStaticSet. Bypass hash lookups.
        if let Some(set) = set.as_any().downcast_ref::<IdLazySet>() {
            if None < version && version <= Some(self.map_version()) {
                let set: IdStaticSet = set.to_static()?;
                return Ok(set.spans);
            }
        }

        // Slow path: iterate through the set and convert it to a non-lazy
        // IdSet. Does not bypass hash lookups.
        let mut spans = IdSet::empty();
        let mut iter = set.iter().await?.chunks(1 << 17);
        while let Some(names) = iter.next().await {
            let names = names.into_iter().collect::<Result<Vec<_>>>()?;
            let ids = self.vertex_id_batch(&names).await?;
            for id in ids {
                spans.push(id?);
            }
        }
        Ok(spans)
    }
}

impl IdMapSnapshot for Arc<dyn IdConvert + Send + Sync> {
    fn id_map_snapshot(&self) -> Result<Arc<dyn IdConvert + Send + Sync>> {
        Ok(self.clone())
    }
}

impl<T: IdMapSnapshot + DagAlgorithm> ToSet for T {
    /// Converts [`IdSet`] to [`NameSet`].
    fn to_set(&self, set: &IdSet) -> Result<NameSet> {
        NameSet::from_spans_dag(set.clone(), self)
    }
}
