/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::detail::graph::EdgeType;
use crate::detail::graph::Node;
use crate::detail::graph::NodeData;
use crate::detail::graph::NodeType;
use crate::detail::graph::UnodeFlags;
use crate::detail::graph::WrappedPath;
use crate::detail::graph::WrappedPathHash;
use crate::detail::log;
use crate::detail::progress::sort_by_string;
use crate::detail::walk::expand_checked_nodes;
use crate::detail::walk::EmptyRoute;
use crate::detail::walk::OutgoingEdge;
use crate::detail::walk::TailingWalkVisitor;
use crate::detail::walk::VisitOne;
use crate::detail::walk::WalkVisitor;

use ahash::RandomState;
use anyhow::bail;
use anyhow::Error;
use array_init::array_init;
use async_trait::async_trait;
use bonsai_hg_mapping::BonsaiHgMapping;
use bonsai_hg_mapping::BonsaiHgMappingEntry;
use bulkops::Direction;
use context::CoreContext;
use dashmap::mapref::one::Ref;
use dashmap::DashMap;
use futures::future::TryFutureExt;
use itertools::Itertools;
use mercurial_types::HgChangesetId;
use mercurial_types::HgFileNodeId;
use mercurial_types::HgManifestId;
use mononoke_types::ChangesetId;
use mononoke_types::ContentId;
use mononoke_types::DeletedManifestV2Id;
use mononoke_types::FastlogBatchId;
use mononoke_types::FileUnodeId;
use mononoke_types::FsnodeId;
use mononoke_types::ManifestUnodeId;
use mononoke_types::SkeletonManifestId;
use phases::Phase;
use phases::Phases;
use slog::info;
use slog::Logger;
use std::cmp;
use std::collections::HashMap;
use std::collections::HashSet;
use std::fmt;
use std::hash::Hash;
use std::marker::PhantomData;
use std::ops::Add;
use std::sync::atomic::AtomicU32;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use strum::EnumCount;
use strum_macros::EnumIter;
use strum_macros::EnumString;
use strum_macros::EnumVariantNames;

#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct StepStats {
    pub error_count: usize,
    pub missing_count: usize,
    pub hash_validation_failure_count: usize,
    pub num_expanded_new: usize,
    pub visited_of_type: usize,
}

impl Add<StepStats> for StepStats {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self {
            error_count: self.error_count + other.error_count,
            missing_count: self.missing_count + other.missing_count,
            hash_validation_failure_count: self.hash_validation_failure_count
                + other.hash_validation_failure_count,
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

