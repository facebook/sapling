/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # namedag
//!
//! Combination of IdMap and IdDag.

use crate::errors::bug;
use crate::errors::programming;
use crate::id::Group;
use crate::id::Id;
use crate::id::VertexName;
use crate::iddag::IdDag;
use crate::iddag::SyncableIdDag;
use crate::iddagstore::IdDagStore;
use crate::iddagstore::InProcessStore;
use crate::iddagstore::IndexedLogStore;
use crate::idmap::IdMap;
use crate::idmap::IdMapAssignHead;
use crate::idmap::IdMapBuildParents;
use crate::idmap::IdMapWrite;
use crate::idmap::MemIdMap;
use crate::idmap::SyncableIdMap;
use crate::nameset::hints::Flags;
use crate::nameset::hints::Hints;
use crate::nameset::id_static::IdStaticSet;
use crate::nameset::NameSet;
use crate::nameset::NameSetQuery;
use crate::ops::DagAddHeads;
use crate::ops::DagAlgorithm;
use crate::ops::DagPersistent;
use crate::ops::IdConvert;
use crate::ops::IdMapEq;
use crate::ops::IdMapSnapshot;
use crate::ops::PrefixLookup;
use crate::ops::ToIdSet;
use crate::ops::ToSet;
use crate::spanset::SpanSet;
use crate::Result;
use indexedlog::multi;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::ops::Deref;
use std::path::Path;
use std::sync::Arc;

/// A DAG that uses VertexName instead of ids as vertexes.
///
/// A high-level wrapper structure. Combination of [`IdMap`] and [`Dag`].
/// Maintains consistency of dag and map internally.
pub struct NameDag {
    pub(crate) dag: IdDag<IndexedLogStore>,
    pub(crate) map: IdMap,

    /// A read-only snapshot of the `IdMap` that will be shared in `NameSet`s.
    ///
    /// This also serves as a way to test whether two `NameSet`s are using a
    /// compatible (same) `IdMap` by testing `Arc::ptr_eq`.
    ///
    /// In theory we can also just clone `self.map` every time and get an
    /// `Arc::ptr_eq` equivalent by using some sort of internal version number
    /// that gets bumped when `map` gets changed. However that might be more
    /// expensive.
    pub(crate) snapshot_map: Arc<dyn IdConvert + Send + Sync>,

    /// A read-only snapshot of the `NameDag`.
    /// Lazily calculated.
    snapshot: RwLock<Option<Arc<NameDag>>>,

    /// `MultiLog` controls on-disk metadata.
    /// `None` for read-only `NameDag`,
    mlog: Option<multi::MultiLog>,

    /// Heads added via `add_heads` that are not flushed yet.
    pending_heads: Vec<VertexName>,
}

/// In-memory version of [`NameDag`].
///
/// Does not support loading from or saving to the filesystem.
/// The graph has to be built from scratch by `add_heads`.
pub struct MemNameDag {
    dag: IdDag<InProcessStore>,
    map: MemIdMap,
    snapshot_map: Arc<dyn IdConvert + Send + Sync>,
    snapshot: RwLock<Option<Arc<MemNameDag>>>,
}

impl NameDag {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let opts = multi::OpenOptions::from_name_opts(vec![
            ("idmap", IdMap::log_open_options()),
            ("iddag", IndexedLogStore::log_open_options()),
        ]);
        let mut mlog = opts.open(path)?;
        let mut logs = mlog.detach_logs();
        let dag_log = logs.pop().unwrap();
        let map_log = logs.pop().unwrap();
        let map = IdMap::open_from_log(map_log)?;
        let dag = IdDag::open_from_store(IndexedLogStore::open_from_log(dag_log))?;
        let snapshot_map = Arc::new(map.try_clone()?);
        Ok(Self {
            dag,
            map,
            snapshot_map,
            snapshot: Default::default(),
            mlog: Some(mlog),
            pending_heads: Default::default(),
        })
    }
}

