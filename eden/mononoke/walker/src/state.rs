/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::graph::{EdgeType, Node, NodeData, NodeType, UnodeFlags, WrappedPath};
use crate::walk::{expand_checked_nodes, EmptyRoute, OutgoingEdge, VisitOne, WalkVisitor};

use ahash::RandomState;
use anyhow::{bail, Error};
use array_init::array_init;
use async_trait::async_trait;
use bonsai_hg_mapping::BonsaiHgMapping;
use context::CoreContext;
use dashmap::{mapref::one::Ref, DashMap};
use futures::future::TryFutureExt;
use mercurial_types::{HgChangesetId, HgFileNodeId, HgManifestId};
use mononoke_types::{
    ChangesetId, ContentId, DeletedManifestId, FastlogBatchId, FileUnodeId, FsnodeId, MPathHash,
    ManifestUnodeId, RepositoryId, SkeletonManifestId,
};
use phases::{Phase, Phases};
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
use strum_macros::{EnumIter, EnumString, EnumVariantNames};

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
    // Interning
    bcs_ids: InternMap<ChangesetId, InternedId<ChangesetId>>,
    hg_cs_ids: InternMap<HgChangesetId, InternedId<HgChangesetId>>,
    hg_filenode_ids: InternMap<HgFileNodeId, InternedId<HgFileNodeId>>,
    mpath_hashs: InternMap<Option<MPathHash>, InternedId<Option<MPathHash>>>,
    hg_manifest_ids: InternMap<HgManifestId, InternedId<HgManifestId>>,
    unode_file_ids: InternMap<FileUnodeId, InternedId<FileUnodeId>>,
    unode_manifest_ids: InternMap<ManifestUnodeId, InternedId<ManifestUnodeId>>,
    // State
    chunk_bcs: StateMap<InternedId<ChangesetId>>,
    deferred_bcs: ValueMap<InternedId<ChangesetId>, HashSet<OutgoingEdge>>,
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
    visited_hg_filenode: StateMap<(InternedId<Option<MPathHash>>, InternedId<HgFileNodeId>)>,
    visited_hg_manifest: StateMap<(InternedId<Option<MPathHash>>, InternedId<HgManifestId>)>,
    // Derived
    visited_blame: StateMap<InternedId<FileUnodeId>>,
    visited_changeset_info: StateMap<InternedId<ChangesetId>>,
    visited_changeset_info_mapping: StateMap<InternedId<ChangesetId>>,
    visited_deleted_manifest: StateMap<DeletedManifestId>,
    visited_deleted_manifest_mapping: StateMap<InternedId<ChangesetId>>,
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
    ) -> Self {
        let fac = RandomState::default();
        Self {
            // Params
            include_node_types,
            include_edge_types,
            always_emit_edge_types,
            enable_derive,
            // Interning
            bcs_ids: InternMap::with_hasher(fac.clone()),
            hg_cs_ids: InternMap::with_hasher(fac.clone()),
            hg_filenode_ids: InternMap::with_hasher(fac.clone()),
            mpath_hashs: InternMap::with_hasher(fac.clone()),
            hg_manifest_ids: InternMap::with_hasher(fac.clone()),
            unode_file_ids: InternMap::with_hasher(fac.clone()),
            unode_manifest_ids: InternMap::with_hasher(fac.clone()),
            // State
            chunk_bcs: StateMap::with_hasher(fac.clone()),
            deferred_bcs: ValueMap::with_hasher(fac.clone()),
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
            visited_hg_manifest: StateMap::with_hasher(fac.clone()),
            // Derived
            visited_blame: StateMap::with_hasher(fac.clone()),
            visited_changeset_info: StateMap::with_hasher(fac.clone()),
            visited_changeset_info_mapping: StateMap::with_hasher(fac.clone()),
            visited_deleted_manifest: StateMap::with_hasher(fac.clone()),
            visited_deleted_manifest_mapping: StateMap::with_hasher(fac.clone()),
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
            // Bonsai
            (Node::PhaseMapping(bcs_id), Some(NodeData::PhaseMapping(Some(Phase::Public)))) => {
                let id = &self.bcs_ids.interned(bcs_id);
                // Only retain visit if already public, otherwise it could mutate between walks.
                self.record(&self.visited_bcs_phase, &id);
                // Save some memory, no need to keep an entry in public_not_visited now its in visited_bcs_phase
                self.public_not_visited.remove(&id);
            }
            // Hg
            (Node::BonsaiHgMapping(k), Some(NodeData::BonsaiHgMapping(Some(_)))) => {
                self.record(&self.visited_bcs_mapping, &self.bcs_ids.interned(&k.inner));
            }
            // Derived
            (Node::ChangesetInfoMapping(bcs_id), Some(NodeData::ChangesetInfoMapping(Some(_)))) => {
                self.record(
                    &self.visited_changeset_info_mapping,
                    &self.bcs_ids.interned(bcs_id),
                );
            }
            (
                Node::DeletedManifestMapping(bcs_id),
                Some(NodeData::DeletedManifestMapping(Some(_))),
            ) => {
                self.record(
                    &self.visited_deleted_manifest_mapping,
                    &self.bcs_ids.interned(bcs_id),
                );
            }
            (Node::FsnodeMapping(bcs_id), Some(NodeData::FsnodeMapping(Some(_)))) => {
                self.record(&self.visited_fsnode_mapping, &self.bcs_ids.interned(bcs_id));
            }
            (
                Node::SkeletonManifestMapping(bcs_id),
                Some(NodeData::SkeletonManifestMapping(Some(_))),
            ) => {
                self.record(
                    &self.visited_skeleton_manifest_mapping,
                    &self.bcs_ids.interned(bcs_id),
                );
            }
            (Node::UnodeMapping(bcs_id), Some(NodeData::UnodeMapping(Some(_)))) => {
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

    async fn defer_from_hg(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        bonsai_hg_mapping: &dyn BonsaiHgMapping,
        hg_cs_id: &HgChangesetId,
    ) -> Result<Option<ChangesetId>, Error> {
        if self.chunk_bcs.is_empty() {
            return Ok(None);
        }
        let hg_int = self.hg_cs_ids.interned(hg_cs_id);
        let bcs_id = if let Some(bcs_id) = self.hg_to_bcs.get(&hg_int) {
            *bcs_id
        } else {
            let bcs_id = bonsai_hg_mapping
                .get_bonsai_from_hg(ctx, repo_id, hg_cs_id.clone())
                .await?;
            if let Some(bcs_id) = bcs_id {
                self.hg_to_bcs.insert(hg_int, bcs_id);
                bcs_id
            } else {
                bail!("Can't have hg without bonsai for {}", hg_cs_id);
            }
        };
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
                ctx.clone(),
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
        let target_node: &Node = &outgoing.target;
        let k = target_node.get_type();
        self.visit_count[k as usize].fetch_add(1, Ordering::Release);

        match &target_node {
            // Entry points
            Node::Root(_) => true,
            Node::Bookmark(_) => true,
            Node::PublishedBookmarks(_) => true,
            // Bonsai
            Node::Changeset(k) => {
                let id = self.bcs_ids.interned(&k.inner);
                if self.chunk_contains(id) {
                    self.record(&self.visited_bcs, &id)
                } else {
                    if !self.visited_bcs.contains_key(&id) {
                        self.record_multi(&self.deferred_bcs, id, outgoing);
                    }
                    false
                }
            }
            Node::BonsaiHgMapping(k) => {
                if let Some(id) = self.bcs_ids.get(&k.inner) {
                    // Does not insert, see record_resolved_visit
                    !self.visited_bcs_mapping.contains_key(&id)
                } else {
                    true
                }
            }
            Node::PhaseMapping(bcs_id) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    // Does not insert, as can only prune visits once data resolved, see record_resolved_visit
                    !self.visited_bcs_phase.contains_key(&id)
                } else {
                    true
                }
            }
            // Hg
            Node::HgBonsaiMapping(k) => self.record(
                &self.visited_hg_cs_mapping,
                &self.hg_cs_ids.interned(&k.inner),
            ),
            Node::HgChangeset(k) => {
                self.record(&self.visited_hg_cs, &self.hg_cs_ids.interned(&k.inner))
            }
            Node::HgChangesetViaBonsai(k) => self.record(
                &self.visited_hg_cs_via_bonsai,
                &self.hg_cs_ids.interned(&k.inner),
            ),
            Node::HgManifest(k) => self.record_with_path(
                &self.visited_hg_manifest,
                (&k.path, &self.hg_manifest_ids.interned(&k.id)),
            ),
            Node::HgFileNode(k) => self.record_with_path(
                &self.visited_hg_filenode,
                (&k.path, &self.hg_filenode_ids.interned(&k.id)),
            ),
            Node::HgFileEnvelope(id) => self.record(
                &self.visited_hg_file_envelope,
                &self.hg_filenode_ids.interned(id),
            ),
            // Content
            Node::FileContent(content_id) => self.record(&self.visited_file, content_id),
            Node::FileContentMetadata(_) => true, // reached via expand_checked_nodes
            Node::AliasContentMapping(_) => true, // reached via expand_checked_nodes
            // Derived
            Node::Blame(k) => self.record(
                &self.visited_blame,
                &self.unode_file_ids.interned(k.as_ref()),
            ),
            Node::ChangesetInfo(bcs_id) => {
                let id = self.bcs_ids.interned(bcs_id);
                if self.chunk_contains(id) {
                    self.record(&self.visited_changeset_info, &id)
                } else {
                    if !self.visited_changeset_info.contains_key(&id) {
                        self.record_multi(&self.deferred_bcs, id, outgoing);
                    }
                    false
                }
            }
            Node::ChangesetInfoMapping(bcs_id) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    !self.visited_changeset_info_mapping.contains_key(&id) // Does not insert, see record_resolved_visit
                } else {
                    true
                }
            }
            Node::DeletedManifest(id) => self.record(&self.visited_deleted_manifest, &id),
            Node::DeletedManifestMapping(bcs_id) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    !self.visited_deleted_manifest_mapping.contains_key(&id) // Does not insert, see record_resolved_visit
                } else {
                    true
                }
            }
            Node::FastlogBatch(k) => self.record(&self.visited_fastlog_batch, &k),
            Node::FastlogDir(k) => self.record(
                &self.visited_fastlog_dir,
                &self.unode_manifest_ids.interned(&k.inner),
            ),
            Node::FastlogFile(k) => self.record(
                &self.visited_fastlog_file,
                &self.unode_file_ids.interned(&k.inner),
            ),
            Node::Fsnode(id) => self.record(&self.visited_fsnode, &id),
            Node::FsnodeMapping(bcs_id) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    !self.visited_fsnode_mapping.contains_key(&id) // Does not insert, see record_resolved_visit
                } else {
                    true
                }
            }
            Node::SkeletonManifest(id) => self.record(&self.visited_skeleton_manifest, &id),
            Node::SkeletonManifestMapping(bcs_id) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    !self.visited_skeleton_manifest_mapping.contains_key(&id) // Does not insert, see record_resolved_visit
                } else {
                    true
                }
            }
            Node::UnodeFile(k) => self.record(
                &self.visited_unode_file,
                &UnodeInterned {
                    id: self.unode_file_ids.interned(&k.inner),
                    flags: k.flags,
                },
            ),
            Node::UnodeManifest(k) => self.record(
                &self.visited_unode_manifest,
                &UnodeInterned {
                    id: self.unode_manifest_ids.interned(&k.inner),
                    flags: k.flags,
                },
            ),
            Node::UnodeMapping(bcs_id) => {
                if let Some(id) = self.bcs_ids.get(bcs_id) {
                    !self.visited_unode_mapping.contains_key(&id) // Does not insert, see record_resolved_visit
                } else {
                    true
                }
            }
        }
    }
}