    fn clear(&self) {
        self.next_id.store(1, Ordering::SeqCst);
        self.interned.clear()
    }
}

type ValueMap<K, V> = DashMap<K, V, RandomState>;

type StateMap<K> = DashMap<K, (), RandomState>;
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct UnodeInterned<T> {
    id: InternedId<T>,
    flags: UnodeFlags,
}

pub struct WalkState {
    // Params
    include_node_types: HashSet<NodeType>,
    include_edge_types: HashSet<EdgeType>,
    always_emit_edge_types: HashSet<EdgeType>,
    enable_derive: bool,
    chunk_direction: Option<Direction>,
    // Interning
    bcs_ids: InternMap<ChangesetId, InternedId<ChangesetId>>,
    hg_cs_ids: InternMap<HgChangesetId, InternedId<HgChangesetId>>,
    hg_filenode_ids: InternMap<HgFileNodeId, InternedId<HgFileNodeId>>,
    path_hashes: InternMap<WrappedPathHash, InternedId<WrappedPathHash>>,
    hg_manifest_ids: InternMap<HgManifestId, InternedId<HgManifestId>>,
    unode_file_ids: InternMap<FileUnodeId, InternedId<FileUnodeId>>,
    unode_manifest_ids: InternMap<ManifestUnodeId, InternedId<ManifestUnodeId>>,
    // State
    chunk_bcs: StateMap<InternedId<ChangesetId>>,
    deferred_bcs: ValueMap<InternedId<ChangesetId>, HashSet<OutgoingEdge>>,
    bcs_to_hg: ValueMap<InternedId<ChangesetId>, HgChangesetId>,
    hg_to_bcs: ValueMap<InternedId<HgChangesetId>, ChangesetId>,
    visited_bcs: StateMap<InternedId<ChangesetId>>,
    visited_bcs_mapping: StateMap<InternedId<ChangesetId>>,
    public_not_visited: StateMap<InternedId<ChangesetId>>,
    visited_bcs_phase: StateMap<InternedId<ChangesetId>>,
    visited_file: StateMap<ContentId>,
    visited_hg_cs: StateMap<InternedId<HgChangesetId>>,
    visited_hg_cs_mapping: StateMap<InternedId<HgChangesetId>>,
    visited_hg_cs_via_bonsai: StateMap<InternedId<HgChangesetId>>,
    visited_hg_file_envelope: StateMap<InternedId<HgFileNodeId>>,
    visited_hg_filenode: StateMap<(InternedId<WrappedPathHash>, InternedId<HgFileNodeId>)>,
    visited_hg_manifest_filenode: StateMap<(InternedId<WrappedPathHash>, InternedId<HgFileNodeId>)>,
    visited_hg_manifest: StateMap<(InternedId<WrappedPathHash>, InternedId<HgManifestId>)>,
    // Derived
    visited_blame: StateMap<InternedId<FileUnodeId>>,
    visited_changeset_info: StateMap<InternedId<ChangesetId>>,
    visited_changeset_info_mapping: StateMap<InternedId<ChangesetId>>,
    visited_deleted_manifest_v2: StateMap<DeletedManifestV2Id>,
    visited_deleted_manifest_v2_mapping: StateMap<InternedId<ChangesetId>>,
    visited_fastlog_batch: StateMap<FastlogBatchId>,
    visited_fastlog_dir: StateMap<InternedId<ManifestUnodeId>>,
    visited_fastlog_file: StateMap<InternedId<FileUnodeId>>,
    visited_fsnode: StateMap<FsnodeId>,
    visited_fsnode_mapping: StateMap<InternedId<ChangesetId>>,
    visited_skeleton_manifest: StateMap<SkeletonManifestId>,
    visited_skeleton_manifest_mapping: StateMap<InternedId<ChangesetId>>,
    visited_unode_file: StateMap<UnodeInterned<FileUnodeId>>,
    visited_unode_manifest: StateMap<UnodeInterned<ManifestUnodeId>>,
    visited_unode_mapping: StateMap<InternedId<ChangesetId>>,
    // Count
    visit_count: [AtomicUsize; NodeType::COUNT],
}

impl WalkState {
    pub fn new(
        include_node_types: HashSet<NodeType>,
        include_edge_types: HashSet<EdgeType>,
        always_emit_edge_types: HashSet<EdgeType>,
        enable_derive: bool,
        chunk_direction: Option<Direction>,
    ) -> Self {
        let fac = RandomState::default();
        Self {
            // Params
            include_node_types,
            include_edge_types,
            always_emit_edge_types,
            enable_derive,
            chunk_direction,
            // Interning
            bcs_ids: InternMap::with_hasher(fac.clone()),
            hg_cs_ids: InternMap::with_hasher(fac.clone()),
            hg_filenode_ids: InternMap::with_hasher(fac.clone()),
            path_hashes: InternMap::with_hasher(fac.clone()),
            hg_manifest_ids: InternMap::with_hasher(fac.clone()),
            unode_file_ids: InternMap::with_hasher(fac.clone()),
            unode_manifest_ids: InternMap::with_hasher(fac.clone()),
            // State
            chunk_bcs: StateMap::with_hasher(fac.clone()),
            deferred_bcs: ValueMap::with_hasher(fac.clone()),
            bcs_to_hg: ValueMap::with_hasher(fac.clone()),
            hg_to_bcs: ValueMap::with_hasher(fac.clone()),
            visited_bcs: StateMap::with_hasher(fac.clone()),
            visited_bcs_mapping: StateMap::with_hasher(fac.clone()),
            public_not_visited: StateMap::with_hasher(fac.clone()),
            visited_bcs_phase: StateMap::with_hasher(fac.clone()),
            visited_file: StateMap::with_hasher(fac.clone()),
            visited_hg_cs: StateMap::with_hasher(fac.clone()),
            visited_hg_cs_mapping: StateMap::with_hasher(fac.clone()),
            visited_hg_cs_via_bonsai: StateMap::with_hasher(fac.clone()),
            visited_hg_file_envelope: StateMap::with_hasher(fac.clone()),
            visited_hg_filenode: StateMap::with_hasher(fac.clone()),
            visited_hg_manifest_filenode: StateMap::with_hasher(fac.clone()),
            visited_hg_manifest: StateMap::with_hasher(fac.clone()),
            // Derived
            visited_blame: StateMap::with_hasher(fac.clone()),
            visited_changeset_info: StateMap::with_hasher(fac.clone()),
            visited_changeset_info_mapping: StateMap::with_hasher(fac.clone()),
            visited_deleted_manifest_v2: StateMap::with_hasher(fac.clone()),
            visited_deleted_manifest_v2_mapping: StateMap::with_hasher(fac.clone()),
            visited_fastlog_batch: StateMap::with_hasher(fac.clone()),
            visited_fastlog_dir: StateMap::with_hasher(fac.clone()),
            visited_fastlog_file: StateMap::with_hasher(fac.clone()),
            visited_fsnode: StateMap::with_hasher(fac.clone()),
            visited_fsnode_mapping: StateMap::with_hasher(fac.clone()),
            visited_skeleton_manifest: StateMap::with_hasher(fac.clone()),
            visited_skeleton_manifest_mapping: StateMap::with_hasher(fac.clone()),
            visited_unode_file: StateMap::with_hasher(fac.clone()),
            visited_unode_manifest: StateMap::with_hasher(fac.clone()),
            visited_unode_mapping: StateMap::with_hasher(fac),
            // Count
            visit_count: array_init(|_i| AtomicUsize::new(0)),
        }
    }

