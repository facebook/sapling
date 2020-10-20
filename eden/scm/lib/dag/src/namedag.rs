/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # namedag
//!
//! Combination of IdMap and IdDag.

use crate::delegate;
use crate::errors::bug;
use crate::errors::programming;
use crate::id::Group;
use crate::id::Id;
use crate::id::VertexName;
use crate::iddag::IdDag;
use crate::iddagstore::IdDagStore;
use crate::iddagstore::InProcessStore;
use crate::iddagstore::IndexedLogStore;
use crate::idmap::AssignHeadOutcome;
use crate::idmap::IdMap;
use crate::idmap::IdMapAssignHead;
use crate::idmap::MemIdMap;
use crate::locked::Locked;
use crate::nameset::hints::Flags;
use crate::nameset::hints::Hints;
use crate::nameset::NameSet;
use crate::ops::DagAddHeads;
use crate::ops::DagAlgorithm;
use crate::ops::DagPersistent;
use crate::ops::IdConvert;
use crate::ops::IdMapSnapshot;
use crate::ops::Open;
use crate::ops::Persist;
use crate::ops::PrefixLookup;
use crate::ops::ToIdSet;
use crate::ops::TryClone;
use crate::segment::SegmentFlags;
use crate::spanset::SpanSet;
use crate::Result;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

#[cfg(test)]
use crate::idmap::IdMapBuildParents;

mod indexedlog_namedag;
mod mem_namedag;

pub use indexedlog_namedag::IndexedLogNameDagPath;
pub use indexedlog_namedag::NameDag;
pub use mem_namedag::MemNameDag;
pub use mem_namedag::MemNameDagPath;

pub struct AbstractNameDag<I, M, P, S> {
    pub(crate) dag: I,
    pub(crate) map: M,

    /// A read-only snapshot of the `NameDag`.
    /// Lazily calculated.
    snapshot: RwLock<Option<Arc<Self>>>,

    /// Heads added via `add_heads` that are not flushed yet.
    pending_heads: Vec<VertexName>,

    /// Path used to open this `NameDag`.
    path: P,

    /// Extra state of the `NameDag`.
    state: S,
}

impl<IS, M, P, S> DagPersistent for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore + Persist,
    IdDag<IS>: TryClone + Send + Sync + 'static,
    M: TryClone + IdMapAssignHead + Persist + Send + Sync + 'static,
    P: Open<OpenTarget = Self> + Send + Sync + 'static,
    S: TryClone + Persist + Send + Sync + 'static,
{
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
        if master_names
            .iter()
            .all(|n| is_ok_some(self.map.vertex_id_with_max_group(n, Group::MASTER)))
            && non_master_names
                .iter()
                .all(|n| self.map.contains_vertex_name(n).unwrap_or(false))
        {
            return Ok(());
        }

        // Take lock.
        //
        // Reload meta. This drops in-memory changes, which is fine because we have
        // checked there are no in-memory changes at the beginning.
        let locked = self.state.prepare_filesystem_sync()?;
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
        dag.sync()?;
        locked.sync()?;

        self.invalidate_snapshot();
        Ok(())
    }

    /// Write in-memory DAG to disk. This will also pick up changes to
    /// the DAG by other processes.
    fn flush(&mut self, master_heads: &[VertexName]) -> Result<()> {
        // Sanity check.
        for head in master_heads.iter() {
            if !self.map.contains_vertex_name(head)? {
                return head.not_found();
            }
        }

        // Dump the pending DAG to memory so we can re-assign numbers.
        //
        // PERF: There could be a fast path that does not re-assign numbers.
        // But in practice we might always want to re-assign master commits.
        let snapshot = self.try_snapshot()?;
        let parents = {
            let snapshot = snapshot.clone();
            move |name| snapshot.parent_names(name)
        };
        let non_master_heads = &snapshot.pending_heads;

        let mut new_name_dag = self.path.open()?;
        let seg_size = self.dag.get_new_segment_size();
        new_name_dag.dag.set_new_segment_size(seg_size);
        new_name_dag.add_heads_and_flush(&parents, master_heads, non_master_heads)?;
        *self = new_name_dag;
        Ok(())
    }
}