impl WalkVisitor<(Node, Option<NodeData>, Option<StepStats>), EmptyRoute> for WalkState {
    fn start_chunk(
        &self,
        new_chunk_bcs: &HashSet<ChangesetId>,
    ) -> Result<HashSet<OutgoingEdge>, Error> {
        // Reset self.chunk_bcs
        let mut chunk_interned = HashSet::new();
        for bcs_id in new_chunk_bcs {
            let i = self.bcs_ids.interned(&bcs_id);
            chunk_interned.insert(i);
            self.chunk_bcs.insert(i, ());
        }
        self.chunk_bcs.retain(|k, _v| chunk_interned.contains(k));

        // Check for items that were outside the chunk now being inside
        let mut in_new_chunk = HashSet::new();
        for e in self.deferred_bcs.iter() {
            if !chunk_interned.contains(&e.key()) {
                continue;
            }
            in_new_chunk.extend(e.value().iter().cloned());
        }
        self.deferred_bcs
            .retain(|k, _v| !chunk_interned.contains(k));

        Ok(in_new_chunk)
    }

    fn end_chunks(&self) -> Result<(), Error> {
        if !self.deferred_bcs.is_empty() {
            bail!(
                "Unexpected remaining edges to walk {:?}",
                self.deferred_bcs
                    .iter()
                    .map(|e| e.value().clone())
                    .collect::<Vec<_>>()
            );
        }
        Ok(())
    }

    fn num_deferred(&self) -> usize {
        self.deferred_bcs.len()
    }

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
        let queued_roots = if resolved.label.incoming_type().is_none() {
            1
        } else {
            0
        };
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
        let num_expanded_new = outgoing.len() + queued_roots;
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

    fn defer_visit(
        &self,
        bcs_id: &ChangesetId,
        walk_item: &OutgoingEdge,
        _route: Option<EmptyRoute>,
    ) -> ((Node, Option<NodeData>, Option<StepStats>), EmptyRoute) {
        let target = walk_item.target.clone();
        let i = self.bcs_ids.interned(bcs_id);
        self.record_multi(&self.deferred_bcs, i, &walk_item);
        ((target, None, None), EmptyRoute {})
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