    fn record<K>(&self, visited: &StateMap<K>, k: &K) -> bool
    where
        K: Eq + Hash + Clone,
    {
        if visited.contains_key(k) {
            false
        } else {
            visited.insert(k.clone(), ()).is_none()
        }
    }

    fn record_multi<K, V>(&self, multi_map: &ValueMap<K, HashSet<V>>, k: K, v: &V) -> bool
    where
        K: Eq + Hash + Clone,
        V: Eq + Hash + Clone,
    {
        let mut entry = multi_map.entry(k).or_insert_with(HashSet::default);
        let values = entry.value_mut();
        // No insert_with in HashSet, so do it ourselves
        if values.contains(v) {
            false
        } else {
            values.insert(v.clone())
        }
    }

    /// If the state did not have this value present, true is returned.
    fn record_with_path<K>(
        &self,
        visited_with_path: &StateMap<(InternedId<WrappedPathHash>, K)>,
        k: (&WrappedPath, &K),
    ) -> bool
    where
        K: Eq + Hash + Copy,
    {
        let (path, id) = k;
        let path = self.path_hashes.interned(path.get_path_hash());
        let key = (path, *id);
        if visited_with_path.contains_key(&key) {
            false
        } else {
            visited_with_path.insert(key, ()).is_none()
        }
    }