impl<IS, M, P, S> DagAddHeads for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: TryClone + IdMapAssignHead,
    P: TryClone,
    S: TryClone,
{
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
        let mut outcome = AssignHeadOutcome::default();
        for head in heads.iter() {
            if !self.map.contains_vertex_name(head)? {
                outcome.merge(self.map.assign_head(head.clone(), &parents, group)?);
                self.pending_heads.push(head.clone());
            }
        }

        // Update segments in the NON_MASTER group.
        #[cfg(test)]
        {
            let parent_ids_func = self.map.build_get_parents_by_id(&parents);
            outcome.verify(&parent_ids_func);
        }

        self.dag
            .build_segments_volatile_from_assign_head_outcome(&outcome)?;

        self.invalidate_snapshot();

        Ok(())
    }
}

impl<IS, M, P, S> AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: TryClone,
    P: TryClone,
    S: TryClone,
{
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
                    snapshot: Default::default(),
                    pending_heads: self.pending_heads.clone(),
                    path: self.path.try_clone()?,
                    state: self.state.try_clone()?,
                };
                let result = Arc::new(cloned);
                *snapshot = Some(result.clone());
                Ok(result)
            }
        }
    }

    fn dag(&self) -> &IdDag<IS> {
        &self.dag
    }

    fn map(&self) -> &M {
        &self.map
    }
}

// Dag operations. Those are just simple wrappers around [`IdDag`].
// See [`IdDag`] for the actual implementations of these algorithms.

