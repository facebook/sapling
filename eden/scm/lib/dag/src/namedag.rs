/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # namedag
//!
//! Combination of IdMap and IdDag.

use crate::id::Group;
use crate::id::VertexName;
use crate::iddag::IdDag;
use crate::iddag::SyncableIdDag;
use crate::idmap::IdMap;
use crate::idmap::IdMapLike;
use crate::idmap::SyncableIdMap;
use crate::nameset::dag::DagSet;
use crate::nameset::NameSet;
use crate::spanset::SpanSet;
use anyhow::{anyhow, bail, ensure, Result};
use indexedlog::multi;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;

/// A DAG that uses VertexName instead of ids as vertexes.
///
/// A high-level wrapper structure. Combination of [`IdMap`] and [`Dag`].
/// Maintains consistency of dag and map internally.
pub struct NameDag {
    pub(crate) dag: IdDag,
    pub(crate) map: IdMap,

    /// A read-only snapshot of the `IdMap` that will be shared in `NameSet`s.
    pub(crate) snapshot_map: Arc<IdMap>,

    mlog: multi::MultiLog,

    /// Heads added via `add_heads` that are not flushed yet.
    pending_heads: Vec<VertexName>,
}

impl NameDag {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let opts = multi::OpenOptions::from_name_opts(vec![
            ("idmap", IdMap::log_open_options()),
            ("iddag", IdDag::log_open_options()),
        ]);
        let mut mlog = opts.open(path)?;
        let mut logs = mlog.detach_logs();
        let dag_log = logs.pop().unwrap();
        let map_log = logs.pop().unwrap();
        let map = IdMap::open_from_log(map_log)?;
        let dag = IdDag::open_from_log(dag_log)?;
        let snapshot_map = Arc::new(map.try_clone()?);
        Ok(Self {
            dag,
            map,
            snapshot_map,
            mlog,
            pending_heads: Default::default(),
        })
    }

    /// Add vertexes and their ancestors to the on-disk DAG.
    ///
    /// This is similar to calling `add_heads` followed by `flush`.
    /// But is faster.
    pub fn add_heads_and_flush<F>(
        &mut self,
        parent_names_func: F,
        master_names: &[VertexName],
        non_master_names: &[VertexName],
    ) -> Result<()>
    where
        F: Fn(VertexName) -> Result<Vec<VertexName>>,
    {
        ensure!(
            self.pending_heads.is_empty(),
            "ProgrammingError: add_heads_and_flush called with pending heads ({:?})",
            &self.pending_heads,
        );
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
        let lock = self.mlog.lock()?;
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
        self.mlog.write_meta(&lock)?;

        // Update snapshot_map.
        self.snapshot_map = Arc::new(self.map.try_clone()?);
        Ok(())
    }

    /// Add vertexes and their ancestors to the in-memory DAG.
    ///
    /// This does not write to disk. Use `add_heads_and_flush` to add heads
    /// and write to disk more efficiently.
    ///
    /// The added vertexes are immediately query-able. They will get Ids
    /// assigned to the NON_MASTER group internally. The `flush` function
    /// can re-assign Ids to the MASTER group.
    pub fn add_heads<F>(&mut self, parents: F, heads: &[VertexName]) -> Result<()>
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

        Ok(())
    }

    /// Write in-memory DAG to disk. This will also pick up changes to
    /// the DAG by other processes.
    pub fn flush(&mut self, master_heads: &[VertexName]) -> Result<()> {
        // Sanity check.
        for head in master_heads.iter() {
            ensure!(
                self.map.find_id_by_name(head.as_ref())?.is_some(),
                "head {:?} does not exist in DAG",
                head
            );
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
            parents_map.get(&name).cloned().ok_or_else(|| {
                anyhow!(
                    "{:?} not found in parent map ({:?}, {:?})",
                    &name,
                    &parents_map,
                    &non_master_heads,
                )
            })
        };
        let flush_result = self.add_heads_and_flush(&parents, master_heads, &non_master_heads);
        if let Err(flush_err) = flush_result {
            // Add back commits to revert the side effect of 'reload()'.
            return match self.add_heads(&parents, &non_master_heads) {
                Ok(_) => Err(flush_err),
                Err(err) => Err(flush_err.context(err)),
            };
        }
        Ok(())
    }

    /// Reload segments from disk. This discards in-memory content.
    pub fn reload(&mut self) -> Result<()> {
        self.map.reload()?;
        self.dag.reload()?;
        self.pending_heads.clear();
        Ok(())
    }

    /// Sort a `NameSet` topologically.
    pub fn sort(&self, set: &NameSet) -> Result<NameSet> {
        if set.is_topo_sorted() {
            Ok(set.clone())
        } else {
            let mut spans = SpanSet::empty();
            for name in set.iter()? {
                let id = self.snapshot_map.vertex_id(name?)?;
                spans.push(id);
            }
            Ok(NameSet::from_spans_idmap(spans, self.snapshot_map.clone()))
        }
    }

    /// Get ordered parent vertexes.
    pub fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>> {
        let id = match self.map.find_id_by_name(name.as_ref())? {
            Some(id) => id,
            None => bail!("{:?} does not exist in DAG", name),
        };
        self.dag
            .parent_ids(id)?
            .into_iter()
            .map(|id| match self.map.find_vertex_name_by_id(id)? {
                Some(name) => Ok(name),
                None => bail!("cannot resolve parent id {} to name", id),
            })
            .collect()
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

    // TODO: Consider implementing these:
    // - NamedSpanSet - SpanSet wrapper that only exposes "names".
    //   - Potentially, it has to implement smartset-like interfaces.
    // - On NameDag, methods wrapping dag algorithms that uses NamedSpanSet
    //   as input and output.
    // Before those APIs, LowLevelAccess might have to be used by callsites.
}