    fn record_resolved_visit(&self, resolved: &OutgoingEdge, node_data: Option<&NodeData>) {
        match (&resolved.target, node_data) {
            // Bonsai
            (Node::PhaseMapping(bcs_id), Some(NodeData::PhaseMapping(Some(Phase::Public)))) => {
                let id = &self.bcs_ids.interned(bcs_id);
                // Only retain visit if already public, otherwise it could mutate between walks.
                self.record(&self.visited_bcs_phase, id);
                // Save some memory, no need to keep an entry in public_not_visited now its in visited_bcs_phase
                self.public_not_visited.remove(id);
            }
            // Hg
            (Node::BonsaiHgMapping(k), Some(_)) => {
                self.record(&self.visited_bcs_mapping, &self.bcs_ids.interned(&k.inner));
            }
            // Derived
            (Node::ChangesetInfoMapping(bcs_id), Some(_)) => {
                self.record(
                    &self.visited_changeset_info_mapping,
                    &self.bcs_ids.interned(bcs_id),
                );
            }
            (Node::DeletedManifestV2Mapping(bcs_id), Some(_)) => {
                self.record(
                    &self.visited_deleted_manifest_v2_mapping,
                    &self.bcs_ids.interned(bcs_id),
                );
            }
            (Node::FsnodeMapping(bcs_id), Some(_)) => {
                self.record(&self.visited_fsnode_mapping, &self.bcs_ids.interned(bcs_id));
            }
            (Node::SkeletonManifestMapping(bcs_id), Some(_)) => {
                self.record(
                    &self.visited_skeleton_manifest_mapping,
                    &self.bcs_ids.interned(bcs_id),
                );
            }
            (Node::UnodeMapping(bcs_id), Some(_)) => {
                self.record(&self.visited_unode_mapping, &self.bcs_ids.interned(bcs_id));
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

    fn chunk_contains(&self, id: InternedId<ChangesetId>) -> bool {
        if self.chunk_bcs.is_empty() {
            true
        } else {
            self.chunk_bcs.contains_key(&id)
        }
    }

    // Clear the the InternedId to X mappings, which also means we must clear all mappings using those mappings.
    // InternedId values will start again from 0,  so we don't want old values to clash.
    //
    // The match clears mappings containing InternedId<Foo> for InternedType::Foo. The mapping was built by
    // looking at the type definition, seeing what other fields reference the InternedId<Foo> provided by the InternMap,
    // and then clearing those too.
    fn clear_interned(&mut self, interned_type: InternedType) {
        match interned_type {
            InternedType::FileUnodeId => {
                self.unode_file_ids.clear();
                self.clear_mapping(NodeType::Blame);
                self.clear_mapping(NodeType::FastlogFile);
                self.clear_mapping(NodeType::UnodeFile);
            }
            InternedType::HgChangesetId => {
                self.hg_cs_ids.clear();
                self.hg_to_bcs.clear();
                self.bcs_to_hg.clear();
                self.clear_mapping(NodeType::HgChangeset);
                self.clear_mapping(NodeType::HgBonsaiMapping);
                self.clear_mapping(NodeType::HgChangesetViaBonsai);
            }
            InternedType::HgFileNodeId => {
                self.hg_filenode_ids.clear();
                self.clear_mapping(NodeType::HgFileEnvelope);
                self.clear_mapping(NodeType::HgFileNode);
            }
            InternedType::HgManifestId => {
                self.hg_manifest_ids.clear();
                self.clear_mapping(NodeType::HgManifest);
            }
            InternedType::ManifestUnodeId => {
                self.unode_manifest_ids.clear();
                self.clear_mapping(NodeType::FastlogDir);
                self.clear_mapping(NodeType::UnodeManifest);
            }
            InternedType::MPathHash => {
                self.path_hashes.clear();
                self.clear_mapping(NodeType::HgFileNode);
                self.clear_mapping(NodeType::HgManifest);
            }
        }
    }

    fn clear_mapping(&mut self, node_type: NodeType) {
        match node_type {
            // Entry points
            NodeType::Root => {}
            NodeType::Bookmark => {}
            NodeType::PublishedBookmarks => {}
            // Bonsai
            NodeType::Changeset => self.visited_bcs.clear(),
            NodeType::BonsaiHgMapping => self.visited_bcs_mapping.clear(),
            NodeType::PhaseMapping => self.visited_bcs_phase.clear(),
            // Hg
            NodeType::HgBonsaiMapping => self.visited_hg_cs_mapping.clear(),
            NodeType::HgChangeset => self.visited_hg_cs.clear(),
            NodeType::HgChangesetViaBonsai => self.visited_hg_cs_via_bonsai.clear(),
            NodeType::HgManifest => self.visited_hg_manifest.clear(),
            NodeType::HgFileNode => self.visited_hg_filenode.clear(),
            NodeType::HgManifestFileNode => self.visited_hg_manifest_filenode.clear(),
            NodeType::HgFileEnvelope => self.visited_hg_file_envelope.clear(),
            // Content
            NodeType::FileContent => self.visited_file.clear(),
            NodeType::FileContentMetadata => {} // reached via expand_checked_nodes
            NodeType::AliasContentMapping => {} // reached via expand_checked_nodes
            // Derived
            NodeType::Blame => self.visited_blame.clear(),
            NodeType::ChangesetInfo => self.visited_changeset_info.clear(),
            NodeType::ChangesetInfoMapping => self.visited_changeset_info_mapping.clear(),
            NodeType::DeletedManifestV2 => self.visited_deleted_manifest_v2.clear(),
            NodeType::DeletedManifestV2Mapping => self.visited_deleted_manifest_v2_mapping.clear(),
            NodeType::FastlogBatch => self.visited_fastlog_batch.clear(),
            NodeType::FastlogDir => self.visited_fastlog_dir.clear(),
            NodeType::FastlogFile => self.visited_fastlog_file.clear(),
            NodeType::Fsnode => self.visited_fsnode.clear(),
            NodeType::FsnodeMapping => self.visited_fsnode_mapping.clear(),
            NodeType::SkeletonManifest => self.visited_skeleton_manifest.clear(),
            NodeType::SkeletonManifestMapping => self.visited_skeleton_manifest_mapping.clear(),
            NodeType::UnodeFile => self.visited_unode_file.clear(),
            NodeType::UnodeManifest => self.visited_unode_manifest.clear(),
            NodeType::UnodeMapping => self.visited_unode_mapping.clear(),
        }
    }

    fn needs_visit_impl(&self, outgoing: &OutgoingEdge, executing_step: bool) -> bool {
        let target_node: &Node = &outgoing.target;
        let k = target_node.get_type();
        if !executing_step {
            self.visit_count[k as usize].fetch_add(1, Ordering::Release);
        }

        // For types handled by record_resolved_visit logic is same when executing or checking a step
        // For types handled by record() and record_with_path, executing_step returns true.
        match (&target_node, executing_step) {
            // Entry points
            (Node::Root(_), _) => true,
            (Node::Bookmark(_), _) => true,
            (Node::PublishedBookmarks(_), _) => true,
            // Bonsai
            (Node::Changeset(_), true) => true,
            (Node::Changeset(k), false) => {
                let id = self.bcs_ids.interned(&k.inner);
                if self.chunk_contains(id) {
                    self.record(&self.visited_bcs, &id)
                } else {
                    if self.chunk_direction == Some(Direction::NewestFirst)
                        && !self.visited_bcs.contains_key(&id)
                    {
                        self.record_multi(&self.deferred_bcs, id, outgoing);
                    }
                    false
                }
            }
            (Node::BonsaiHgMapping(k), _) => {
                if let Some(id) = self.bcs_ids.get(&k.inner) {
                    // Does not insert, see record_resolved_visit
                    !self.visited_bcs_mapping.contains_key(&id)
                } else {
                    true
                }
            }
            (Node::PhaseMapping(bcs_id), _) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    // Does not insert, as can only prune visits once data resolved, see record_resolved_visit
                    !self.visited_bcs_phase.contains_key(&id)
                } else {
                    true
                }
            }
            // Hg
            (Node::HgBonsaiMapping(_), true) => true,
            (Node::HgBonsaiMapping(k), false) => self.record(
                &self.visited_hg_cs_mapping,
                &self.hg_cs_ids.interned(&k.inner),
            ),
            (Node::HgChangeset(_), true) => true,
            (Node::HgChangeset(k), false) => {
                self.record(&self.visited_hg_cs, &self.hg_cs_ids.interned(&k.inner))
            }
            (Node::HgChangesetViaBonsai(_), true) => true,
            (Node::HgChangesetViaBonsai(k), false) => self.record(
                &self.visited_hg_cs_via_bonsai,
                &self.hg_cs_ids.interned(&k.inner),
            ),
            (Node::HgManifest(_), true) => true,
            (Node::HgManifest(k), false) => self.record_with_path(
                &self.visited_hg_manifest,
                (&k.path, &self.hg_manifest_ids.interned(&k.id)),
            ),
            (Node::HgFileNode(_), true) => true,
            (Node::HgFileNode(k), false) => self.record_with_path(
                &self.visited_hg_filenode,
                (&k.path, &self.hg_filenode_ids.interned(&k.id)),
            ),
            (Node::HgManifestFileNode(_), true) => true,
            (Node::HgManifestFileNode(k), false) => self.record_with_path(
                &self.visited_hg_manifest_filenode,
                (&k.path, &self.hg_filenode_ids.interned(&k.id)),
            ),
            (Node::HgFileEnvelope(_), true) => true,
            (Node::HgFileEnvelope(id), false) => self.record(
                &self.visited_hg_file_envelope,
                &self.hg_filenode_ids.interned(id),
            ),
            // Content
            (Node::FileContent(_), true) => true,
            (Node::FileContent(content_id), false) => self.record(&self.visited_file, content_id),
            (Node::FileContentMetadata(_), _) => true, // reached via expand_checked_nodes
            (Node::AliasContentMapping(_), _) => true, // reached via expand_checked_nodes
            // Derived
            (Node::Blame(_), true) => true,
            (Node::Blame(k), false) => self.record(
                &self.visited_blame,
                &self.unode_file_ids.interned(k.as_ref()),
            ),
            (Node::ChangesetInfo(_), true) => true,
            (Node::ChangesetInfo(bcs_id), false) => {
                let id = self.bcs_ids.interned(bcs_id);
                if self.chunk_contains(id) {
                    self.record(&self.visited_changeset_info, &id)
                } else {
                    if self.chunk_direction == Some(Direction::NewestFirst)
                        && !self.visited_changeset_info.contains_key(&id)
                    {
                        self.record_multi(&self.deferred_bcs, id, outgoing);
                    }
                    false
                }
            }
            (Node::ChangesetInfoMapping(bcs_id), _) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    !self.visited_changeset_info_mapping.contains_key(&id) // Does not insert, see record_resolved_visit
                } else {
                    true
                }
            }
            (Node::DeletedManifestV2(_), true) => true,
            (Node::DeletedManifestV2(id), false) => {
                self.record(&self.visited_deleted_manifest_v2, id)
            }
            (Node::DeletedManifestV2Mapping(bcs_id), _) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    !self.visited_deleted_manifest_v2_mapping.contains_key(&id) // Does not insert, see record_resolved_visit
                } else {
                    true
                }
            }
            (Node::FastlogBatch(_), true) => true,
            (Node::FastlogBatch(k), false) => self.record(&self.visited_fastlog_batch, k),
            (Node::FastlogDir(_), true) => true,
            (Node::FastlogDir(k), false) => self.record(
                &self.visited_fastlog_dir,
                &self.unode_manifest_ids.interned(&k.inner),
            ),
            (Node::FastlogFile(_), true) => true,
            (Node::FastlogFile(k), false) => self.record(
                &self.visited_fastlog_file,
                &self.unode_file_ids.interned(&k.inner),
            ),
            (Node::Fsnode(_), true) => true,
            (Node::Fsnode(id), false) => self.record(&self.visited_fsnode, id),
            (Node::FsnodeMapping(bcs_id), _) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    !self.visited_fsnode_mapping.contains_key(&id) // Does not insert, see record_resolved_visit
                } else {
                    true
                }
            }
            (Node::SkeletonManifest(_), true) => true,
            (Node::SkeletonManifest(id), false) => self.record(&self.visited_skeleton_manifest, id),
            (Node::SkeletonManifestMapping(bcs_id), _) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    !self.visited_skeleton_manifest_mapping.contains_key(&id) // Does not insert, see record_resolved_visit
                } else {
                    true
                }
            }
            (Node::UnodeFile(_), true) => true,
            (Node::UnodeFile(k), false) => self.record(
                &self.visited_unode_file,
                &UnodeInterned {
                    id: self.unode_file_ids.interned(&k.inner),
                    flags: k.flags,
                },
            ),
            (Node::UnodeManifest(_), true) => true,
            (Node::UnodeManifest(k), false) => self.record(
                &self.visited_unode_manifest,
                &UnodeInterned {
                    id: self.unode_manifest_ids.interned(&k.inner),
                    flags: k.flags,
                },
            ),
            (Node::UnodeMapping(bcs_id), _) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    !self.visited_unode_mapping.contains_key(&id) // Does not insert, see record_resolved_visit
                } else {
                    true
                }
            }
        }
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    Hash,
    EnumIter,
    EnumString,
    EnumVariantNames
)]
pub enum InternedType {
    // No ChangesetId as that is not flushable as it is used to maintain deferred_bcs
    FileUnodeId,
    HgChangesetId,
    HgFileNodeId,
    HgManifestId,
    ManifestUnodeId,
    MPathHash,
}

