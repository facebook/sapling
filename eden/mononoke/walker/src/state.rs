/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{EdgeType, Node, NodeData, NodeType, WrappedPath};
use crate::walk::{expand_checked_nodes, EmptyRoute, OutgoingEdge, VisitOne, WalkVisitor};

use ahash::RandomState;
use array_init::array_init;
use context::CoreContext;
use dashmap::{mapref::one::Ref, DashMap};
use mercurial_types::{HgChangesetId, HgFileNodeId, HgManifestId};
use mononoke_types::{ChangesetId, ContentId, FsnodeId, MPathHash};
use phases::Phase;
use std::{
    cmp,
    collections::HashSet,
    fmt,
    hash::Hash,
    marker::PhantomData,
    ops::Add,
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
};
use strum::EnumCount;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct StepStats {
    pub error_count: usize,
    pub num_expanded_new: usize,
    pub visited_of_type: usize,
}

impl Add<StepStats> for StepStats {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            error_count: self.error_count + other.error_count,
            num_expanded_new: self.num_expanded_new + other.num_expanded_new,
            visited_of_type: cmp::max(self.visited_of_type, other.visited_of_type),
        }
    }
}

// So we could change the type later without too much code churn
type InternId = u32;

// Common trait for the interned ids
trait Interned: Clone + Copy + Eq + Hash + PartialEq {
    fn new(id: InternId) -> Self;
}

#[derive(Eq, Hash, PartialEq)]
struct InternedId<K> {
    id: InternId,
    _phantom: PhantomData<K>,
}

// Can't auto-derive as dont want to make K Copy
impl<K> Copy for InternedId<K> {}
impl<K> Clone for InternedId<K> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            _phantom: PhantomData,
        }
    }
}

impl<K> Interned for InternedId<K>
where
    K: Eq + Hash + PartialEq,
{
    fn new(id: InternId) -> Self {
        Self {
            id,
            _phantom: PhantomData,
        }
    }
}

struct InternMap<K, V> {
    interned: DashMap<K, V, RandomState>,
    next_id: AtomicU32,
}

impl<K, V> InternMap<K, V>
where
    K: Eq + Hash + Copy + fmt::Debug,
    V: Interned,
{
    fn with_hasher(fac: RandomState) -> Self {
        Self {
            interned: DashMap::with_hasher(fac),
            next_id: AtomicU32::new(0),
        }
    }

    // Intern the key if not already present, returns interned value.
    //
    // From `DashMap::entry()` documentation:
    // **Locking behaviour:** May deadlock if called when holding any sort of reference into the map.
    fn interned(&self, k: &K) -> V {
        // First try the read lock for the interned id, making sure we give up the read guard
        let id: Option<V> = self.interned.get(k).map(|id| *id);
        // Read guard released, escalated to write lock if necessary
        id.unwrap_or_else(|| {
            *self
                .interned
                .entry(*k)
                .or_insert_with(|| {
                    let id = self.next_id.fetch_add(1, Ordering::Release);
                    if id == InternId::MAX {
                        panic!("Intern counter wrapped around for {:?}", k);
                    }
                    V::new(id)
                })
                .value()
        })
    }

    // Get a immutable reference to an entry in the map
    //
    // From `DashMap::get()` documentation:
    // **Locking behaviour:** May deadlock if called when holding a mutable reference into the map.
    fn get(&self, k: &K) -> Option<Ref<K, V, RandomState>> {
        self.interned.get(k)
    }
}

type StateMap<T> = DashMap<T, (), RandomState>;

pub struct WalkState {
    // Params
    include_node_types: HashSet<NodeType>,
    include_edge_types: HashSet<EdgeType>,
    always_emit_edge_types: HashSet<EdgeType>,
    // Interning
    bcs_ids: InternMap<ChangesetId, InternedId<ChangesetId>>,
    hg_cs_ids: InternMap<HgChangesetId, InternedId<HgChangesetId>>,
    hg_filenode_ids: InternMap<HgFileNodeId, InternedId<HgFileNodeId>>,
    mpath_hashs: InternMap<Option<MPathHash>, InternedId<Option<MPathHash>>>,
    fsnode_ids: InternMap<FsnodeId, InternedId<FsnodeId>>,
    hg_manifest_ids: InternMap<HgManifestId, InternedId<HgManifestId>>,
    // State
    visited_bcs: StateMap<InternedId<ChangesetId>>,
    visited_bcs_mapping: StateMap<InternedId<ChangesetId>>,
    visited_bcs_phase: StateMap<InternedId<ChangesetId>>,
    visited_file: StateMap<ContentId>,
    visited_hg_cs: StateMap<InternedId<HgChangesetId>>,
    visited_hg_cs_mapping: StateMap<InternedId<HgChangesetId>>,
    visited_hg_file_envelope: StateMap<InternedId<HgFileNodeId>>,
    visited_hg_filenode: StateMap<(InternedId<Option<MPathHash>>, InternedId<HgFileNodeId>)>,
    visited_hg_manifest: StateMap<(InternedId<Option<MPathHash>>, InternedId<HgManifestId>)>,
    visited_fsnode: StateMap<(InternedId<Option<MPathHash>>, InternedId<FsnodeId>)>,
    visit_count: [AtomicUsize; NodeType::COUNT],
}