/// DAG related read-only algorithms.
impl<IS, M, P, S> DagAlgorithm for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone + Send + Sync + 'static,
    M: TryClone + IdMapAssignHead + Sync + Send + 'static,
    P: TryClone + Sync + Send + 'static,
    S: TryClone + Sync + Send + 'static,
{
    /// Sort a `NameSet` topologically.
    fn sort(&self, set: &NameSet) -> Result<NameSet> {
        if set.hints().contains(Flags::TOPO_DESC)
            && set.hints().is_dag_compatible(self.dag_snapshot()?)
        {
            Ok(set.clone())
        } else {
            let flags = extract_ancestor_flag_if_compatible(set.hints(), self.dag_snapshot()?);
            let mut spans = SpanSet::empty();
            for name in set.iter()? {
                let id = self.map().vertex_id(name?)?;
                spans.push(id);
            }
            let result = NameSet::from_spans_dag(spans, self)?;
            result.hints().add_flags(flags);
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
        let result = NameSet::from_spans_dag(spans, self)?;
        result.hints().add_flags(Flags::FULL);
        Ok(result)
    }

    /// Calculates all ancestors reachable from any name from the given set.
    fn ancestors(&self, set: NameSet) -> Result<NameSet> {
        if set.hints().contains(Flags::ANCESTORS)
            && set.hints().is_dag_compatible(self.dag_snapshot()?)
        {
            return Ok(set);
        }
        let spans = self.to_id_set(&set)?;
        let spans = self.dag().ancestors(spans)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        result.hints().add_flags(Flags::ANCESTORS);
        Ok(result)
    }

    /// Calculates parents of the given set.
    ///
    /// Note: Parent order is not preserved. Use [`NameDag::parent_names`]
    /// to preserve order.
    fn parents(&self, set: NameSet) -> Result<NameSet> {
        // Preserve ANCESTORS flag. If ancestors(x) == x, then ancestors(parents(x)) == parents(x).
        let flags = extract_ancestor_flag_if_compatible(set.hints(), self.dag_snapshot()?);
        let spans = self.dag().parents(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
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
            && set.hints().is_dag_compatible(self.dag_snapshot()?)
        {
            // heads_ancestors is faster.
            return self.heads_ancestors(set);
        }
        let spans = self.dag().heads(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::heads(self, set)?);
        }
        Ok(result)
    }

    /// Calculates children of the given set.
    fn children(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag().children(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        Ok(result)
    }

    /// Calculates roots of the given set.
    fn roots(&self, set: NameSet) -> Result<NameSet> {
        let flags = extract_ancestor_flag_if_compatible(set.hints(), self.dag_snapshot()?);
        let spans = self.dag().roots(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        result.hints().add_flags(flags);
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
        let result = NameSet::from_spans_dag(spans, self)?;
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::gca_all(self, set)?);
        }
        Ok(result)
    }

    /// Calculates all common ancestors of the given set.
    fn common_ancestors(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag().common_ancestors(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        result.hints().add_flags(Flags::ANCESTORS);
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
        let result = NameSet::from_spans_dag(spans, self)?;
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
        let result = NameSet::from_spans_dag(spans, self)?;
        Ok(result)
    }

    /// Calculates the descendants of the given set.
    fn descendants(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag().descendants(self.to_id_set(&set)?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        Ok(result)
    }

    /// Get a snapshot of the current graph.
    fn dag_snapshot(&self) -> Result<Arc<dyn DagAlgorithm + Send + Sync>> {
        Ok(self.try_snapshot()? as Arc<dyn DagAlgorithm + Send + Sync>)
    }
}

/// Extract the ANCESTORS flag if the set with the `hints` is bound to a
/// compatible DAG.
fn extract_ancestor_flag_if_compatible(
    hints: &Hints,
    dag: Arc<dyn DagAlgorithm + Send + Sync>,
) -> Flags {
    if hints.is_dag_compatible(dag) {
        hints.flags() & Flags::ANCESTORS
    } else {
        Flags::empty()
    }
}

delegate! {
    PrefixLookup {
        impl<I, M: PrefixLookup, P, S> PrefixLookup for AbstractNameDag<I, M, P, S>
    } => self.map
}
delegate! {
    IdConvert {
        impl<I, M: IdConvert, P, S> IdConvert for AbstractNameDag<I, M, P, S>
    } => self.map
}

/// Export non-master DAG as parent_names_func on HashMap.
///
/// This can be expensive. It is expected to be either called infrequently,
/// or called with a small amount of data. For example, bounded amount of
/// non-master commits.
fn non_master_parent_names<M, S>(
    map: &Locked<M>,
    dag: &Locked<IdDag<S>>,
) -> Result<HashMap<VertexName, Vec<VertexName>>>
where
    M: IdMapAssignHead + Persist,
    S: IdDagStore + Persist,
{
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
pub fn rebuild_non_master<M, S>(map: &mut Locked<M>, dag: &mut Locked<IdDag<S>>) -> Result<()>
where
    M: IdMapAssignHead + Persist,
    S: IdDagStore + Persist,
{
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
pub fn build<F, IS, M>(
    map: &mut Locked<M>,
    dag: &mut Locked<IdDag<IS>>,
    parent_names_func: F,
    master_heads: &[VertexName],
    non_master_heads: &[VertexName],
) -> Result<()>
where
    F: Fn(VertexName) -> Result<Vec<VertexName>>,
    IS: IdDagStore + Persist,
    M: IdMapAssignHead + Persist,
{
    // Update IdMap.
    let mut outcome = AssignHeadOutcome::default();
    for (nodes, group) in [
        (master_heads, Group::MASTER),
        (non_master_heads, Group::NON_MASTER),
    ]
    .iter()
    {
        for node in nodes.iter() {
            outcome.merge(map.assign_head(node.clone(), &parent_names_func, *group)?);
        }
    }

    // Update segments.
    {
        #[cfg(test)]
        {
            let parent_ids_func = map.build_get_parents_by_id(&parent_names_func);
            outcome.verify(&parent_ids_func);
        }

        dag.build_segments_volatile_from_assign_head_outcome(&outcome)?;
    }

    // Rebuild non-master ids and segments.
    if map.need_rebuild_non_master() {
        rebuild_non_master(map, dag)?;
    }

    Ok(())
}

fn is_ok_some<T>(value: Result<Option<T>>) -> bool {
    match value {
        Ok(Some(_)) => true,
        _ => false,
    }
}

/// IdMap + IdDag backend for DagAlgorithm.
pub trait NameDagStorage {
    type IdDagStore: IdDagStore;
    type IdMap: IdConvert;

    /// The IdDag storage.
    fn dag(&self) -> &IdDag<Self::IdDagStore>;

    /// The IdMap storage.
    fn map(&self) -> &Self::IdMap;

    /// (Relatively cheaply) clone the dag.
    fn storage_dag_snapshot(&self) -> Result<Arc<dyn DagAlgorithm + Send + Sync>>;
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
    fn storage_dag_snapshot(&self) -> Result<Arc<dyn DagAlgorithm + Send + Sync>> {
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
    fn storage_dag_snapshot(&self) -> Result<Arc<dyn DagAlgorithm + Send + Sync>> {
        Ok(self.try_snapshot()? as Arc<dyn DagAlgorithm + Send + Sync>)
    }
}

impl<IS, M, P, S> IdMapSnapshot for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone + Send + Sync + 'static,
    M: TryClone + IdMapAssignHead + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Send + Sync + 'static,
{
    fn id_map_snapshot(&self) -> Result<Arc<dyn IdConvert + Send + Sync>> {
        Ok(self.try_snapshot()? as Arc<dyn IdConvert + Send + Sync>)
    }
}

impl<IS, M, P, S> fmt::Debug for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    M: IdConvert,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        debug(&self.dag, &self.map, f)
    }
}

fn debug<S: IdDagStore>(
    iddag: &IdDag<S>,
    idmap: &dyn IdConvert,
    f: &mut fmt::Formatter,
) -> fmt::Result {
    // Show Id, with optional hash.
    let show = |id: Id| DebugId {
        id,
        name: idmap.vertex_name(id).ok(),
    };
    let show_flags = |flags: SegmentFlags| -> String {
        let mut result = Vec::new();
        if flags.contains(SegmentFlags::HAS_ROOT) {
            result.push("Root");
        }
        if flags.contains(SegmentFlags::ONLY_HEAD) {
            result.push("OnlyHead");
        }
        result.join(" ")
    };

    if let Ok(max_level) = iddag.max_level() {
        writeln!(f, "Max Level: {}", max_level)?;
        for lv in (0..=max_level).rev() {
            writeln!(f, " Level {}", lv)?;
            for group in Group::ALL.iter().cloned() {
                writeln!(f, "  {}:", group)?;
                if let Ok(id) = iddag.next_free_id(0, group) {
                    writeln!(f, "   Next Free Id: {}", id)?;
                }
                if let Ok(segments) = iddag.next_segments(group.min_id(), lv) {
                    writeln!(f, "   Segments: {}", segments.len())?;
                    for segment in segments.into_iter().rev() {
                        if let (Ok(span), Ok(parents), Ok(flags)) =
                            (segment.span(), segment.parents(), segment.flags())
                        {
                            write!(
                                f,
                                "    {:.12?} : {:.12?} {:.12?}",
                                show(span.low),
                                show(span.high),
                                parents.into_iter().map(show).collect::<Vec<_>>(),
                            )?;
                            let flags = show_flags(flags);
                            if !flags.is_empty() {
                                write!(f, " {}", flags)?;
                            }
                            writeln!(f)?;
                        }
                    }
                }
            }
        }
    }

    Ok(())
}

struct DebugId {
    id: Id,
    name: Option<VertexName>,
}

impl fmt::Debug for DebugId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(name) = &self.name {
            fmt::Debug::fmt(&name, f)?;
            f.write_str("+")?;
        }
        write!(f, "{:?}", self.id)?;
        Ok(())
    }
}