#[async_trait]
impl VisitOne for WalkState {
    fn in_chunk(&self, bcs_id: &ChangesetId) -> bool {
        if self.chunk_bcs.is_empty() {
            true
        } else {
            let id = self.bcs_ids.interned(bcs_id);
            self.chunk_bcs.contains_key(&id)
        }
    }

    fn get_hg_from_bonsai(&self, bcs_id: &ChangesetId) -> Option<HgChangesetId> {
        let bcs_int = self.bcs_ids.interned(bcs_id);
        self.bcs_to_hg.get(&bcs_int).map(|v| *v.value())
    }

    fn record_hg_from_bonsai(&self, bcs_id: &ChangesetId, hg_cs_id: HgChangesetId) {
        let bcs_int = self.bcs_ids.interned(bcs_id);
        self.bcs_to_hg.insert(bcs_int, hg_cs_id);
    }

    async fn get_bonsai_from_hg(
        &self,
        ctx: &CoreContext,
        bonsai_hg_mapping: &dyn BonsaiHgMapping,
        hg_cs_id: &HgChangesetId,
    ) -> Result<ChangesetId, Error> {
        let hg_int = self.hg_cs_ids.interned(hg_cs_id);
        let bcs_id = if let Some(bcs_id) = self.hg_to_bcs.get(&hg_int) {
            *bcs_id
        } else {
            let bcs_id = bonsai_hg_mapping
                .get_bonsai_from_hg(ctx, hg_cs_id.clone())
                .await?;
            if let Some(bcs_id) = bcs_id {
                let bcs_int = self.bcs_ids.interned(&bcs_id);
                self.hg_to_bcs.insert(hg_int, bcs_id);
                self.bcs_to_hg.insert(bcs_int, *hg_cs_id);
                bcs_id
            } else {
                bail!("Can't have hg without bonsai for {}", hg_cs_id);
            }
        };
        Ok(bcs_id)
    }