impl DagPersistent for NameDag {
    /// Add vertexes and their ancestors to the on-disk DAG.
    ///
    /// This is similar to calling `add_heads` followed by `flush`.
    /// But is faster.
    fn add_heads_and_flush<F>(
        &mut self,
        parent_names_func: F,
        master_names: &[VertexName],
        non_master_names: &[VertexName],
    ) -> Result<()>
    where
        F: Fn(VertexName) -> Result<Vec<VertexName>>,
    {
        if !self.pending_heads.is_empty() {
            return programming(format!(
                "ProgrammingError: add_heads_and_flush called with pending heads ({:?})",
                &self.pending_heads,
            ));
        }
        // Already include specified nodes?
        if master_names.iter().all(|n| {
            is_ok_some(
                self.map
                    .find_id_by_name_with_max_group(n.as_ref(), Group::MASTER),
            )
        }) && non_master_names
            .iter()
            .all(|n| is_ok_some(self.map.find_id_by_name(n.as_ref())))
        {
            return Ok(());
        }

        // Take lock.
        //
        // Reload meta. This drops in-memory changes, which is fine because we have
        // checked there are no in-memory changes at the beginning.
        if self.mlog.is_none() {
            return bug("MultiLog should be Some for read-write NameDag");
        }
        let mlog = self.mlog.as_mut().unwrap();
        let lock = mlog.lock()?;
        let mut map = self.map.prepare_filesystem_sync()?;
        let mut dag = self.dag.prepare_filesystem_sync()?;

        // Build.
        build(
            &mut map,
            &mut dag,
            parent_names_func,
            master_names,
            non_master_names,
        )?;

        // Write to disk.
        map.sync()?;
        dag.sync(std::iter::once(&mut self.dag))?;
        mlog.write_meta(&lock)?;

        // Update snapshot_map.
        self.snapshot_map = Arc::new(self.map.try_clone()?);
        self.invalidate_snapshot();
        Ok(())
    }

    /// Write in-memory DAG to disk. This will also pick up changes to
    /// the DAG by other processes.
    fn flush(&mut self, master_heads: &[VertexName]) -> Result<()> {
        // Sanity check.
        for head in master_heads.iter() {
            if self.map.find_id_by_name(head.as_ref())?.is_none() {
                return head.not_found();
            }
        }

        // Dump the pending DAG to memory so we can re-assign numbers.
        //
        // PERF: There could be a fast path that does not re-assign numbers.
        // But in practice we might always want to re-assign master commits.
        let parents_map = self.pending_graph()?;
        let mut non_master_heads = Vec::new();
        std::mem::swap(&mut self.pending_heads, &mut non_master_heads);

        self.reload()?;
        let parents = |name| {
            parents_map
                .get(&name)
                .cloned()
                .ok_or_else(|| name.not_found_error())
        };
        let flush_result = self.add_heads_and_flush(&parents, master_heads, &non_master_heads);
        if let Err(flush_err) = flush_result {
            // Attempt to add back commits to revert the side effect of 'reload()'.
            // No slot for "add_heads" error.
            let _ = self.add_heads(&parents, &non_master_heads);
            return Err(flush_err);
        }
        Ok(())
    }
}

impl DagAddHeads for NameDag {
    /// Add vertexes and their ancestors to the in-memory DAG.
    ///
    /// This does not write to disk. Use `add_heads_and_flush` to add heads
    /// and write to disk more efficiently.
    ///
    /// The added vertexes are immediately query-able. They will get Ids
    /// assigned to the NON_MASTER group internally. The `flush` function
    /// can re-assign Ids to the MASTER group.
    fn add_heads<F>(&mut self, parents: F, heads: &[VertexName]) -> Result<()>
    where
        F: Fn(VertexName) -> Result<Vec<VertexName>>,
    {
        // Assign to the NON_MASTER group unconditionally so we can avoid the
        // complexity re-assigning non-master ids.
        //
        // This simplifies the API (not taking 2 groups), but comes with a
        // performance penalty - if the user does want to make one of the head
        // in the "master" group, we have to re-assign ids in flush().
        //
        // Practically, the callsite might want to use add_heads + flush
        // intead of add_heads_and_flush, if:
        // - The callsites cannot figure out "master_heads" at the same time
        //   it does the graph change. For example, hg might know commits
        //   before bookmark movements.
        // - The callsite is trying some temporary graph changes, and does
        //   not want to pollute the on-disk DAG. For example, calculating
        //   a preview of a rebase.
        let group = Group::NON_MASTER;

        // Update IdMap. Keep track of what heads are added.
        for head in heads.iter() {
            if self.map.find_id_by_name(head.as_ref())?.is_none() {
                self.map.assign_head(head.clone(), &parents, group)?;
                self.pending_heads.push(head.clone());
            }
        }

        // Update segments in the NON_MASTER group.
        let parent_ids_func = self.map.build_get_parents_by_id(&parents);
        let id = self.map.next_free_id(group)?;
        if id > group.min_id() {
            self.dag.build_segments_volatile(id - 1, &parent_ids_func)?;
        }

        drop(parent_ids_func);

        // Update snapshot_map so the changes become visible to queries.
        self.snapshot_map = Arc::new(self.map.try_clone()?);
        self.invalidate_snapshot();

        Ok(())
    }
}