impl WalkState {
    pub fn new(
        include_node_types: HashSet<NodeType>,
        include_edge_types: HashSet<EdgeType>,
        always_emit_edge_types: HashSet<EdgeType>,
    ) -> Self {
        let fac = RandomState::default();
        Self {
            // Params
            include_node_types,
            include_edge_types,
            always_emit_edge_types,
            // Interning
            bcs_ids: InternMap::with_hasher(fac.clone()),
            hg_cs_ids: InternMap::with_hasher(fac.clone()),
            hg_filenode_ids: InternMap::with_hasher(fac.clone()),
            mpath_hashs: InternMap::with_hasher(fac.clone()),
            fsnode_ids: InternMap::with_hasher(fac.clone()),
            hg_manifest_ids: InternMap::with_hasher(fac.clone()),
            // State
            visited_bcs: StateMap::with_hasher(fac.clone()),
            visited_bcs_mapping: StateMap::with_hasher(fac.clone()),
            visited_bcs_phase: StateMap::with_hasher(fac.clone()),
            visited_file: StateMap::with_hasher(fac.clone()),
            visited_hg_cs: StateMap::with_hasher(fac.clone()),
            visited_hg_cs_mapping: StateMap::with_hasher(fac.clone()),
            visited_hg_file_envelope: StateMap::with_hasher(fac.clone()),
            visited_hg_filenode: StateMap::with_hasher(fac.clone()),
            visited_hg_manifest: StateMap::with_hasher(fac.clone()),
            visited_fsnode: StateMap::with_hasher(fac),
            visit_count: array_init(|_i| AtomicUsize::new(0)),
        }
    }

    fn record<K>(&self, visited: &StateMap<K>, k: &K) -> bool
    where
        K: Eq + Hash + Copy,
    {
        if visited.contains_key(k) {
            false
        } else {
            visited.insert(*k, ()).is_none()
        }
    }

    /// If the state did not have this value present, true is returned.
    fn record_with_path<K>(
        &self,
        visited_with_path: &StateMap<(InternedId<Option<MPathHash>>, K)>,
        k: (&WrappedPath, &K),
    ) -> bool
    where
        K: Eq + Hash + Copy,
    {
        let (path, id) = k;
        let path = self.mpath_hashs.interned(&path.get_path_hash().cloned());
        let key = (path, *id);
        if visited_with_path.contains_key(&key) {
            false
        } else {
            visited_with_path.insert(key, ()).is_none()
        }
    }

    fn record_resolved_visit(&self, resolved: &OutgoingEdge, node_data: Option<&NodeData>) {
        match (&resolved.target, node_data) {
            (
                Node::BonsaiPhaseMapping(bcs_id),
                Some(NodeData::BonsaiPhaseMapping(Some(Phase::Public))),
            ) => {
                // Only retain visit if already public, otherwise it could mutate between walks.
                self.record(&self.visited_bcs_phase, &self.bcs_ids.interned(bcs_id));
            }
            (Node::BonsaiHgMapping(bcs_id), Some(NodeData::BonsaiHgMapping(Some(_)))) => {
                self.record(&self.visited_bcs_mapping, &self.bcs_ids.interned(bcs_id));
            }
            _ => {}
        }
    }

    fn retain_edge(&self, outgoing_edge: &OutgoingEdge) -> bool {
        // Retain if a root, or if selected
        outgoing_edge.label.incoming_type().is_none()
            || (self
                .include_node_types
                .contains(&outgoing_edge.target.get_type())
                && self.include_edge_types.contains(&outgoing_edge.label))
    }