    async fn defer_from_hg(
        &self,
        ctx: &CoreContext,
        bonsai_hg_mapping: &dyn BonsaiHgMapping,
        hg_cs_id: &HgChangesetId,
    ) -> Result<Option<ChangesetId>, Error> {
        if self.chunk_bcs.is_empty() {
            return Ok(None);
        }
        let bcs_id = self
            .get_bonsai_from_hg(ctx, bonsai_hg_mapping, hg_cs_id)
            .await?;
        let id = self.bcs_ids.interned(&bcs_id);
        if self.chunk_bcs.contains_key(&id) {
            Ok(None)
        } else {
            Ok(Some(bcs_id))
        }
    }

    async fn is_public(
        &self,
        ctx: &CoreContext,
        phases_store: &dyn Phases,
        bcs_id: &ChangesetId,
    ) -> Result<bool, Error> {
        // Short circuit if we already know its public
        if let Some(id) = self.bcs_ids.get(bcs_id) {
            if self.visited_bcs_phase.contains_key(&id) || self.public_not_visited.contains_key(&id)
            {
                return Ok(true);
            }
        }

        let public_not_visited = &self.public_not_visited;
        let id = self.bcs_ids.interned(bcs_id);

        let is_public = phases_store
            .get_public(
                ctx,
                vec![*bcs_id],
                !self.enable_derive, /* emphemeral_derive */
            )
            .map_ok(move |public| public.contains(bcs_id))
            .await?;

        // Only record visit in public_not_visited if it is public, as state can't change from that point
        // NB, this puts it in public_not_visited rather than visited_bcs_phase so that we still emit a Phase
        // entry from the stream
        if is_public {
            public_not_visited.insert(id, ());
        }
        Ok(is_public)
    }