// Dag operations. Those are just simple wrappers around [`IdDag`].
// See [`IdDag`] for the actual implementations of these algorithms.

impl NameDag {
    /// Returns a [`SpanSet`] that covers all vertexes tracked by this DAG.
    pub fn all(&self) -> Result<NameSet> {
        let spans = self.dag.all()?;
        let query = DagSet::from_spans_idmap(spans, self.snapshot_map.clone()).mark_as_all();
        Ok(NameSet::from_query(query))
    }

    /// Calculates all ancestors reachable from any name from the given set.
    pub fn ancestors(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag.ancestors(self.to_span_set(set)?)?;
        Ok(NameSet::from_spans_idmap(spans, self.snapshot_map.clone()))
    }

    /// Calculates parents of the given set.
    ///
    /// Note: Parent order is not preserved. Use [`NameDag::parent_names`]
    /// to preserve order.
    pub fn parents(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag.parents(self.to_span_set(set)?)?;
        Ok(NameSet::from_spans_idmap(spans, self.snapshot_map.clone()))
    }

    /// Calculates the n-th first ancestor.
    pub fn first_ancestor_nth(&self, name: VertexName, n: u64) -> Result<VertexName> {
        let id = self.map.vertex_id(name)?;
        let id = self.dag.first_ancestor_nth(id, n)?;
        self.map.vertex_name(id)
    }

    /// Calculates heads of the given set.
    pub fn heads(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag.heads(self.to_span_set(set)?)?;
        Ok(NameSet::from_spans_idmap(spans, self.snapshot_map.clone()))
    }

    /// Calculates children of the given set.
    pub fn children(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag.children(self.to_span_set(set)?)?;
        Ok(NameSet::from_spans_idmap(spans, self.snapshot_map.clone()))
    }

    /// Calculates roots of the given set.
    pub fn roots(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag.roots(self.to_span_set(set)?)?;
        Ok(NameSet::from_spans_idmap(spans, self.snapshot_map.clone()))
    }