impl NameDag {
    /// Reload segments from disk. This discards in-memory content.
    fn reload(&mut self) -> Result<()> {
        self.map.reload()?;
        self.dag.reload()?;
        self.pending_heads.clear();
        Ok(())
    }

    /// Return parent relationship for non-master vertexes reachable from heads
    /// added by `add_heads`.
    fn pending_graph(&self) -> Result<HashMap<VertexName, Vec<VertexName>>> {
        let mut parents_map: HashMap<VertexName, Vec<VertexName>> = Default::default();
        let mut to_visit: Vec<VertexName> = self.pending_heads.clone();
        while let Some(name) = to_visit.pop() {
            let group = self.map.find_id_by_name(name.as_ref())?.map(|i| i.group());
            if group == Some(Group::MASTER) {
                continue;
            }
            let parents = self.parent_names(name.clone())?;
            for parent in parents.iter() {
                to_visit.push(parent.clone());
            }
            parents_map.insert(name, parents);
        }
        Ok(parents_map)
    }

    /// Invalidate cached content. Call this after changing the graph.
    fn invalidate_snapshot(&mut self) {
        *self.snapshot.write() = None;
    }

    /// Attempt to get a snapshot of this graph.
    fn try_snapshot(&self) -> Result<Arc<Self>> {
        if let Some(s) = self.snapshot.read().deref() {
            return Ok(s.clone());
        }

        let mut snapshot = self.snapshot.write();
        match snapshot.deref() {
            Some(s) => Ok(s.clone()),
            None => {
                let cloned = Self {
                    dag: self.dag.try_clone()?,
                    map: self.map.try_clone()?,
                    snapshot_map: self.snapshot_map.clone(),
                    snapshot: Default::default(),
                    mlog: None,
                    pending_heads: self.pending_heads.clone(),
                };
                let result = Arc::new(cloned);
                *snapshot = Some(result.clone());
                Ok(result)
            }
        }
    }
}

impl MemNameDag {
    /// Create an empty [`MemNameDag`].
    pub fn new() -> Self {
        Self {
            dag: IdDag::new_in_process(),
            map: MemIdMap::new(),
            snapshot_map: Arc::new(MemIdMap::new()),
            snapshot: Default::default(),
        }
    }

    /// Invalidate cached content. Call this after changing the graph.
    fn invalidate_snapshot(&mut self) {
        *self.snapshot.write() = None;
    }

    /// Get a snapshot of this graph.
    fn snapshot(&self) -> Arc<Self> {
        if let Some(s) = self.snapshot.read().deref() {
            return s.clone();
        }

        let mut snapshot = self.snapshot.write();
        match snapshot.deref() {
            Some(s) => s.clone(),
            None => {
                let cloned = Self {
                    dag: self.dag.clone(),
                    map: self.map.clone(),
                    snapshot_map: self.snapshot_map.clone(),
                    snapshot: Default::default(),
                };
                let result = Arc::new(cloned);
                *snapshot = Some(result.clone());
                result
            }
        }
    }
}