    fn get_visit_count(&self, t: &NodeType) -> usize {
        self.visit_count[*t as usize].load(Ordering::Acquire)
    }
}

impl VisitOne for WalkState {
    /// If the set did not have this value present, true is returned.
    fn needs_visit(&self, outgoing: &OutgoingEdge) -> bool {
        let target_node: &Node = &outgoing.target;
        let k = target_node.get_type();
        self.visit_count[k as usize].fetch_add(1, Ordering::Release);

        match &target_node {
            Node::BonsaiChangeset(bcs_id) => {
                self.record(&self.visited_bcs, &self.bcs_ids.interned(bcs_id))
            }
            // TODO - measure if worth tracking - the mapping is cachelib enabled.
            Node::BonsaiHgMapping(bcs_id) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    // Does not insert, see record_resolved_visit
                    !self.visited_bcs_mapping.contains_key(&id)
                } else {
                    true
                }
            }
            Node::BonsaiPhaseMapping(bcs_id) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    // Does not insert, as can only prune visits once data resolved, see record_resolved_visit
                    !self.visited_bcs_phase.contains_key(&id)
                } else {
                    true
                }
            }
            Node::HgBonsaiMapping(hg_cs_id) => self.record(
                &self.visited_hg_cs_mapping,
                &self.hg_cs_ids.interned(hg_cs_id),
            ),
            Node::HgChangeset(hg_cs_id) => {
                self.record(&self.visited_hg_cs, &self.hg_cs_ids.interned(hg_cs_id))
            }
            Node::HgManifest((p, id)) => self.record_with_path(
                &self.visited_hg_manifest,
                (p, &self.hg_manifest_ids.interned(id)),
            ),
            Node::HgFileNode((p, id)) => self.record_with_path(
                &self.visited_hg_filenode,
                (p, &self.hg_filenode_ids.interned(id)),
            ),
            Node::HgFileEnvelope(id) => self.record(
                &self.visited_hg_file_envelope,
                &self.hg_filenode_ids.interned(id),
            ),
            Node::FileContent(content_id) => self.record(&self.visited_file, content_id),
            Node::Fsnode((p, id)) => {
                self.record_with_path(&self.visited_fsnode, (p, &self.fsnode_ids.interned(id)))
            }
            _ => true,
        }
    }
}

impl WalkVisitor<(Node, Option<NodeData>, Option<StepStats>), EmptyRoute> for WalkState {
    fn start_step(
        &self,
        ctx: CoreContext,
        _route: Option<&EmptyRoute>,
        _step: &OutgoingEdge,
    ) -> CoreContext {
        ctx
    }

    fn visit(
        &self,
        _ctx: &CoreContext,
        resolved: OutgoingEdge,
        node_data: Option<NodeData>,
        route: Option<EmptyRoute>,
        mut outgoing: Vec<OutgoingEdge>,
    ) -> (
        (Node, Option<NodeData>, Option<StepStats>),
        EmptyRoute,
        Vec<OutgoingEdge>,
    ) {
        if route.is_none() || !self.always_emit_edge_types.is_empty() {
            outgoing.retain(|e| {
                if e.label.incoming_type().is_none() {
                    // Make sure stats are updated for root nodes
                    self.needs_visit(&e);
                    true
                } else {
                    // Check the always emit edges, outer visitor has now processed them.
                    self.retain_edge(e)
                        && (!self.always_emit_edge_types.contains(&e.label) || self.needs_visit(&e))
                }
            });
        }

        let num_outgoing = outgoing.len();
        expand_checked_nodes(&mut outgoing);

        // Make sure we don't expand to types of node and edge not wanted
        if num_outgoing != outgoing.len() {
            outgoing.retain(|e| self.retain_edge(e));
        }

        self.record_resolved_visit(&resolved, node_data.as_ref());

        // Stats
        let num_expanded_new = outgoing.len();
        let node = resolved.target;

        let (error_count, node_data) = match node_data {
            Some(NodeData::ErrorAsData(_key)) => (1, None),
            Some(d) => (0, Some(d)),
            None => (0, None),
        };
        let stats = StepStats {
            error_count,
            num_expanded_new,
            visited_of_type: self.get_visit_count(&node.get_type()),
        };

        ((node, node_data, Some(stats)), EmptyRoute {}, outgoing)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::size_of;

    #[test]
    fn test_interned_size() {
        // InternedId size is important as we have a lot of them, so test in case it changes
        assert_eq!(4, size_of::<InternedId<ChangesetId>>());
    }
}