    /// If the set did not have this value present, true is returned.
    fn needs_visit(&self, outgoing: &OutgoingEdge) -> bool {
        self.needs_visit_impl(outgoing, false)
    }
}

impl TailingWalkVisitor for WalkState {
    fn start_chunk(
        &mut self,
        new_chunk_bcs: &HashSet<ChangesetId>,
        mapping_prepop: Vec<BonsaiHgMappingEntry>,
    ) -> Result<HashSet<OutgoingEdge>, Error> {
        // Reset self.chunk_bcs
        let mut chunk_interned = HashSet::new();
        for bcs_id in new_chunk_bcs {
            let i = self.bcs_ids.interned(bcs_id);
            chunk_interned.insert(i);
            self.chunk_bcs.insert(i, ());
        }
        self.chunk_bcs.retain(|k, _v| chunk_interned.contains(k));

        // Check for items that were outside the chunk now being inside
        let mut in_new_chunk = HashSet::new();
        for e in self.deferred_bcs.iter() {
            if !chunk_interned.contains(e.key()) {
                continue;
            }
            in_new_chunk.extend(e.value().iter().cloned());
        }
        self.deferred_bcs
            .retain(|k, _v| !chunk_interned.contains(k));

        for i in mapping_prepop {
            let bcs_int = self.bcs_ids.interned(&i.bcs_id);
            let hg_int = self.hg_cs_ids.interned(&i.hg_cs_id);
            self.bcs_to_hg.insert(bcs_int, i.hg_cs_id);
            self.hg_to_bcs.insert(hg_int, i.bcs_id);
        }

        Ok(in_new_chunk)
    }

    fn clear_state(
        &mut self,
        node_types: &HashSet<NodeType>,
        interned_types: &HashSet<InternedType>,
    ) {
        node_types.iter().for_each(|t| self.clear_mapping(*t));
        interned_types.iter().for_each(|t| self.clear_interned(*t));
    }