impl DagAddHeads for MemNameDag {
    /// Add vertexes and their ancestors to the in-memory DAG.
    fn add_heads<F>(&mut self, parents: F, heads: &[VertexName]) -> Result<()>
    where
        F: Fn(VertexName) -> Result<Vec<VertexName>>,
    {
        // For simplicity, just use the master group for now.
        let group = Group::MASTER;
        for head in heads.iter() {
            if self.map.contains_vertex_name(head)? {
                continue;
            }
            self.map.assign_head(head.clone(), &parents, group)?;
        }

        let parent_ids_func = self.map.build_get_parents_by_id(&parents);
        let id = self.map.next_free_id(group)?;
        if id > group.min_id() {
            self.dag.build_segments_volatile(id - 1, &parent_ids_func)?;
        }
        self.snapshot_map = Arc::new(self.map.clone());
        drop(parent_ids_func);
        self.invalidate_snapshot();
        Ok(())
    }
}

// Dag operations. Those are just simple wrappers around [`IdDag`].
// See [`IdDag`] for the actual implementations of these algorithms.

/// DAG related read-only algorithms.
impl<T: ToSet + NameDagStorage> DagAlgorithm for T {
    /// Sort a `NameSet` topologically.
    fn sort(&self, set: &NameSet) -> Result<NameSet> {
        if set.hints().contains(Flags::TOPO_DESC)
            && set.hints().is_dag_compatible(self as &dyn DagAlgorithm)
        {
            Ok(set.clone())
        } else {
            let flags = extract_ancestor_flag_if_compatible(set.hints(), self);
            let mut spans = SpanSet::empty();
            for name in set.iter()? {
                let id = self.map().vertex_id(name?)?;
                spans.push(id);
            }
            let result = NameSet::from_spans_idmap(spans, self.clone_map());
            result.hints().add_flags(flags).set_dag(self);
            Ok(result)
        }
    }

    /// Get ordered parent vertexes.
    fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>> {
        let id = self.map().vertex_id(name)?;
        self.dag()
            .parent_ids(id)?
            .into_iter()
            .map(|id| self.map().vertex_name(id))
            .collect()
    }

    /// Returns a [`SpanSet`] that covers all vertexes tracked by this DAG.
    fn all(&self) -> Result<NameSet> {
        let spans = self.dag().all()?;
        let query = IdStaticSet::from_spans_idmap(spans, self.clone_map());
        let hints = query.hints();
        hints.add_flags(Flags::FULL).set_dag(self);
        let result = NameSet::from_query(query);
        Ok(result)
    }

    /// Calculates all ancestors reachable from any name from the given set.
    fn ancestors(&self, set: NameSet) -> Result<NameSet> {
        if set.hints().contains(Flags::ANCESTORS)
            && set.hints().is_dag_compatible(self as &dyn DagAlgorithm)
        {
            return Ok(set);
        }
        let spans = self.to_id_set(&set)?;
        #[cfg(test)]
        {
            self.to_set(&spans)?.assert_eq(set.clone());
        }
        let spans = self.dag().ancestors(spans)?;
        let result = NameSet::from_spans_idmap(spans, self.clone_map());
        result.hints().add_flags(Flags::ANCESTORS).set_dag(self);
        Ok(result)
    }

