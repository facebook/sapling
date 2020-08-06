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
use dashmap::DashMap;
use mercurial_types::{HgChangesetId, HgFileNodeId, HgManifestId};
use mononoke_types::{ChangesetId, ContentId, FsnodeId, MPathHash};
use phases::Phase;
use std::{
    cmp,
    collections::HashSet,
    hash::Hash,
    ops::Add,
    sync::atomic::{AtomicUsize, Ordering},
};

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

type BuildStateHasher = RandomState;
type StateMap<T> = DashMap<T, (), BuildStateHasher>;

#[derive(Debug)]
pub struct WalkState {
    // TODO implement ID interning to u32 or u64 for types in more than one map
    // e.g. ChangesetId, HgChangesetId, HgFileNodeId
    include_node_types: HashSet<NodeType>,
    include_edge_types: HashSet<EdgeType>,
    always_emit_edge_types: HashSet<EdgeType>,
    visited_bcs: StateMap<ChangesetId>,
    visited_bcs_mapping: StateMap<ChangesetId>,
    visited_bcs_phase: StateMap<ChangesetId>,
    visited_file: StateMap<ContentId>,
    visited_hg_cs: StateMap<HgChangesetId>,
    visited_hg_cs_mapping: StateMap<HgChangesetId>,
    visited_hg_file_envelope: StateMap<HgFileNodeId>,
    visited_hg_filenode: StateMap<(Option<MPathHash>, HgFileNodeId)>,
    visited_hg_manifest: StateMap<(Option<MPathHash>, HgManifestId)>,
    visited_fsnode: StateMap<(Option<MPathHash>, FsnodeId)>,
    visit_count: [AtomicUsize; NodeType::MAX_ORDINAL + 1],
}

fn record<K>(visited: &StateMap<K>, k: &K) -> bool
where
    K: Eq + Hash + Copy,
{
    visited.insert(*k, ()).is_none()
}

/// If the state did not have this value present, true is returned.
fn record_with_path<K>(
    visited_with_path: &StateMap<(Option<MPathHash>, K)>,
    k: &(WrappedPath, K),
) -> bool
where
    K: Eq + Hash + Copy,
{
    let (path, id) = k;
    let mpathhash_opt = path.get_path_hash().cloned();
    visited_with_path.insert((mpathhash_opt, *id), ()).is_none()
}

impl WalkState {
    pub fn new(
        include_node_types: HashSet<NodeType>,
        include_edge_types: HashSet<EdgeType>,
        always_emit_edge_types: HashSet<EdgeType>,
    ) -> Self {
        let fac = BuildStateHasher::default();
        Self {
            include_node_types,
            include_edge_types,
            always_emit_edge_types,
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

    fn record_resolved_visit(&self, resolved: &OutgoingEdge, node_data: Option<&NodeData>) {
        match (&resolved.target, node_data) {
            (
                Node::BonsaiPhaseMapping(bcs_id),
                Some(NodeData::BonsaiPhaseMapping(Some(Phase::Public))),
            ) => {
                // Only retain visit if already public, otherwise it could mutate between walks.
                self.visited_bcs_phase.insert(*bcs_id, ());
            }
            (Node::BonsaiHgMapping(bcs_id), Some(NodeData::BonsaiHgMapping(Some(_)))) => {
                self.visited_bcs_mapping.insert(*bcs_id, ());
            }
            _ => (),
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
            Node::BonsaiChangeset(bcs_id) => record(&self.visited_bcs, bcs_id),
            // TODO - measure if worth tracking - the mapping is cachelib enabled.
            Node::BonsaiHgMapping(bcs_id) => {
                // Does not insert, see record_resolved_visit
                !self.visited_bcs_mapping.contains_key(bcs_id)
            }
            Node::BonsaiPhaseMapping(bcs_id) => {
                // Does not insert, as can only prune visits once data resolved, see record_resolved_visit
                !self.visited_bcs_phase.contains_key(bcs_id)
            }
            Node::HgBonsaiMapping(hg_cs_id) => record(&self.visited_hg_cs_mapping, hg_cs_id),
            Node::HgChangeset(hg_cs_id) => record(&self.visited_hg_cs, hg_cs_id),
            Node::HgManifest(k) => record_with_path(&self.visited_hg_manifest, k),
            Node::HgFileNode(k) => record_with_path(&self.visited_hg_filenode, k),
            Node::HgFileEnvelope(id) => record(&self.visited_hg_file_envelope, id),
            Node::FileContent(content_id) => record(&self.visited_file, content_id),
            Node::Fsnode(k) => record_with_path(&self.visited_fsnode, k),
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