    fn end_chunks(&mut self, logger: &Logger, contiguous_bounds: bool) -> Result<(), Error> {
        if !self.deferred_bcs.is_empty() {
            let summary: HashMap<EdgeType, usize> = self
                .deferred_bcs
                .iter()
                .flat_map(|e| e.value().clone())
                .group_by(|e| e.label)
                .into_iter()
                .map(|(key, group)| (key, group.count()))
                .collect();

            let summary_msg: String = sort_by_string(summary.keys())
                .iter()
                .map(|k| {
                    let mut s = k.to_string();
                    s.push(':');
                    s.push_str(&summary.get(k).map_or("".to_string(), |v| v.to_string()));
                    s
                })
                .join(" ");

            // Where we load from checkpoints the chunks may not be contiguous,
            // which means that some deferred edges can be covered in the checkpointed
            // section we are not repeating.
            if contiguous_bounds {
                let mut count = 0;
                bail!(
                    "Unexpected remaining edges to walk {}, sample of remaining: {:?}",
                    summary_msg,
                    self.deferred_bcs
                        .iter()
                        .take_while(|_| {
                            count += 1;
                            count < 50
                        })
                        .map(|e| e.value().clone())
                        .collect::<Vec<_>>()
                );
            } else {
                info!(logger, #log::CHUNKING, "Deferred edge counts by type were: {}", summary_msg);
                // Deferrals are only between chunks, clear if all chunks done.
                self.deferred_bcs.clear();
            }
        }
        Ok(())
    }

    fn num_deferred(&self) -> usize {
        self.deferred_bcs.len()
    }
}

impl WalkVisitor<(Node, Option<NodeData>, Option<StepStats>), EmptyRoute> for WalkState {
    fn start_step(
        &self,
        ctx: CoreContext,
        route: Option<&EmptyRoute>,
        step: &OutgoingEdge,
    ) -> Option<CoreContext> {
        if route.is_none() // is it a root
            || step.label.incoming_type().is_none() // is it from a root?
            || self.always_emit_edge_types.contains(&step.label) // always emit?
            || self.needs_visit_impl(step, true)
        {
            Some(ctx)
        } else {
            None
        }
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
        let queued_roots = if resolved.label.incoming_type().is_none() {
            1
        } else {
            0
        };
        if route.is_none() || !self.always_emit_edge_types.is_empty() {
            outgoing.retain(|e| {
                if e.label.incoming_type().is_none() {
                    // Make sure stats are updated for root nodes
                    self.needs_visit(e);
                    true
                } else {
                    // Check the always emit edges, outer visitor has now processed them.
                    self.retain_edge(e)
                        && (!self.always_emit_edge_types.contains(&e.label) || self.needs_visit(e))
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
        let num_expanded_new = outgoing.len() + queued_roots;
        let node = resolved.target;

        let mut stats = StepStats {
            error_count: 0,
            missing_count: 0,
            hash_validation_failure_count: 0,
            num_expanded_new,
            visited_of_type: self.get_visit_count(&node.get_type()),
        };
        let node_data = match node_data {
            Some(NodeData::ErrorAsData(_key)) => {
                stats.error_count += 1;
                None
            }
            Some(NodeData::MissingAsData(_key)) => {
                stats.missing_count += 1;
                None
            }
            Some(NodeData::HashValidationFailureAsData(_key)) => {
                stats.hash_validation_failure_count += 1;
                None
            }
            Some(d) => Some(d),
            None => None,
        };

        ((node, node_data, Some(stats)), EmptyRoute {}, outgoing)
    }

    fn defer_visit(
        &self,
        bcs_id: &ChangesetId,
        walk_item: &OutgoingEdge,
        _route: Option<EmptyRoute>,
    ) -> Result<((Node, Option<NodeData>, Option<StepStats>), EmptyRoute), Error> {
        let node_data = match self.chunk_direction {
            Some(Direction::NewestFirst) => {
                let i = self.bcs_ids.interned(bcs_id);
                self.record_multi(&self.deferred_bcs, i, walk_item);
                None
            }
            // We'll never visit backward looking edges when running OldestFirst, so don't record them.
            // returning Some for NodeData tells record_resolved_visit that we don't need to visit this node again if we see it.
            Some(Direction::OldestFirst) => Some(NodeData::OutsideChunk),
            None => bail!(
                "Attempt to defer {:?} step {:?} when not chunking",
                bcs_id,
                walk_item
            ),
        };
        Ok(((walk_item.target.clone(), node_data, None), EmptyRoute {}))
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