    /// Calculates parents of the given set.
    ///
    /// Note: Parent order is not preserved. Use [`NameDag::parent_names`]
    /// to preserve order.
    fn parents(&self, set: NameSet) -> Result<NameSet> {
        // Preserve ANCESTORS flag. If ancestors(x) == x, then ancestors(parents(x)) == parents(x).
        let flags = extract_ancestor_flag_if_compatible(set.hints(), self);
        let spans = self.dag().parents(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_idmap(spans, self.clone_map());
        result.hints().add_flags(flags);
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::parents(self, set)?);
        }
        Ok(result)
    }

    /// Calculates the n-th first ancestor.
    fn first_ancestor_nth(&self, name: VertexName, n: u64) -> Result<VertexName> {
        #[cfg(test)]
        let name2 = name.clone();
        let id = self.map().vertex_id(name)?;
        let id = self.dag().first_ancestor_nth(id, n)?;
        let result = self.map().vertex_name(id)?;
        #[cfg(test)]
        {
            let result2 = crate::default_impl::first_ancestor_nth(self, name2, n)?;
            assert_eq!(result, result2);
        }
        Ok(result)
    }

    /// Calculates heads of the given set.
    fn heads(&self, set: NameSet) -> Result<NameSet> {
        if set.hints().contains(Flags::ANCESTORS)
            && set.hints().is_dag_compatible(self as &dyn DagAlgorithm)
        {
            // heads_ancestors is faster.
            return self.heads_ancestors(set);
        }
        let spans = self.dag().heads(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_idmap(spans, self.clone_map());
        result.hints().set_dag(self);
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::heads(self, set)?);
        }
        Ok(result)
    }

    /// Calculates children of the given set.
    fn children(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag().children(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_idmap(spans, self.clone_map());
        result.hints().set_dag(self);
        Ok(result)
    }

    /// Calculates roots of the given set.
    fn roots(&self, set: NameSet) -> Result<NameSet> {
        let flags = extract_ancestor_flag_if_compatible(set.hints(), self);
        let spans = self.dag().roots(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_idmap(spans, self.clone_map());
        result.hints().add_flags(flags).set_dag(self);
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::roots(self, set)?);
        }
        Ok(result)
    }

    /// Calculates one "greatest common ancestor" of the given set.
    ///
    /// If there are no common ancestors, return None.
    /// If there are multiple greatest common ancestors, pick one arbitrarily.
    /// Use `gca_all` to get all of them.
    fn gca_one(&self, set: NameSet) -> Result<Option<VertexName>> {
        let result: Option<VertexName> = match self.dag().gca_one(self.to_id_set(&set)?)? {
            None => None,
            Some(id) => Some(self.map().vertex_name(id)?),
        };
        #[cfg(test)]
        {
            assert_eq!(&result, &crate::default_impl::gca_one(self, set)?);
        }
        Ok(result)
    }

    /// Calculates all "greatest common ancestor"s of the given set.
    /// `gca_one` is faster if an arbitrary answer is ok.
    fn gca_all(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag().gca_all(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_idmap(spans, self.clone_map());
        result.hints().set_dag(self);
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::gca_all(self, set)?);
        }
        Ok(result)
    }

    /// Calculates all common ancestors of the given set.
    fn common_ancestors(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag().common_ancestors(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_idmap(spans, self.clone_map());
        result.hints().add_flags(Flags::ANCESTORS).set_dag(self);
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::common_ancestors(self, set)?);
        }
        Ok(result)
    }

    /// Tests if `ancestor` is an ancestor of `descendant`.
    fn is_ancestor(&self, ancestor: VertexName, descendant: VertexName) -> Result<bool> {
        #[cfg(test)]
        let result2 = crate::default_impl::is_ancestor(self, ancestor.clone(), descendant.clone())?;
        let ancestor_id = self.map().vertex_id(ancestor)?;
        let descendant_id = self.map().vertex_id(descendant)?;
        let result = self.dag().is_ancestor(ancestor_id, descendant_id)?;
        #[cfg(test)]
        {
            assert_eq!(&result, &result2);
        }
        Ok(result)
    }

    /// Calculates "heads" of the ancestors of the given set. That is,
    /// Find Y, which is the smallest subset of set X, where `ancestors(Y)` is
    /// `ancestors(X)`.
    ///
    /// This is faster than calculating `heads(ancestors(set))`.
    ///
    /// This is different from `heads`. In case set contains X and Y, and Y is
    /// an ancestor of X, but not the immediate ancestor, `heads` will include
    /// Y while this function won't.
    fn heads_ancestors(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag().heads_ancestors(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_idmap(spans, self.clone_map());
        result.hints().set_dag(self);
        #[cfg(test)]
        {
            // default_impl::heads_ancestors calls `heads` if `Flags::ANCESTORS`
            // is set. Prevent infinite loop.
            if !set.hints().contains(Flags::ANCESTORS) {
                result.assert_eq(crate::default_impl::heads_ancestors(self, set)?);
            }
        }
        Ok(result)
    }

    /// Calculates the "dag range" - vertexes reachable from both sides.
    fn range(&self, roots: NameSet, heads: NameSet) -> Result<NameSet> {
        let roots = self.to_id_set(&roots)?;
        let heads = self.to_id_set(&heads)?;
        let spans = self.dag().range(roots, heads)?;
        let result = NameSet::from_spans_idmap(spans, self.clone_map());
        result.hints().set_dag(self);
        Ok(result)
    }

    /// Calculates the descendants of the given set.
    fn descendants(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag().descendants(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_idmap(spans, self.clone_map());
        result.hints().set_dag(self);
        Ok(result)
    }
}

/// Extract the ANCESTORS flag if the set with the `hints` is bound to a
/// compatible DAG.
fn extract_ancestor_flag_if_compatible(hints: &Hints, dag: &dyn DagAlgorithm) -> Flags {
    if hints.is_dag_compatible(dag) {
        hints.flags() & Flags::ANCESTORS
    } else {
        Flags::empty()
    }
}

impl<T> PrefixLookup for T
where
    T: NameDagStorage,
    T::IdMap: PrefixLookup,
{
    fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> Result<Vec<VertexName>> {
        self.map().vertexes_by_hex_prefix(hex_prefix, limit)
    }
}

impl<T> IdConvert for T
where
    T: NameDagStorage,
    T::IdMap: IdConvert,
{
    fn vertex_id(&self, name: VertexName) -> Result<Id> {
        self.map().vertex_id(name)
    }
    fn vertex_id_with_max_group(&self, name: &VertexName, max_group: Group) -> Result<Option<Id>> {
        self.map().vertex_id_with_max_group(name, max_group)
    }
    fn vertex_name(&self, id: Id) -> Result<VertexName> {
        self.map().vertex_name(id)
    }
    fn contains_vertex_name(&self, name: &VertexName) -> Result<bool> {
        self.map().contains_vertex_name(name)
    }
}

/// Export non-master DAG as parent_names_func on HashMap.
///
/// This can be expensive. It is expected to be either called infrequently,
/// or called with a small amount of data. For example, bounded amount of
/// non-master commits.
fn non_master_parent_names(
    map: &SyncableIdMap,
    dag: &SyncableIdDag<IndexedLogStore>,
) -> Result<HashMap<VertexName, Vec<VertexName>>> {
    let parent_ids = dag.non_master_parent_ids()?;
    // Map id to name.
    let parent_names = parent_ids
        .iter()
        .map(|(id, parent_ids)| {
            let name = map.vertex_name(*id)?;
            let parent_names = parent_ids
                .into_iter()
                .map(|p| map.vertex_name(*p))
                .collect::<Result<Vec<_>>>()?;
            Ok((name, parent_names))
        })
        .collect::<Result<HashMap<_, _>>>()?;
    Ok(parent_names)
}

/// Re-assign ids and segments for non-master group.
pub fn rebuild_non_master(
    map: &mut SyncableIdMap,
    dag: &mut SyncableIdDag<IndexedLogStore>,
) -> Result<()> {
    // backup part of the named graph in memory.
    let parents = non_master_parent_names(map, dag)?;
    let mut heads = parents
        .keys()
        .collect::<HashSet<_>>()
        .difference(
            &parents
                .values()
                .flat_map(|ps| ps.into_iter())
                .collect::<HashSet<_>>(),
        )
        .map(|&v| v.clone())
        .collect::<Vec<_>>();
    heads.sort_unstable();

    // Remove existing non-master data.
    dag.remove_non_master()?;
    map.remove_non_master()?;

    // Rebuild them.
    let parent_func = |name: VertexName| match parents.get(&name) {
        Some(names) => Ok(names.iter().cloned().collect()),
        None => bug(format!(
            "bug: parents of {:?} is missing (in rebuild_non_master)",
            name
        )),
    };
    build(map, dag, parent_func, &[], &heads[..])?;

    Ok(())
}

/// Build IdMap and Segments for the given heads.
pub fn build<F>(
    map: &mut SyncableIdMap,
    dag: &mut SyncableIdDag<IndexedLogStore>,
    parent_names_func: F,
    master_heads: &[VertexName],
    non_master_heads: &[VertexName],
) -> Result<()>
where
    F: Fn(VertexName) -> Result<Vec<VertexName>>,
{
    // Update IdMap.
    for (nodes, group) in [
        (master_heads, Group::MASTER),
        (non_master_heads, Group::NON_MASTER),
    ]
    .iter()
    {
        for node in nodes.iter() {
            map.assign_head(node.clone(), &parent_names_func, *group)?;
        }
    }

    // Update segments.
    {
        let parent_ids_func = map.build_get_parents_by_id(&parent_names_func);
        for &group in Group::ALL.iter() {
            let id = map.next_free_id(group)?;
            if id > group.min_id() {
                dag.build_segments_persistent(id - 1, &parent_ids_func)?;
            }
        }
    }

    // Rebuild non-master ids and segments.
    if map.need_rebuild_non_master {
        rebuild_non_master(map, dag)?;
    }

    Ok(())
}

/// Provide low level access to dag and map.
/// Unsafe because it's possible to break consistency by writing to them.
///
/// This is currently used in Python bindings to provide an interface that is
/// consistent with `smartset.abstractsmartset`. Ideally, `smartset` provides
/// public commit hash interface, and there is no LowLevelAccess here.
pub unsafe trait LowLevelAccess {
    fn dag(&self) -> &IdDag<IndexedLogStore>;
    fn map(&self) -> &IdMap;
}

unsafe impl LowLevelAccess for NameDag {
    fn dag(&self) -> &IdDag<IndexedLogStore> {
        &self.dag
    }
    fn map(&self) -> &IdMap {
        &self.map
    }
}

fn is_ok_some<T>(value: Result<Option<T>>) -> bool {
    match value {
        Ok(Some(_)) => true,
        _ => false,
    }
}

/// IdMap + IdDag backend for DagAlgorithm.
pub trait NameDagStorage: IdMapEq {
    type IdDagStore: IdDagStore;
    type IdMap: IdConvert;

    /// The IdDag storage.
    fn dag(&self) -> &IdDag<Self::IdDagStore>;

    /// The IdMap storage.
    fn map(&self) -> &Self::IdMap;

    /// (Cheaply) clone the map.
    fn clone_map(&self) -> Arc<dyn IdConvert + Send + Sync>;

    /// (Relatively cheaply) clone the dag.
    fn snapshot_dag(&self) -> Result<Arc<dyn DagAlgorithm + Send + Sync>>;
}

impl NameDagStorage for NameDag {
    type IdDagStore = IndexedLogStore;
    type IdMap = IdMap;

    fn dag(&self) -> &IdDag<Self::IdDagStore> {
        &self.dag
    }
    fn map(&self) -> &Self::IdMap {
        &self.map
    }
    fn clone_map(&self) -> Arc<dyn IdConvert + Send + Sync> {
        self.snapshot_map.clone()
    }
    fn snapshot_dag(&self) -> Result<Arc<dyn DagAlgorithm + Send + Sync>> {
        Ok(self.try_snapshot()? as Arc<dyn DagAlgorithm + Send + Sync>)
    }
}

impl NameDagStorage for MemNameDag {
    type IdDagStore = InProcessStore;
    type IdMap = MemIdMap;

    fn dag(&self) -> &IdDag<Self::IdDagStore> {
        &self.dag
    }
    fn map(&self) -> &Self::IdMap {
        &self.map
    }
    fn clone_map(&self) -> Arc<dyn IdConvert + Send + Sync> {
        self.snapshot_map.clone()
    }
    fn snapshot_dag(&self) -> Result<Arc<dyn DagAlgorithm + Send + Sync>> {
        Ok(self.snapshot() as Arc<dyn DagAlgorithm + Send + Sync>)
    }
}

impl IdMapEq for NameDag {
    fn is_map_compatible(&self, other: &Arc<dyn IdConvert + Send + Sync>) -> bool {
        Arc::ptr_eq(other, &self.snapshot_map)
    }
}

impl IdMapEq for MemNameDag {
    fn is_map_compatible(&self, other: &Arc<dyn IdConvert + Send + Sync>) -> bool {
        Arc::ptr_eq(other, &self.snapshot_map)
    }
}

impl IdMapSnapshot for NameDag {
    fn id_map_snapshot(&self) -> Result<Arc<dyn IdConvert + Send + Sync>> {
        Ok(Arc::clone(&self.snapshot_map))
    }
}

impl IdMapSnapshot for MemNameDag {
    fn id_map_snapshot(&self) -> Result<Arc<dyn IdConvert + Send + Sync>> {
        Ok(Arc::clone(&self.snapshot_map))
    }
}
