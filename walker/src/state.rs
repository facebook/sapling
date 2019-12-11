/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::graph::{Node, NodeData, NodeType};
use crate::walk::{OutgoingEdge, WalkVisitor};
use chashmap::CHashMap;
use mercurial_types::{HgChangesetId, HgFileNodeId, HgManifestId};
use mononoke_types::{ChangesetId, ContentId, MPath, MPathHash};
use std::{hash::Hash, sync::Arc};

#[derive(Debug)]
struct WalkStateCHashMap {
    // TODO implement ID interning to u32 or u64 for types in more than one map
    // e.g. ChangesetId, HgChangesetId, HgFileNodeId
    visited_bcs: CHashMap<ChangesetId, ()>,
    visited_bcs_mapping: CHashMap<ChangesetId, ()>,
    visited_file: CHashMap<ContentId, ()>,
    visited_hg_cs: CHashMap<HgChangesetId, ()>,
    visited_hg_cs_mapping: CHashMap<HgChangesetId, ()>,
    visited_hg_file_envelope: CHashMap<HgFileNodeId, ()>,
    visited_hg_filenode: CHashMap<(Option<MPathHash>, HgFileNodeId), ()>,
    visited_hg_manifest: CHashMap<(Option<MPathHash>, HgManifestId), ()>,
    visit_count: CHashMap<NodeType, usize>,
}

/// If the state did not have this value present, true is returned.
fn record_with_path<K>(
    visited_with_path: &CHashMap<(Option<MPathHash>, K), ()>,
    k: &(Option<MPath>, K),
) -> bool
where
    K: Eq + Hash + Copy,
{
    let (path, id) = k;
    let mpathhash_opt = path.as_ref().map(|m| m.get_path_hash());
    !visited_with_path.insert((mpathhash_opt, *id), ()).is_some()
}

impl WalkStateCHashMap {
    fn new() -> Self {
        Self {
            visited_bcs: CHashMap::new(),
            visited_bcs_mapping: CHashMap::new(),
            visited_file: CHashMap::new(),
            visited_hg_cs: CHashMap::new(),
            visited_hg_cs_mapping: CHashMap::new(),
            visited_hg_file_envelope: CHashMap::new(),
            visited_hg_filenode: CHashMap::new(),
            visited_hg_manifest: CHashMap::new(),
            visit_count: CHashMap::new(),
        }
    }

    /// If the set did not have this value present, true is returned.
    fn record_outgoing(
        &self,
        _current: Option<(&Node, &NodeData)>,
        outgoing_edge: &OutgoingEdge,
    ) -> bool {
        let dest_node: &Node = &outgoing_edge.dest;
        let k = dest_node.get_type();
        &self.visit_count.upsert(k, || 1, |old| *old += 1);

        match &dest_node {
            Node::BonsaiChangeset(bcs_id) => self.visited_bcs.insert(*bcs_id, ()).is_none(),
            // TODO - measure if worth tracking - the mapping is cachelib enabled.
            Node::BonsaiHgMapping(bcs_id) => self.visited_bcs_mapping.insert(*bcs_id, ()).is_none(),
            Node::HgBonsaiMapping(hg_cs_id) => {
                self.visited_hg_cs_mapping.insert(*hg_cs_id, ()).is_none()
            }
            Node::HgChangeset(hg_cs_id) => self.visited_hg_cs.insert(*hg_cs_id, ()).is_none(),
            Node::HgManifest(k) => record_with_path(&self.visited_hg_manifest, k),
            Node::HgFileNode(k) => record_with_path(&self.visited_hg_filenode, k),
            Node::HgFileEnvelope(id) => self.visited_hg_file_envelope.insert(*id, ()).is_none(),
            Node::FileContent(content_id) => self.visited_file.insert(*content_id, ()).is_none(),
            _ => true,
        }
    }

    fn get_visit_count(&self, t: &NodeType) -> usize {
        self.visit_count.get(t).map(|v| *v).unwrap_or(0)
    }
}

#[derive(Clone, Debug)]
pub struct WalkState {
    inner: Arc<WalkStateCHashMap>,
}

impl WalkState {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(WalkStateCHashMap::new()),
        }
    }
}

impl WalkVisitor for WalkState {
    // This can mutate the internal state.  Returns true if we should visit the node
    fn record_outgoing(
        &self,
        current: Option<(&Node, &NodeData)>,
        outgoing_edge: &OutgoingEdge,
    ) -> bool {
        self.inner.record_outgoing(current, outgoing_edge)
    }

    // How many times has the checker seen this type
    fn get_visit_count(&self, t: &NodeType) -> usize {
        self.inner.get_visit_count(t)
    }
}