    /// Calculates one "greatest common ancestor" of the given set.
    ///
    /// If there are no common ancestors, return None.
    /// If there are multiple greatest common ancestors, pick one arbitrarily.
    /// Use `gca_all` to get all of them.
    pub fn gca_one(&self, set: NameSet) -> Result<Option<VertexName>> {
        match self.dag.gca_one(self.to_span_set(set)?)? {
            None => Ok(None),
            Some(id) => Ok(Some(self.map.vertex_name(id)?)),
        }
    }

    /// Calculates all "greatest common ancestor"s of the given set.
    /// `gca_one` is faster if an arbitrary answer is ok.
    pub fn gca_all(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag.gca_all(self.to_span_set(set)?)?;
        Ok(NameSet::from_spans_idmap(spans, self.snapshot_map.clone()))
    }

    /// Calculates all common ancestors of the given set.
    pub fn common_ancestors(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag.common_ancestors(self.to_span_set(set)?)?;
        Ok(NameSet::from_spans_idmap(spans, self.snapshot_map.clone()))
    }

    /// Tests if `ancestor` is an ancestor of `descendant`.
    pub fn is_ancestor(&self, ancestor: VertexName, descendant: VertexName) -> Result<bool> {
        let ancestor_id = self.map.vertex_id(ancestor)?;
        let descendant_id = self.map.vertex_id(descendant)?;
        self.dag.is_ancestor(ancestor_id, descendant_id)
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
    pub fn heads_ancestors(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag.heads_ancestors(self.to_span_set(set)?)?;
        Ok(NameSet::from_spans_idmap(spans, self.snapshot_map.clone()))
    }

    /// Calculates the "dag range" - vertexes reachable from both sides.
    pub fn range(&self, roots: NameSet, heads: NameSet) -> Result<NameSet> {
        let roots = self.to_span_set(roots)?;
        let heads = self.to_span_set(heads)?;
        let spans = self.dag.range(roots, heads)?;
        Ok(NameSet::from_spans_idmap(spans, self.snapshot_map.clone()))
    }

    /// Calculates the descendants of the given set.
    pub fn descendants(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag.descendants(self.to_span_set(set)?)?;
        Ok(NameSet::from_spans_idmap(spans, self.snapshot_map.clone()))
    }

    /// Converts [`NameSet`] to [`SpanSet`].
    fn to_span_set(&self, set: NameSet) -> Result<SpanSet> {
        // Fast path: extract SpanSet directly.
        if let Some(set) = set.as_any().downcast_ref::<DagSet>() {
            if Arc::ptr_eq(&set.map, &self.snapshot_map) {
                return Ok(set.spans.clone());
            }
        }

        // Slow path: iterate through the set and convert it to a non-lazy
        // SpanSet.
        let mut spans = SpanSet::empty();
        for name in set.iter()? {
            let name = name?;
            let id = self.map.vertex_id(name)?;
            spans.push(id);
        }
        Ok(spans)
    }
}

/// Export non-master DAG as parent_names_func on HashMap.
///
/// This can be expensive. It is expected to be either called infrequently,
/// or called with a small amount of data. For example, bounded amount of
/// non-master commits.
fn non_master_parent_names(
    map: &SyncableIdMap,
    dag: &SyncableIdDag,
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
pub fn rebuild_non_master(map: &mut SyncableIdMap, dag: &mut SyncableIdDag) -> Result<()> {
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
        None => bail!(
            "bug: parents of {:?} is missing (in rebuild_non_master)",
            name
        ),
    };
    build(map, dag, parent_func, &[], &heads[..])?;

    Ok(())
}

/// Build IdMap and Segments for the given heads.
pub fn build<F>(
    map: &mut SyncableIdMap,
    dag: &mut SyncableIdDag,
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
    fn dag(&self) -> &IdDag;
    fn map(&self) -> &IdMap;
}

unsafe impl LowLevelAccess for NameDag {
    fn dag(&self) -> &IdDag {
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
