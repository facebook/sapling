/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! # dag
//!
//! Combination of IdMap and IdDag.

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::collections::HashSet;
use std::env::var;
use std::fmt;
use std::io;
use std::ops::Deref;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use dag_types::FlatSegment;
use futures::FutureExt;
use futures::StreamExt;
use futures::TryStreamExt;
use futures::future::BoxFuture;
use nonblocking::non_blocking_result;

use crate::Error::NeedSlowPath;
use crate::IdSet;
use crate::IdSpan;
use crate::Level;
use crate::Result;
use crate::VerLink;
use crate::VertexListWithOptions;
use crate::VertexOptions;
use crate::clone::CloneData;
use crate::default_impl;
use crate::errors::DagError;
use crate::errors::NotFoundError;
use crate::errors::bug;
use crate::errors::programming;
use crate::id::Group;
use crate::id::Id;
use crate::id::Vertex;
use crate::iddag::IdDag;
use crate::iddag::IdDagAlgorithm;
use crate::iddagstore::IdDagStore;
use crate::idmap::CoreMemIdMap;
use crate::idmap::IdMapAssignHead;
use crate::idmap::IdMapWrite;
use crate::lifecycle::LifecycleId;
use crate::ops::CheckIntegrity;
use crate::ops::DagAddHeads;
use crate::ops::DagAlgorithm;
use crate::ops::DagExportCloneData;
use crate::ops::DagExportPullData;
use crate::ops::DagImportCloneData;
use crate::ops::DagImportPullData;
use crate::ops::DagPersistent;
use crate::ops::DagStrip;
use crate::ops::IdConvert;
use crate::ops::IdMapSnapshot;
use crate::ops::Open;
use crate::ops::Parents;
use crate::ops::Persist;
use crate::ops::PrefixLookup;
use crate::ops::StorageVersion;
use crate::ops::ToIdSet;
use crate::ops::TryClone;
use crate::protocol;
use crate::protocol::AncestorPath;
use crate::protocol::Process;
use crate::protocol::RemoteIdConvertProtocol;
use crate::protocol::is_remote_protocol_disabled;
use crate::segment::PreparedFlatSegments;
use crate::segment::SegmentFlags;
use crate::set::Set;
use crate::set::hints::Flags;
use crate::set::hints::Hints;
use crate::set::id_static::BasicIterationOrder;
use crate::types_ext::PreparedFlatSegmentsExt;
use crate::utils;

mod builder;
#[cfg(any(test, feature = "indexedlog-backend"))]
mod indexedlog_dag;
mod mem_dag;

pub use builder::DagBuilder;
#[cfg(any(test, feature = "indexedlog-backend"))]
pub use indexedlog_dag::Dag;
#[cfg(any(test, feature = "indexedlog-backend"))]
pub use indexedlog_dag::IndexedLogDagPath;
pub use mem_dag::MemDag;
pub use mem_dag::MemDagPath;

pub struct AbstractDag<I, M, P, S>
where
    I: Send + Sync,
    M: Send + Sync,
    P: Send + Sync,
    S: Send + Sync,
{
    pub(crate) dag: I,
    pub(crate) map: M,

    /// A read-only snapshot of the `Dag`.
    /// Lazily calculated.
    snapshot: RwLock<Option<Arc<Self>>>,

    /// Non-virtual heads added via `add_heads` that are not flushed yet.
    /// They can be flushed by `flush()`.
    pending_heads: VertexListWithOptions,

    /// Path used to open this `Dag`.
    path: P,

    /// Extra state of the `Dag`.
    state: S,

    /// Identity of the dag. Derived from `path`.
    id: String,

    /// `Id`s that are persisted on disk. Used to answer `dirty()`.
    persisted_id_set: IdSet,

    /// Overlay IdMap. Used to store IdMap results resolved using remote
    /// protocols.
    overlay_map: Arc<RwLock<CoreMemIdMap>>,

    /// `Id`s that are allowed in the `overlay_map`. A protection.
    /// The `overlay_map` is shared (Arc) and its ID should not exceed the
    /// existing maximum ID at `map` open time. The IDs from
    /// 0..overlay_map_next_id are considered immutable, but lazy.
    overlay_map_id_set: IdSet,

    /// The source of `overlay_map`s. This avoids absolute Ids, and is
    /// used to flush overlay_map content shall the IdMap change on
    /// disk.
    overlay_map_paths: Arc<Mutex<Vec<(AncestorPath, Vec<Vertex>)>>>,

    /// Defines how to communicate with a remote service.
    /// The actual logic probably involves networking like HTTP etc
    /// and is intended to be implemented outside the `dag` crate.
    remote_protocol: Arc<dyn RemoteIdConvertProtocol>,

    /// If set, clear and insert to the VIRTUAL group after reloading.
    managed_virtual_group: Option<Arc<(Box<dyn Parents>, VertexListWithOptions /* derived */)>>,

    /// A negative cache. Vertexes that are looked up remotely, and the remote
    /// confirmed the vertexes are outside the master group.
    missing_vertexes_confirmed_by_remote: Arc<RwLock<HashSet<Vertex>>>,

    /// Internal stats (for testing and debugging).
    lifecycle_id: LifecycleId,
    pub(crate) internal_stats: DagInternalStats,
}

/// Statistics of dag internals. Useful to check if fast paths are used.
#[derive(Debug, Default)]
pub struct DagInternalStats {
    /// Bumps when sort(set) takes O(set) slow path.
    pub sort_slow_path_count: AtomicUsize,
}

impl<D, M, P, S> AbstractDag<D, M, P, S>
where
    D: Send + Sync,
    M: Send + Sync,
    P: Send + Sync,
    S: Send + Sync,
{
    /// Extract inner states. Useful for advanced use-cases.
    pub fn into_idmap_dag(self) -> (M, D) {
        (self.map, self.dag)
    }

    /// Extract inner states. Useful for advanced use-cases.
    pub fn into_idmap_dag_path_state(self) -> (M, D, P, S) {
        (self.map, self.dag, self.path, self.state)
    }
}

impl<IS, M, P, S> AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: Send + Sync + IdMapWrite + IdMapAssignHead + TryClone + 'static,
    P: Send + Sync + TryClone + 'static,
    S: Send + Sync + TryClone + 'static,
{
    /// Set the content of the VIRTUAL group that survives reloading.
    ///
    /// `items` is a list of vertexes and parents. The vertexes MUST be unique
    /// and not already exist in non-VIRTUAL groups. This assumption is used
    /// as an optimization to avoid remote lookups.
    ///
    /// Existing content of the VIRTUAL group will be cleared before inserting
    /// `items`. So this API feels declarative. As a comparison, `add_heads`
    /// is imperative.
    ///
    /// This function calls `maybe_recreate_virtual_group` immediately to clear
    /// and update contents in the VIRTUAL group. `maybe_recreate_virtual_group`
    /// will be called automatically after graph changing operations:
    /// `add_heads_and_flush`, `strip`, `flush`, `import_pull_data`.
    pub async fn set_managed_virtual_group(
        &mut self,
        items: Option<Vec<(Vertex, Vec<Vertex>)>>,
    ) -> Result<()> {
        tracing::debug!(target: "dag::set_managed_virtual_group", lifecycle_id=?self.lifecycle_id, ?items);
        self.managed_virtual_group = items.map(|items| {
            // Calculate `Parents` and `VertexListWithOptions` so they can be
            // used in `maybe_recreate_virtual_group`.
            let opts = VertexOptions {
                reserve_size: 0,
                desired_group: Group::VIRTUAL,
            };
            let heads: VertexListWithOptions = items
                .iter()
                .map(|(v, _p)| (v.clone(), opts.clone()))
                .collect::<Vec<_>>()
                .into();
            let parents: HashMap<Vertex, Vec<Vertex>> = items.into_iter().collect();
            let parents: Box<dyn Parents> = Box::new(parents);
            Arc::new((parents, heads))
        });
        self.maybe_recreate_virtual_group().await
    }

    /// Clear vertexes in the VIRTUAL group.
    pub(crate) async fn clear_virtual_group(&mut self) -> Result<()> {
        let id_set = self.dag.all_ids_in_groups(&[Group::VIRTUAL])?;
        if !id_set.is_empty() {
            let removed = self.dag.strip(id_set)?;
            for span in removed.iter_span_desc() {
                self.map.remove_range(span.low, span.high).await?;
            }
        }
        Ok(())
    }

    /// If `managed_virtual_group` is set, clear the VIRTUAL group and re-insert
    /// based on `managed_virtual_group`.
    async fn maybe_recreate_virtual_group(&mut self) -> Result<()> {
        if let Some(maintained_virtual_group) = self.managed_virtual_group.as_ref() {
            let maintained_virtual_group = maintained_virtual_group.clone();
            self.clear_virtual_group().await?;
            let parents = &maintained_virtual_group.0;
            let head_opts = &maintained_virtual_group.1;
            // With the assumption (see set_managed_virtual_group docstring) that VIRTUAL group
            // only has unique vertexes, we can pre-populate the negative cache to avoid remote
            // lookup.
            {
                let mut cache = self.missing_vertexes_confirmed_by_remote.write().unwrap();
                for v in head_opts.vertexes() {
                    cache.insert(v);
                }
            }
            // Insert to the VIRTUAL group, using the a precalculated insertion order.
            self.add_heads(parents.as_ref(), head_opts).await?;
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl<IS, M, P, S> DagPersistent for AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore + Persist + StorageVersion,
    IdDag<IS>: TryClone + 'static,
    M: TryClone + IdMapAssignHead + Persist + Send + Sync + 'static,
    P: Open<OpenTarget = Self> + Send + Sync + 'static,
    S: TryClone + StorageVersion + Persist + Send + Sync + 'static,
{
    // See docstring in ops.rs for details.
    async fn add_heads_and_flush(
        &mut self,
        parents: &dyn Parents,
        heads: &VertexListWithOptions,
    ) -> Result<()> {
        if !self.pending_heads.is_empty() {
            return programming(format!(
                "ProgrammingError: add_heads_and_flush called with pending heads ({:?})",
                &self.pending_heads.vertexes(),
            ));
        }
        tracing::debug!(target: "dag::add_heads_and_flush", lifecycle_id=?self.lifecycle_id, ?heads);

        // Clear the VIRTUAL group. Their parents might have changed in incompatible ways.
        self.clear_virtual_group().await?;

        // Take lock.
        //
        // Reload meta and logs. This drops in-memory changes, which is fine because we have
        // checked there are no in-memory changes at the beginning.
        //
        // Also see comments in `DagState::lock()`.
        let old_version = self.state.storage_version();
        let lock = self.state.lock()?;
        let map_lock = self.map.lock()?;
        let dag_lock = self.dag.lock()?;
        self.state.reload(&lock)?;
        let new_version = self.state.storage_version();
        if old_version != new_version {
            self.invalidate_snapshot();
            self.invalidate_missing_vertex_cache();
            self.invalidate_overlay_map()?;
        }

        self.map.reload(&map_lock)?;
        self.dag.reload(&dag_lock)?;

        // Build.
        self.build_with_lock(parents, heads, &map_lock).await?;

        // Write to disk.
        self.map.persist(&map_lock)?;
        self.dag.persist(&dag_lock)?;
        self.state.persist(&lock)?;
        drop(dag_lock);
        drop(map_lock);
        drop(lock);

        self.persisted_id_set = self.dag.all_ids_in_groups(&Group::PERSIST)?;
        self.maybe_recreate_virtual_group().await?;

        debug_assert_eq!(self.dirty().await?.count().await?, 0);

        Ok(())
    }

    /// Write in-memory DAG to disk. This will also pick up changes to
    /// the DAG by other processes.
    ///
    /// This function re-assigns ids for vertexes. That requires the
    /// pending ids and vertexes to be non-lazy. If you're changing
    /// internal structures (ex. dag and map) directly, or introducing
    /// lazy vertexes, then avoid this function. Instead, lock and
    /// flush directly (see `add_heads_and_flush`, `import_clone_data`).
    ///
    /// `heads` specify additional options for special vertexes. This
    /// overrides the `VertexOptions` provided to `add_head`. If `heads`
    /// is empty, then `VertexOptions` provided to `add_head` will be
    /// used.
    async fn flush(&mut self, heads: &VertexListWithOptions) -> Result<()> {
        tracing::debug!(target: "dag::flush", lifecycle_id=?self.lifecycle_id, ?heads);
        // Sanity check.
        for result in self.vertex_id_batch(&heads.vertexes()).await? {
            result?;
        }
        // Previous version of the API requires `master_heads: &[Vertex]`.
        // Warn about possible misuses.
        if heads.vertexes_by_group(Group::MASTER).len() != heads.len() {
            return programming(format!(
                "Dag::flush({:?}) is probably misused (group is not master)",
                heads
            ));
        }

        // Write cached IdMap to disk.
        self.flush_cached_idmap().await?;

        // Constructs a new graph so we can copy pending data from the existing graph.
        let mut new_name_dag: Self = self.path.open()?;

        let parents: &(dyn DagAlgorithm + Send + Sync) = self;
        let non_master_heads: VertexListWithOptions = self.pending_heads.clone();
        new_name_dag.inherit_configurations_from(self);
        let heads = heads.clone().chain(non_master_heads);
        new_name_dag.add_heads_and_flush(&parents, &heads).await?;
        new_name_dag.maybe_recreate_virtual_group().await?;

        *self = new_name_dag;
        Ok(())
    }

    /// Write in-memory IdMap paths to disk so the next time we don't need to
    /// ask remote service for IdMap translation.
    #[tracing::instrument(skip(self))]
    async fn flush_cached_idmap(&self) -> Result<()> {
        // The map might have changed on disk. We cannot use the ids in overlay_map
        // directly. Instead, re-translate the paths.

        // Prepare data to insert. Do not hold Mutex across async yield points.
        let mut to_insert: Vec<(AncestorPath, Vec<Vertex>)> = Vec::new();
        std::mem::swap(&mut to_insert, &mut *self.overlay_map_paths.lock().unwrap());
        if to_insert.is_empty() {
            return Ok(());
        }

        // Lock, reload from disk. Use a new state so the existing dag is not affected.
        tracing::debug!(target: "dag::cache", "flushing cached idmap ({} items)", to_insert.len());
        let mut new: Self = self.path.open()?;
        let lock = new.state.lock()?;
        let map_lock = new.map.lock()?;
        let dag_lock = new.dag.lock()?;
        new.state.reload(&lock)?;
        new.map.reload(&map_lock)?;
        new.dag.reload(&dag_lock)?;
        new.inherit_configurations_from(self);
        std::mem::swap(&mut to_insert, &mut *new.overlay_map_paths.lock().unwrap());
        new.flush_cached_idmap_with_lock(&map_lock).await?;

        new.state.persist(&lock)?;

        Ok(())
    }
}

impl<IS, M, P, S> AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone + 'static,
    M: TryClone + IdConvert + IdMapWrite + Persist + Send + Sync + 'static,
    P: Send + Sync + 'static,
    S: TryClone + Send + Sync + 'static,
{
    /// Implementation detail. Must be protected by a lock.
    async fn flush_cached_idmap_with_lock(&mut self, map_lock: &M::Lock) -> Result<()> {
        let mut to_insert: Vec<(AncestorPath, Vec<Vertex>)> = Vec::new();
        std::mem::swap(&mut to_insert, &mut *self.overlay_map_paths.lock().unwrap());
        if to_insert.is_empty() {
            return Ok(());
        }

        let id_names = calculate_id_name_from_paths(
            &self.map,
            &*self.dag,
            &self.overlay_map_id_set,
            &to_insert,
        )
        .await?;

        // For testing purpose, skip inserting certain vertexes.
        let mut skip_vertexes: Option<HashSet<Vertex>> = None;
        if crate::is_testing() {
            if let Ok(s) = var("DAG_SKIP_FLUSH_VERTEXES") {
                skip_vertexes = Some(
                    s.split(',')
                        .filter_map(|s| Vertex::from_hex(s.as_bytes()).ok())
                        .collect(),
                )
            }
        }

        for (id, name) in id_names {
            if let Some(skip) = &skip_vertexes {
                if skip.contains(&name) {
                    tracing::info!(
                        target: "dag::cache",
                        "skip flushing {:?}-{} to IdMap set by DAG_SKIP_FLUSH_VERTEXES",
                        &name,
                        id
                    );
                    continue;
                }
            }
            tracing::debug!(target: "dag::cache", "insert {:?}-{} to IdMap", &name, id);
            self.map.insert(id, name.as_ref()).await?;
        }

        self.map.persist(map_lock)?;
        Ok(())
    }
}

impl<IS, M, P, S> AbstractDag<IdDag<IS>, M, P, S>
where
    IS: Send + Sync + 'static,
    IdDag<IS>: StorageVersion,
    M: Send + Sync + 'static,
    P: Send + Sync + 'static,
    S: Send + Sync + 'static,
{
    /// Attempt to reuse caches from `other` if two `Dag`s are compatible.
    /// Usually called when `self` is newly created.
    fn maybe_reuse_caches_from(&mut self, other: &Self) {
        // No need to check IdMap (or "state" which includes both IdDag and IdMap).
        // If IdMap is changed (ex. by flush_cached_idmap), the cache states
        // (missing_vertexes_confirmed_by_remote, overlay_map) are still reusable.
        let dag_version_mismatch = self.dag.storage_version() != other.dag.storage_version();
        let persisted_id_mismatch =
            self.persisted_id_set.as_spans() != other.persisted_id_set.as_spans();
        if dag_version_mismatch || persisted_id_mismatch {
            tracing::debug!(target: "dag::cache", "cannot reuse cache");
            return;
        }
        tracing::debug!(
            target: "dag::cache", "reusing cache ({} missing)",
            other.missing_vertexes_confirmed_by_remote.read().unwrap().len(),
        );
        self.missing_vertexes_confirmed_by_remote =
            other.missing_vertexes_confirmed_by_remote.clone();
        self.overlay_map = other.overlay_map.clone();
        self.overlay_map_paths = other.overlay_map_paths.clone();
    }

    /// Set the remote protocol for converting between Id and Vertex remotely.
    ///
    /// This is usually used on "sparse" ("lazy") Dag where the IdMap is incomplete
    /// for vertexes in the master groups.
    pub fn set_remote_protocol(&mut self, protocol: Arc<dyn RemoteIdConvertProtocol>) {
        self.remote_protocol = protocol;
    }

    /// Inherit configurations like `managed_virtual_group` from `original`.
    fn inherit_configurations_from(&mut self, original: &Self) {
        let seg_size = original.dag.get_new_segment_size();
        self.dag.set_new_segment_size(seg_size);
        self.set_remote_protocol(original.remote_protocol.clone());
        self.managed_virtual_group = original.managed_virtual_group.clone();
        self.maybe_reuse_caches_from(original)
    }
}

#[async_trait::async_trait]
impl<IS, M, P, S> DagAddHeads for AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: TryClone + IdMapAssignHead + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Send + Sync + 'static,
{
    // See docstring in ops.rs for details.
    async fn add_heads(
        &mut self,
        parents: &dyn Parents,
        heads: &VertexListWithOptions,
    ) -> Result<bool> {
        tracing::debug!(target: "dag::add_heads", lifecycle_id=?self.lifecycle_id, ?heads);
        // Populate vertex negative cache to reduce round-trips doing remote lookups.
        // Attention: this might have side effect recreating the snapshots!
        // Skip this optimization for virtual group add_heads since the virtual set
        // is usually small and related tracing logs can be noisy.
        if heads.min_desired_group().unwrap_or(Group::VIRTUAL) < Group::VIRTUAL {
            self.populate_missing_vertexes_for_add_heads(parents, &heads.vertexes())
                .await?;
        }

        // When heads are not VIRTUAL, invalidate_snapshot() helps performance, is optional for
        // correctness. invalidate_snapshot() decreases VerLink ref count so VerLink::bump() can
        // use a fast path mutating in place. When heads are VIRTUAL, invalidate_snapshot() is
        // necessary for correctness, since the versions (VerLinks) won't be bumped to avoid
        // excessive cache invalidation.
        self.invalidate_snapshot();

        // This API takes `heads` with "desired_group"s. When a head already exists in a lower
        // group than its "desired_group" we need to remove the higher-group id and re-assign
        // the head and its ancestors to the lower group.
        //
        // For simplicity, add_heads is *append-only* and does not want to deal with the
        // reassignment. So if you have code like:
        //
        //    let set1 = dag.range(x, y); // set1 is associated with the current dag, "dag v1".
        //    dag.add_heads(...);
        //    let set2 = dag.range(p, q); // set2 is associated with the updated dag, "dag v2".
        //    let set3 = set2 & set1;
        //
        // The `set3` understands that the "dag v2" is a superset of "dag v1" (because add_heads
        // does not strip ids), and can use fast paths - it can assume same ids in set2 and set3
        // mean the same vertexes and ensure set3 is associated with "dag v2". If `add_heads`
        // strips out commits, then the fast paths (note: not just for set3, also p and q) cannot
        // be used.
        //
        // Practically, `heads` match one of these patterns:
        // - (This use-case is going away): desired_group = MASTER for all heads. This is used by
        //   old Mononoke server-side logic. The server only indexes the "main" branch. All vertexes
        //   are in the MASTER group. To avoid misuse by the client-side, we check that there
        //   is nothing outside the MASTER group.
        // - desired_group = NON_MASTER for all heads. This is used by Sapling client.
        //   It might use desired_group = MASTER on add_heads_and_flush, but not here.

        // Performance-wise, add_heads + flush is slower than
        // add_heads_and_flush.
        //
        // Practically, the callsite might want to use add_heads + flush
        // instead of add_heads_and_flush, if:
        // - The callsites cannot figure out "master_heads" at the same time
        //   it does the graph change. For example, hg might know commits
        //   before bookmark movements.
        // - The callsite is trying some temporary graph changes, and does
        //   not want to pollute the on-disk DAG. For example, calculating
        //   a preview of a rebase.
        // Update IdMap. Keep track of what heads are added.
        let mut outcome = PreparedFlatSegments::default();
        let mut covered = self.dag().all_ids_in_groups(&Group::ALL)?;
        let mut reserved = calculate_initial_reserved(self, &covered, heads).await?;
        for (head, opts) in heads.vertex_options() {
            let need_assigning = match self.vertex_id_optional(&head).await? {
                Some(id) => {
                    if id.group() > opts.desired_group {
                        return programming(format!(
                            "add_heads: cannot re-assign {:?}:{:?} from {} to {} (desired), use add_heads_and_flush instead",
                            head,
                            id,
                            id.group(),
                            opts.desired_group
                        ));
                    } else {
                        // In some cases (ex. old Mononoke use-case), the id exists in IdMap but
                        // not IdDag. Still need to assign the id to IdDag.
                        !self.dag.contains_id(id)?
                    }
                }
                None => true,
            };
            if need_assigning {
                let group = opts.desired_group;
                // If any ancestors have incompatible group (ex. desired = MASTER, ancestor has
                // NON_MASTER), then `assign_head` below will report an error.
                let prepared_segments = self
                    .assign_head(head.clone(), parents, group, &mut covered, &reserved)
                    .await?;
                outcome.merge(prepared_segments);
                if opts.reserve_size > 0 {
                    let low = self.map.vertex_id(head.clone()).await? + 1;
                    update_reserved(&mut reserved, &covered, low, opts.reserve_size);
                }
                if group != Group::VIRTUAL {
                    self.pending_heads.push((head, opts));
                }
            }
        }

        // Update high level segments from the flat segments just inserted.
        self.dag
            .build_segments_from_prepared_flat_segments(&outcome)?;

        Ok(outcome.segment_count() > 0)
    }
}

#[async_trait::async_trait]
impl<IS, M, P, S> DagStrip for AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore + Persist,
    IdDag<IS>: TryClone + StorageVersion,
    M: TryClone + Persist + IdMapWrite + IdConvert + Send + Sync + 'static,
    P: TryClone + Open<OpenTarget = Self> + Send + Sync + 'static,
    S: TryClone + Persist + Send + Sync + 'static,
{
    async fn strip(&mut self, set: &Set) -> Result<()> {
        if !self.pending_heads.is_empty() {
            return programming(format!(
                "strip does not support pending heads ({:?})",
                &self.pending_heads.vertexes(),
            ));
        }
        tracing::debug!(target: "dag::strip", lifecycle_id=?self.lifecycle_id, ?set);

        // Do strip with a lock to avoid cases where descendants are added to
        // the stripped segments.
        let mut new: Self = self.path.open()?;
        let (lock, map_lock, dag_lock) = new.reload()?;
        new.inherit_configurations_from(self);

        new.strip_with_lock(set, &map_lock).await?;
        new.persist(lock, map_lock, dag_lock)?;
        new.maybe_recreate_virtual_group().await?;

        *self = new;
        Ok(())
    }
}

impl<IS, M, P, S> AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: TryClone + Persist + IdMapWrite + IdConvert + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Send + Sync + 'static,
{
    /// Internal implementation of "strip".
    async fn strip_with_lock(&mut self, set: &Set, map_lock: &M::Lock) -> Result<()> {
        if !self.pending_heads.is_empty() {
            return programming(format!(
                "strip does not support pending heads ({:?})",
                &self.pending_heads.vertexes(),
            ));
        }

        let id_set = self.to_id_set(set).await?;

        // Heads in the master group must be known. Strip might "create" heads that are not
        // currently known. Resolve them to ensure graph integrity.
        let head_ids: Vec<Id> = {
            // strip will include descendants.
            let to_strip = self.dag.descendants(id_set.clone())?;
            // only vertexes in the master group can be lazy.
            let master_group = self.dag.master_group()?;
            let master_group_after_strip = master_group.difference(&to_strip);
            let heads_before_strip = self.dag.heads_ancestors(master_group)?;
            let heads_after_strip = self.dag.heads_ancestors(master_group_after_strip)?;
            let new_heads = heads_after_strip.difference(&heads_before_strip);
            new_heads.iter_desc().collect()
        };
        let heads_after_strip = self.vertex_name_batch(&head_ids).await?;
        tracing::debug!(target: "dag::strip", "heads after strip: {:?}", &heads_after_strip);
        // Write IdMap cache first, they will become problematic to write
        // after "remove" because the `VerLink`s might become incompatible.
        self.flush_cached_idmap_with_lock(map_lock).await?;

        let removed_id_set = self.dag.strip(id_set)?;
        tracing::debug!(target: "dag::strip", "removed id set: {:?}", &removed_id_set);

        let mut removed_vertexes = Vec::new();
        for span in removed_id_set.iter_span_desc() {
            let vertexes = self.map.remove_range(span.low, span.high).await?;
            removed_vertexes.extend(vertexes);
        }
        tracing::debug!(target: "dag::strip", "removed vertexes: {:?}", &removed_vertexes);

        // Add removed names to missing cache.
        self.missing_vertexes_confirmed_by_remote
            .write()
            .unwrap()
            .extend(removed_vertexes);

        // Snapshot cannot be reused.
        self.invalidate_snapshot();

        Ok(())
    }
}

#[async_trait::async_trait]
impl<IS, M, P, S> IdMapWrite for AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: TryClone + IdMapAssignHead + Send + Sync,
    P: TryClone + Send + Sync,
    S: TryClone + Send + Sync,
{
    async fn insert(&mut self, id: Id, name: &[u8]) -> Result<()> {
        self.map.insert(id, name).await
    }

    async fn remove_range(&mut self, low: Id, high: Id) -> Result<Vec<Vertex>> {
        self.map.remove_range(low, high).await
    }
}

#[async_trait::async_trait]
impl<IS, M, P, S> DagImportCloneData for AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore + Persist + 'static,
    IdDag<IS>: TryClone,
    M: TryClone + IdMapAssignHead + Persist + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Persist + Send + Sync + 'static,
{
    async fn import_clone_data(&mut self, clone_data: CloneData<Vertex>) -> Result<()> {
        // Write directly to disk. Bypassing "flush()" that re-assigns Ids
        // using parent functions.
        let (lock, map_lock, dag_lock) = self.reload()?;

        if !self.dag.all()?.is_empty() {
            return programming("Cannot import clone data for non-empty graph");
        }
        for (id, name) in clone_data.idmap {
            tracing::debug!(target: "dag::clone", "insert IdMap: {:?}-{:?}", &name, id);
            self.map.insert(id, name.as_ref()).await?;
        }
        self.dag
            .build_segments_from_prepared_flat_segments(&clone_data.flat_segments)?;

        self.verify_missing().await?;

        self.persist(lock, map_lock, dag_lock)
    }
}

impl<IS, M, P, S> AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore + Persist + 'static,
    IdDag<IS>: TryClone,
    M: TryClone + IdMapAssignHead + Persist + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Persist + Send + Sync + 'static,
{
    /// Verify that universally known vertexes and heads are present in IdMap.
    async fn verify_missing(&self) -> Result<()> {
        let missing: Vec<Id> = self.check_universal_ids().await?;
        if !missing.is_empty() {
            let msg = format!(
                concat!(
                    "Clone data does not contain vertex for {:?}. ",
                    "This is most likely a server-side bug."
                ),
                missing,
            );
            return programming(msg);
        }

        Ok(())
    }

    fn reload(&mut self) -> Result<(S::Lock, M::Lock, IS::Lock)> {
        let lock = self.state.lock()?;
        let map_lock = self.map.lock()?;
        let dag_lock = self.dag.lock()?;
        self.state.reload(&lock)?;
        self.map.reload(&map_lock)?;
        self.dag.reload(&dag_lock)?;

        Ok((lock, map_lock, dag_lock))
    }

    fn persist(&mut self, lock: S::Lock, map_lock: M::Lock, dag_lock: IS::Lock) -> Result<()> {
        self.map.persist(&map_lock)?;
        self.dag.persist(&dag_lock)?;
        self.state.persist(&lock)?;

        self.invalidate_overlay_map()?;
        self.persisted_id_set = self.dag.all_ids_in_groups(&Group::PERSIST)?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl<IS, M, P, S> DagImportPullData for AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore + Persist,
    IdDag<IS>: TryClone + StorageVersion,
    M: TryClone + IdMapAssignHead + Persist + Send + Sync + 'static,
    P: Open<OpenTarget = Self> + TryClone + Send + Sync + 'static,
    S: TryClone + Persist + Send + Sync + 'static,
{
    // See docstring in ops.py for details.
    async fn import_pull_data(
        &mut self,
        clone_data: CloneData<Vertex>,
        heads: &VertexListWithOptions,
    ) -> Result<()> {
        if !self.pending_heads.is_empty() {
            return programming(format!(
                "import_pull_data called with pending heads ({:?})",
                &self.pending_heads.vertexes(),
            ));
        }
        if let Some(group) = heads.max_desired_group() {
            if group != Group::MASTER {
                return programming(concat!(
                    "import_pull_data should only take MASTER group heads. ",
                    "Only MASTER group can contain lazy vertexes like what pull_data uses."
                ));
            }
        }

        for id in clone_data.flat_segments.parents_head_and_roots() {
            if !clone_data.idmap.contains_key(&id) {
                return programming(format!(
                    "server does not provide name for id {:?} in pull data",
                    id
                ));
            }
        }

        // Constructs a new graph so we don't expose a broken `self` state on error.
        let mut new: Self = self.path.open()?;
        let (lock, map_lock, dag_lock) = new.reload()?;
        new.inherit_configurations_from(self);

        // Parents that should exist in the local graph. Look them up in 1 round-trip
        // and insert to the local graph.
        // Also check that roots of the new segments do not overlap with the local graph.
        // For example,
        //
        //      D          When the client has B (and A, C), and is pulling D,
        //     /|\         the server provides D, E, F, with parents B and C,
        //    F B E        and roots F and E.
        //      |\|        The client must have B and C, and must not have F
        //      A C        or E.
        {
            let mut root_ids: Vec<Id> = Vec::new();
            let mut parent_ids: Vec<Id> = Vec::new();
            let segments = &clone_data.flat_segments.segments;
            let id_set = IdSet::from_spans(segments.iter().map(|s| s.low..=s.high));
            for seg in segments {
                let pids: Vec<Id> = seg.parents.to_vec();
                // Parents that are not part of the pull vertexes should exist
                // in the local graph.
                let connected_pids: Vec<Id> = pids
                    .iter()
                    .copied()
                    .filter(|&p| !id_set.contains(p))
                    .collect();
                if connected_pids.len() == pids.len() {
                    // The "low" of the segment is a root (of vertexes to insert).
                    // It needs an overlap check.
                    root_ids.push(seg.low);
                }
                parent_ids.extend(connected_pids);
            }

            let to_names = |ids: &[Id], hint: &str| -> Result<Vec<Vertex>> {
                let names = ids.iter().map(|i| match clone_data.idmap.get(i) {
                    Some(v) => Ok(v.clone()),
                    None => {
                        programming(format!("server does not provide name for {} {:?}", hint, i))
                    }
                });
                names.collect()
            };

            let parent_names = to_names(&parent_ids, "parent")?;
            let root_names = to_names(&root_ids, "root")?;
            tracing::trace!(
                "pull: connected parents: {:?}, roots: {:?}",
                &parent_names,
                &root_names
            );

            // Pre-lookup in one round-trip.
            let mut names = parent_names
                .iter()
                .chain(root_names.iter())
                .cloned()
                .collect::<Vec<_>>();
            names.sort_unstable();
            names.dedup();
            let resolved = new.vertex_id_batch(&names).await?;
            assert_eq!(resolved.len(), names.len());
            for (id, name) in resolved.into_iter().zip(names) {
                if let Ok(id) = id {
                    if !new.map.contains_vertex_name(&name).await? {
                        tracing::debug!(target: "dag::pull", "insert IdMap: {:?}-{:?}", &name, id);
                        new.map.insert(id, name.as_ref()).await?;
                    }
                }
            }

            for name in root_names {
                if new.contains_vertex_name(&name).await? {
                    let e = NeedSlowPath(format!("{:?} exists in local graph", name));
                    return Err(e);
                }
            }

            let client_parents = new.vertex_id_batch(&parent_names).await?;
            client_parents.into_iter().collect::<Result<Vec<Id>>>()?;
        }

        // Prepare indexes and states used below.
        /// Query server segments with some indexes.
        struct ServerState<'a> {
            seg_by_high: BTreeMap<Id, FlatSegment>,
            idmap_by_name: BTreeMap<&'a Vertex, Id>,
            idmap_by_id: &'a BTreeMap<Id, Vertex>,
        }
        let mut server = ServerState {
            seg_by_high: clone_data
                .flat_segments
                .segments
                .iter()
                .map(|s| (s.high, s.clone()))
                .collect(),
            idmap_by_name: clone_data
                .idmap
                .iter()
                .map(|(&id, name)| (name, id))
                .collect(),
            idmap_by_id: &clone_data.idmap,
        };

        impl<'a> ServerState<'a> {
            /// Find the segment that contains the (server-side) Id.
            fn seg_containing_id(&self, server_id: Id) -> Result<&FlatSegment> {
                let seg = match self.seg_by_high.range(server_id..).next() {
                    Some((_high, seg)) => {
                        if seg.low <= server_id && seg.high >= server_id {
                            Some(seg)
                        } else {
                            None
                        }
                    }
                    None => None,
                };
                seg.ok_or_else(|| {
                    DagError::Programming(format!(
                        "server does not provide segment covering id {}",
                        server_id
                    ))
                })
            }

            /// Split a server segment from `[ low --  middle -- high ]` to
            /// `[ low -- middle ] [ middle + 1 -- high ]`.
            fn split_seg(&mut self, high: Id, middle: Id) {
                // This is useful when "middle" is a "parent" of another segment, like:
                //    seg 1 (server): 100 -- 115 -- 120
                //    seg 2 (server):                    121 -- 130, parents: [115]
                // While the task is to ensure seg 2's head H is present, we can split seg 1:
                //    seg 1a (server): 110 -- 115
                //    seg 1b (server):            116 -- 120, parents: [115]
                //    seg 2  (server):                       121 -- 130 (H), parents: [115]
                // Then remap and insert seg 1a and seg 2 first to complete the "H" goal:
                //    seg 1a (client): 10 -- 15
                //    seg 2  (client):          16 -- 20 (H), parents: [15]
                // The 10 ... 20 range is now continuous and friendly to merge to high-level
                // segments. The rest of seg 1, seg 1b (server) can be picked up later when
                // visiting from other heads.
                let seg = self
                    .seg_by_high
                    .remove(&high)
                    .expect("bug: invalid high passed to split_seg");
                assert!(seg.low <= middle);
                assert!(seg.high > middle);
                assert!(self.idmap_by_id.contains_key(&middle));
                let seg1 = FlatSegment {
                    low: seg.low,
                    high: middle,
                    parents: seg.parents,
                };
                let seg2 = FlatSegment {
                    low: middle + 1,
                    high: seg.high,
                    parents: vec![middle],
                };
                self.seg_by_high.insert(seg1.high, seg1);
                self.seg_by_high.insert(seg2.high, seg2);
            }

            fn name_by_id(&self, id: Id) -> Vertex {
                self.idmap_by_id
                    .get(&id)
                    .expect("IdMap should contain the `id`. It should be checked before.")
                    .clone()
            }

            fn id_by_name(&self, name: &Vertex) -> Option<Id> {
                self.idmap_by_name.get(name).copied()
            }
        }

        // `taken` is the union of `covered` and `reserved`, mainly used by `find_free_span`.
        let mut taken = {
            // Normally we would want `calculate_initial_reserved` here. But we calculate head
            // reservation for all `heads` in order, instead of just considering heads in the
            // `clone_data`. So we're fine without the "initial reserved". In other words, the
            // `calculate_initial_reserved` logic is "inlined" into the `for ... in heads`
            // loop below.
            new.dag().all_ids_in_groups(&[Group::MASTER])?
        };

        // Output. Remapped segments to insert.
        let mut prepared_client_segments = PreparedFlatSegments::default();

        // Insert segments by visiting the heads following the `VertexOptions` order.
        //
        // If a segment is not ready to be inserted (ex. its parents are still missing),
        // their parents will be visited recursively. This has the nice effects
        // comparing to `import_clone_data` which blindly takes the input as-is:
        // - De-fragment `clone_data`: gaps or sub-optional segment order won't hurt.
        // - Respect the local `VertexOptions`: respect the order and reserve_size
        //   set locally if possible.
        // - Ignore "bogus" unrelated sub-graph: if the `clone_data` contains more
        //   segments then needed, they will be simply ignored.
        //
        // The implementation (of using a stack) is similar to `IdMap::assign_head`,
        // but insert a segment at a time, not a vertex at a time.
        //
        // Only the MASTER group supports laziness. So we only care about it.
        for (head, opts) in heads.vertex_options() {
            let mut stack: Vec<Id> = vec![];
            if let Some(head_server_id) = server.id_by_name(&head) {
                let _head_server_seg = server.seg_containing_id(head_server_id)?;
                stack.push(head_server_id);
            }

            while let Some(server_high) = stack.pop() {
                let mut server_seg = server.seg_containing_id(server_high)?;
                if server_high < server_seg.high {
                    // Split the segment for more efficient high level segments.
                    let seg_high = server_seg.high;
                    server.split_seg(seg_high, server_high);
                    server_seg = server.seg_containing_id(server_high)?;
                    assert_eq!(server_high, server_seg.high);
                }
                let high_vertex = server.name_by_id(server_high);
                let client_high_id = new
                    .map
                    .vertex_id_with_max_group(&high_vertex, Group::MAX)
                    .await?;
                match client_high_id {
                    Some(id) if id.group() == Group::MASTER => {
                        // `server_seg` is present in MASTER group (previously inserted
                        // by this loop). No need to insert or visit parents.
                        continue;
                    }
                    Some(id) => {
                        // `id` in non-MASTER group. This should not really happen because we have
                        // checked all "roots" are missing in the local graph. See `NeedSlowPath`
                        // above.
                        let e = NeedSlowPath(format!(
                            "{:?} exists in local graph as {:?} - fast path requires MASTER group",
                            &high_vertex, id
                        ));
                        return Err(e);
                    }
                    None => {}
                }

                let parent_server_ids = &server_seg.parents;
                let parent_names: Vec<Vertex> = {
                    let iter = parent_server_ids.iter().map(|id| server.name_by_id(*id));
                    iter.collect()
                };

                // The client parent ids in the MASTER group.
                let mut parent_client_ids = Vec::new();
                let mut missing_parent_server_ids = Vec::new();

                // Calculate `parent_client_ids`, and `missing_parent_server_ids`.
                // Intentionally using `new.map` not `new` to bypass remote lookups.
                {
                    let client_id_res = new.map.vertex_id_batch(&parent_names).await?;
                    assert_eq!(client_id_res.len(), parent_server_ids.len());
                    for (res, &server_id) in client_id_res.into_iter().zip(parent_server_ids) {
                        match res {
                            Ok(id) if id.group() != Group::MASTER => {
                                return Err(NeedSlowPath(format!(
                                    "{:?} exists id in local graph as {:?} - fast path requires MASTER group",
                                    &parent_names, id
                                )));
                            }
                            Ok(id) => {
                                parent_client_ids.push(id);
                            }
                            Err(crate::Error::VertexNotFound(_)) => {
                                missing_parent_server_ids.push(server_id);
                            }
                            Err(e) => return Err(e),
                        }
                    }
                }

                if !missing_parent_server_ids.is_empty() {
                    // Parents are not ready. Needs revisit this segment after inserting parents.
                    stack.push(server_high);
                    // Insert missing parents.
                    // First parent, first insertion.
                    for &server_id in missing_parent_server_ids.iter().rev() {
                        stack.push(server_id);
                    }
                    continue;
                }

                // All parents are present. Time to insert this segment.
                // Find a suitable low..=high range.
                let candidate_id = match parent_client_ids.iter().max().copied() {
                    None => Group::MASTER.min_id(),
                    Some(id) => id + 1,
                };
                let size = server_seg.high.0 - server_seg.low.0 + 1;
                let span = find_free_span(&taken, candidate_id, size, false);

                // Map the server_seg.low..=server_seg.high to client span.low..=span.high.
                // Insert to IdMap.
                for (&server_id, name) in server.idmap_by_id.range(server_seg.low..=server_seg.high)
                {
                    let client_id = server_id + span.low.0 - server_seg.low.0;
                    if client_id.group() != Group::MASTER {
                        return Err(crate::Error::IdOverflow(Group::MASTER));
                    }
                    new.map.insert(client_id, name.as_ref()).await?;
                }

                // Prepare insertion to IdDag.
                prepared_client_segments.push_segment(span.low, span.high, &parent_client_ids);

                // Mark the range as taken.
                taken.push(span);
            }

            // Consider reservation for `head` by updating `taken`.
            if opts.reserve_size > 0 {
                let head_client_id = new.map.vertex_id(head).await?;
                let span = find_free_span(&taken, head_client_id + 1, opts.reserve_size as _, true);
                taken.push(span);
            }
        }

        new.dag
            .build_segments_from_prepared_flat_segments(&prepared_client_segments)?;

        // Some "missing" vertexes might be imported.
        new.invalidate_missing_vertex_cache();

        if cfg!(debug_assertions) {
            new.verify_missing().await?;
        }

        new.persist(lock, map_lock, dag_lock)?;

        // Update maintained VIRTUAL group.
        new.maybe_recreate_virtual_group().await?;

        *self = new;
        Ok(())
    }
}

#[async_trait::async_trait]
impl<IS, M, P, S> DagExportCloneData for AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: IdConvert + TryClone + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Send + Sync + 'static,
{
    async fn export_clone_data(&self) -> Result<CloneData<Vertex>> {
        let idmap: BTreeMap<Id, Vertex> = {
            let ids: Vec<Id> = self.dag.universal_ids()?.into_iter().collect();
            tracing::debug!("export: {} universally known vertexes", ids.len());
            let names = {
                let fallible_names = self.vertex_name_batch(&ids).await?;
                let mut names = Vec::with_capacity(fallible_names.len());
                for name in fallible_names {
                    names.push(name?);
                }
                names
            };
            ids.into_iter().zip(names).collect()
        };

        let flat_segments: PreparedFlatSegments = {
            let segments = self.dag.next_segments(Id::MIN, 0)?;
            let mut prepared = Vec::with_capacity(segments.len());
            for segment in segments {
                let span = segment.span()?;
                let parents = segment.parents()?;
                prepared.push(FlatSegment {
                    low: span.low,
                    high: span.high,
                    parents,
                });
            }
            PreparedFlatSegments {
                segments: prepared.into_iter().collect(),
            }
        };

        let data = CloneData {
            flat_segments,
            idmap,
        };
        Ok(data)
    }
}

#[async_trait::async_trait]
impl<IS, M, P, S> DagExportPullData for AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: IdConvert + TryClone + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Send + Sync + 'static,
{
    async fn export_pull_data(&self, set: &Set) -> Result<CloneData<Vertex>> {
        let id_set = self.to_id_set(set).await?;

        let flat_segments = self.dag.idset_to_flat_segments(id_set)?;
        let ids: Vec<_> = flat_segments.parents_head_and_roots().into_iter().collect();

        let idmap: BTreeMap<Id, Vertex> = {
            tracing::debug!("pull: {} vertexes in idmap", ids.len());
            let names = {
                let fallible_names = self.vertex_name_batch(&ids).await?;
                let mut names = Vec::with_capacity(fallible_names.len());
                for name in fallible_names {
                    names.push(name?);
                }
                names
            };
            assert_eq!(ids.len(), names.len());
            ids.into_iter().zip(names).collect()
        };

        let data = CloneData {
            flat_segments,
            idmap,
        };
        Ok(data)
    }
}

impl<IS, M, P, S> AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: TryClone + Send + Sync,
    P: TryClone + Send + Sync,
    S: TryClone + Send + Sync,
{
    /// Invalidate cached content. Call this before changing the graph
    /// so `version` in `snapshot` is dropped, and `version.bump()` might
    /// have a faster path.
    ///
    /// Forgetting to call this function might hurt performance a bit, but does
    /// not affect correctness.
    fn invalidate_snapshot(&mut self) {
        *self.snapshot.write().unwrap() = None;
    }

    fn invalidate_missing_vertex_cache(&mut self) {
        tracing::debug!(target: "dag::cache", "cleared missing cache");
        *self.missing_vertexes_confirmed_by_remote.write().unwrap() = Default::default();
    }

    fn invalidate_overlay_map(&mut self) -> Result<()> {
        self.overlay_map = Default::default();
        self.update_overlay_map_id_set()?;
        tracing::debug!(target: "dag::cache", "cleared overlay map cache");
        Ok(())
    }

    fn update_overlay_map_id_set(&mut self) -> Result<()> {
        self.overlay_map_id_set = self.dag.master_group()?;
        Ok(())
    }

    /// Attempt to get a snapshot of this graph.
    pub(crate) fn try_snapshot(&self) -> Result<Arc<Self>> {
        if let Some(s) = self.snapshot.read().unwrap().deref() {
            if s.dag.version() == self.dag.version() {
                return Ok(Arc::clone(s));
            }
        }

        let mut snapshot = self.snapshot.write().unwrap();
        match snapshot.deref() {
            Some(s) if s.dag.version() == self.dag.version() => Ok(s.clone()),
            _ => {
                let cloned = Self {
                    dag: self.dag.try_clone()?,
                    map: self.map.try_clone()?,
                    snapshot: Default::default(),
                    pending_heads: self.pending_heads.clone(),
                    persisted_id_set: self.persisted_id_set.clone(),
                    path: self.path.try_clone()?,
                    state: self.state.try_clone()?,
                    id: self.id.clone(),
                    // If we do deep clone here we can remove `overlay_map_next_id`
                    // protection. However that could be too expensive.
                    overlay_map: Arc::clone(&self.overlay_map),
                    overlay_map_id_set: self.overlay_map_id_set.clone(),
                    overlay_map_paths: Arc::clone(&self.overlay_map_paths),
                    remote_protocol: self.remote_protocol.clone(),
                    managed_virtual_group: self.managed_virtual_group.clone(),
                    missing_vertexes_confirmed_by_remote: Arc::clone(
                        &self.missing_vertexes_confirmed_by_remote,
                    ),
                    lifecycle_id: self.lifecycle_id.clone(),
                    internal_stats: Default::default(),
                };
                let result = Arc::new(cloned);
                *snapshot = Some(Arc::clone(&result));
                Ok(result)
            }
        }
    }

    pub fn dag(&self) -> &IdDag<IS> {
        &self.dag
    }

    pub fn map(&self) -> &M {
        &self.map
    }

    pub(crate) fn get_remote_protocol(&self) -> Arc<dyn RemoteIdConvertProtocol> {
        self.remote_protocol.clone()
    }
}

impl<IS, M, P, S> AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: TryClone + IdMapAssignHead + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Send + Sync + 'static,
{
    async fn populate_missing_vertexes_for_add_heads(
        &mut self,
        parents: &dyn Parents,
        heads: &[Vertex],
    ) -> Result<()> {
        if self.is_vertex_lazy() {
            let unassigned = calculate_definitely_unassigned_vertexes(self, parents, heads).await?;
            let mut missing = self.missing_vertexes_confirmed_by_remote.write().unwrap();
            for v in unassigned {
                if missing.insert(v.clone()) {
                    tracing::trace!(target: "dag::cache", "cached missing {:?} (definitely missing)", &v);
                }
            }
        }
        Ok(())
    }
}

/// Calculate vertexes that are definitely not assigned (not in the IdMap,
/// and not in the lazy part of the IdMap) according to
/// `hint_pending_subdag`. This does not report all unassigned vertexes.
/// But the reported vertexes are guaranteed not assigned.
///
/// If X is assigned, then X's parents must have been assigned.
/// If X is not assigned, then all X's descendants are not assigned.
///
/// This function visits the "roots" of "parents", and if they are not assigned,
/// then add their descendants to the "unassigned" result set.
async fn calculate_definitely_unassigned_vertexes<IS, M, P, S>(
    this: &AbstractDag<IdDag<IS>, M, P, S>,
    parents: &dyn Parents,
    heads: &[Vertex],
) -> Result<Vec<Vertex>>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: TryClone + IdMapAssignHead + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Send + Sync + 'static,
{
    // subdag: vertexes to insert
    //
    // For example, when adding C---D to the graph A---B:
    //
    //      A---B
    //           \
    //            C---D
    //
    // The subdag is C---D (C does not have parent).
    //
    // Extra checks are needed because upon reload, the main graph
    // A---B might already contain part of the subdag to be added.
    let subdag = parents.hint_subdag_for_insertion(heads).await?;

    let mut remaining = subdag.all().await?;
    let mut unassigned = Set::empty();

    // For lazy graph, avoid some remote lookups by figuring out
    // some definitely unassigned (missing) vertexes. For example,
    //
    //      A---B---C
    //           \
    //            D---E
    //
    // When adding D---E (subdag, new vertex that might trigger remote
    // lookup) with parent B to the main graph (A--B--C),
    // 1. If B exists, and is not in the master group, then B and its
    //    descendants cannot be not lazy, and there is no need to lookup
    //    D remotely.
    // 2. If B exists, and is in the master group, and all its children
    //    except D (i.e. C) are known locally, and the vertex name of D
    //    does not match other children (C), we know that D cannot be
    //    in the lazy part of the main graph, and can skip the remote
    //    lookup.
    let mut unassigned_roots = Vec::new();
    if this.is_vertex_lazy() {
        let roots = subdag.roots(remaining.clone()).await?;
        let mut roots_iter = roots.iter().await?;
        while let Some(root) = roots_iter.next().await {
            let root = root?;

            // Do a local "contains" check.
            if matches!(
                &this.contains_vertex_name_locally(&[root.clone()]).await?[..],
                [true]
            ) {
                tracing::debug!(target: "dag::definitelymissing", "root {:?} is already known", &root);
                continue;
            }

            let root_parents_id_set = {
                let root_parents = parents.parent_names(root.clone()).await?;
                let root_parents_set = match this.sort(&Set::from_static_names(root_parents)).await
                {
                    Ok(set) => set,
                    Err(_) => {
                        tracing::trace!(target: "dag::definitelymissing", "root {:?} is unclear (parents cannot be resolved)", &root);
                        continue;
                    }
                };
                this.to_id_set(&root_parents_set).await?
            };

            // If there are no parents of `root`, we cannot confidently test
            // whether `root` is missing or not.
            if root_parents_id_set.is_empty() {
                tracing::trace!(target: "dag::definitelymissing", "root {:?} is unclear (no parents)", &root);
                continue;
            }

            // All parents of `root` are non-lazy.
            // So `root` is non-lazy and the local "contains" check is the same
            // as a remote "contains" check.
            if root_parents_id_set
                .iter_desc()
                .all(|i| i.group() > Group::MASTER)
            {
                tracing::debug!(target: "dag::definitelymissing", "root {:?} is not assigned (non-lazy parent)", &root);
                unassigned_roots.push(root);
                continue;
            }

            // All children of lazy parents of `root` are known locally.
            // So `root` cannot match an existing vertex in the lazy graph.
            let children_ids: Vec<Id> = this
                .dag
                .children(root_parents_id_set)?
                .iter_desc()
                .collect();
            if this
                .map
                .contains_vertex_id_locally(&children_ids)
                .await?
                .iter()
                .all(|b| *b)
            {
                tracing::debug!(target: "dag::definitelymissing", "root {:?} is not assigned (children of parents are known)", &root);
                unassigned_roots.push(root);
                continue;
            }

            tracing::trace!(target: "dag::definitelymissing", "root {:?} is unclear", &root);
        }

        if !unassigned_roots.is_empty() {
            unassigned = subdag
                .descendants(Set::from_static_names(unassigned_roots))
                .await?;
            remaining = remaining.difference(&unassigned);
        }
    }

    // Figure out unassigned (missing) vertexes that do need to be inserted.
    // This is done via utils::filter_known.
    let filter_known = |sample: &[Vertex]| -> BoxFuture<Result<Vec<Vertex>>> {
        let sample = sample.to_vec();
        async {
            let known_bools: Vec<bool> = {
                let ids = this.vertex_id_batch(&sample).await?;
                ids.into_iter().map(|i| i.is_ok()).collect()
            };
            debug_assert_eq!(sample.len(), known_bools.len());
            let known = sample
                .into_iter()
                .zip(known_bools)
                .filter_map(|(v, b)| if b { Some(v) } else { None })
                .collect();
            Ok(known)
        }
        .boxed()
    };
    let assigned = utils::filter_known(remaining.clone(), &filter_known).await?;
    unassigned = unassigned.union(&remaining.difference(&assigned));
    tracing::debug!(target: "dag::definitelymissing", "unassigned (missing): {:?}", &unassigned);

    let unassigned = unassigned.iter().await?.try_collect().await?;
    Ok(unassigned)
}

// The "client" Dag. Using a remote protocol to fill lazy part of the vertexes.
impl<IS, M, P, S> AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: IdConvert + TryClone + Send + Sync,
    P: TryClone + Send + Sync,
    S: TryClone + Send + Sync,
{
    /// Resolve vertexes remotely and cache the result in the overlay map.
    /// Return the resolved ids in the given order. Not all names are resolved.
    async fn resolve_vertexes_remotely(&self, names: &[Vertex]) -> Result<Vec<Option<Id>>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        if is_remote_protocol_disabled() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "resolving vertexes remotely disabled",
            )
            .into());
        }
        if names.len() < 30 {
            tracing::debug!(target: "dag::protocol", "resolve names {:?} remotely", &names);
        } else {
            tracing::debug!(target: "dag::protocol", "resolve names ({}) remotely", names.len());
        }
        crate::failpoint!("dag-resolve-vertexes-remotely");
        let request: protocol::RequestNameToLocation =
            (self.map(), self.dag()).process(names.to_vec()).await?;
        let path_names = self
            .remote_protocol
            .resolve_names_to_relative_paths(request.heads, request.names)
            .await?;
        self.insert_relative_paths(path_names).await?;
        let overlay = self.overlay_map.read().unwrap();
        let mut ids = Vec::with_capacity(names.len());
        let mut missing = self.missing_vertexes_confirmed_by_remote.write().unwrap();
        for name in names {
            if let Some(id) = overlay.lookup_vertex_id(name) {
                ids.push(Some(id));
            } else {
                tracing::trace!(target: "dag::cache", "cached missing {:?} (server confirmed)", &name);
                missing.insert(name.clone());
                ids.push(None);
            }
        }
        Ok(ids)
    }

    /// Resolve ids remotely and cache the result in the overlay map.
    /// Return the resolved ids in the given order. All ids must be resolved.
    async fn resolve_ids_remotely(&self, ids: &[Id]) -> Result<Vec<Vertex>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if is_remote_protocol_disabled() {
            return Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "resolving ids remotely disabled",
            )
            .into());
        }
        if ids.len() < 30 {
            tracing::debug!(target: "dag::protocol", "resolve ids {:?} remotely", &ids);
        } else {
            tracing::debug!(target: "dag::protocol", "resolve ids ({}) remotely", ids.len());
        }
        crate::failpoint!("dag-resolve-ids-remotely");
        let request: protocol::RequestLocationToName = (self.map(), self.dag())
            .process(IdSet::from_spans(ids.iter().copied()))
            .await?;
        let path_names = self
            .remote_protocol
            .resolve_relative_paths_to_names(request.paths)
            .await?;
        self.insert_relative_paths(path_names).await?;
        let overlay = self.overlay_map.read().unwrap();
        let mut names = Vec::with_capacity(ids.len());
        for &id in ids {
            if let Some(name) = overlay.lookup_vertex_name(id).cloned() {
                names.push(name);
            } else {
                return id.not_found();
            }
        }
        Ok(names)
    }

    /// Insert `x~n` relative paths to the overlay IdMap.
    async fn insert_relative_paths(
        &self,
        path_names: Vec<(AncestorPath, Vec<Vertex>)>,
    ) -> Result<()> {
        if path_names.is_empty() {
            return Ok(());
        }
        let to_insert: Vec<(Id, Vertex)> = calculate_id_name_from_paths(
            self.map(),
            self.dag().deref(),
            &self.overlay_map_id_set,
            &path_names,
        )
        .await?;

        let mut paths = self.overlay_map_paths.lock().unwrap();
        paths.extend(path_names);
        drop(paths);

        let mut overlay = self.overlay_map.write().unwrap();
        for (id, name) in to_insert {
            tracing::trace!(target: "dag::cache", "cached mapping {:?} <=> {:?}", id, &name);
            overlay.insert_vertex_id_name(id, name);
        }

        Ok(())
    }
}

/// Calculate (id, name) pairs to insert from (path, [name]) pairs.
async fn calculate_id_name_from_paths(
    map: &dyn IdConvert,
    dag: &dyn IdDagAlgorithm,
    overlay_map_id_set: &IdSet,
    path_names: &[(AncestorPath, Vec<Vertex>)],
) -> Result<Vec<(Id, Vertex)>> {
    if path_names.is_empty() {
        return Ok(Vec::new());
    }
    let mut to_insert: Vec<(Id, Vertex)> =
        Vec::with_capacity(path_names.iter().map(|(_, ns)| ns.len()).sum());
    for (path, names) in path_names {
        if names.is_empty() {
            continue;
        }
        // Resolve x~n to id. x is "universally known" so it should exist locally.
        let x_id = map.vertex_id(path.x.clone()).await.map_err(|e| {
            let msg = format!(
                concat!(
                    "Cannot resolve x ({:?}) in x~n locally. The x is expected to be known ",
                    "locally and is populated at clone time. This x~n is used to convert ",
                    "{:?} to a location in the graph. (Check initial clone logic) ",
                    "(Error: {})",
                ),
                &path.x, &names[0], e
            );
            crate::Error::Programming(msg)
        })?;
        tracing::trace!(
            "resolve path {:?} names {:?} (x = {}) to overlay",
            &path,
            &names,
            x_id
        );
        if !overlay_map_id_set.contains(x_id) {
            crate::failpoint!("dag-error-x-n-overflow");
            let msg = format!(
                concat!(
                    "Server returned x~n (x = {:?} {}, n = {}). But x is out of range ",
                    "({:?}). This is not expected and indicates some ",
                    "logic error on the server side."
                ),
                &path.x, x_id, path.n, overlay_map_id_set
            );
            return programming(msg);
        }
        let mut id = match dag.first_ancestor_nth(x_id, path.n).map_err(|e| {
            let msg = format!(
                concat!(
                    "Cannot resolve x~n (x = {:?} {}, n = {}): {}. ",
                    "This indicates the client-side graph is somewhat incompatible from the ",
                    "server-side graph. Something (server-side or client-side) was probably ",
                    "seriously wrong before this error."
                ),
                &path.x, x_id, path.n, e
            );
            crate::Error::Programming(msg)
        }) {
            Err(e) => {
                crate::failpoint!("dag-error-x-n-unresolvable");
                return Err(e);
            }
            Ok(id) => id,
        };
        if names.len() < 30 {
            tracing::debug!("resolved {:?} => {} {:?}", &path, id, &names);
        } else {
            tracing::debug!("resolved {:?} => {} {:?} ...", &path, id, &names[0]);
        }
        for (i, name) in names.iter().enumerate() {
            if i > 0 {
                // Follow id's first parent.
                id = match dag.parent_ids(id)?.first().cloned() {
                    Some(id) => id,
                    None => {
                        let msg = format!(
                            concat!(
                                "Cannot resolve x~(n+i) (x = {:?} {}, n = {}, i = {}) locally. ",
                                "This indicates the client-side graph is somewhat incompatible ",
                                "from the server-side graph. Something (server-side or ",
                                "client-side) was probably seriously wrong before this error."
                            ),
                            &path.x, x_id, path.n, i
                        );
                        return programming(msg);
                    }
                }
            }

            tracing::trace!(" resolved {:?} = {:?}", id, &name,);
            to_insert.push((id, name.clone()));
        }
    }
    Ok(to_insert)
}

// The server Dag. IdMap is complete. Provide APIs for client Dag to resolve vertexes.
// Currently mainly used for testing purpose.
#[async_trait::async_trait]
impl<IS, M, P, S> RemoteIdConvertProtocol for AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: IdConvert + TryClone + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Send + Sync + 'static,
{
    async fn resolve_names_to_relative_paths(
        &self,
        heads: Vec<Vertex>,
        names: Vec<Vertex>,
    ) -> Result<Vec<(AncestorPath, Vec<Vertex>)>> {
        let request = protocol::RequestNameToLocation { names, heads };
        let response: protocol::ResponseIdNamePair =
            (self.map(), self.dag()).process(request).await?;
        Ok(response.path_names)
    }

    async fn resolve_relative_paths_to_names(
        &self,
        paths: Vec<AncestorPath>,
    ) -> Result<Vec<(AncestorPath, Vec<Vertex>)>> {
        let request = protocol::RequestLocationToName { paths };
        let response: protocol::ResponseIdNamePair =
            (self.map(), self.dag()).process(request).await?;
        Ok(response.path_names)
    }
}

// On "snapshot".
#[async_trait::async_trait]
impl<IS, M, P, S> RemoteIdConvertProtocol for Arc<AbstractDag<IdDag<IS>, M, P, S>>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: IdConvert + TryClone + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Send + Sync + 'static,
{
    async fn resolve_names_to_relative_paths(
        &self,
        heads: Vec<Vertex>,
        names: Vec<Vertex>,
    ) -> Result<Vec<(AncestorPath, Vec<Vertex>)>> {
        self.deref()
            .resolve_names_to_relative_paths(heads, names)
            .await
    }

    async fn resolve_relative_paths_to_names(
        &self,
        paths: Vec<AncestorPath>,
    ) -> Result<Vec<(AncestorPath, Vec<Vertex>)>> {
        self.deref().resolve_relative_paths_to_names(paths).await
    }
}

// Dag operations. Those are just simple wrappers around [`IdDag`].
// See [`IdDag`] for the actual implementations of these algorithms.

/// DAG related read-only algorithms.
#[async_trait::async_trait]
impl<IS, M, P, S> DagAlgorithm for AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone + 'static,
    M: TryClone + IdConvert + Sync + Send + 'static,
    P: TryClone + Sync + Send + 'static,
    S: TryClone + Sync + Send + 'static,
{
    /// Sort a `Set` topologically.
    async fn sort(&self, set: &Set) -> Result<Set> {
        let hints = set.hints();
        if hints.contains(Flags::TOPO_DESC)
            && matches!(hints.dag_version(), Some(v) if v <= self.dag_version())
            && matches!(hints.id_map_version(), Some(v) if v <= self.map_version())
        {
            tracing::debug!(target: "dag::algo::sort", "sort({:6?}) (fast path)", set);
            return Ok(set.clone());
        } else if let Some(flat_set) = set.specialized_flatten_id() {
            let dag_version = flat_set.dag.dag_version();
            if dag_version <= self.dag_version() {
                let mut flat_set = flat_set.into_owned();
                flat_set.set_iteration_order(BasicIterationOrder::Desc);
                flat_set.map = self.id_map_snapshot()?;
                flat_set.dag = self.dag_snapshot()?;
                tracing::debug!(target: "dag::algo::sort", "sort({:6?}) (fast path 2)", set);
                return Ok(Set::from_query(flat_set));
            } else {
                tracing::info!(target: "dag::algo::sort", "sort({:6?}) (cannot use fast path 2 due to mismatched version)", set);
            }
        }
        tracing::warn!(target: "dag::algo::sort", "sort({:6?}) (slow path)", set);
        self.internal_stats
            .sort_slow_path_count
            .fetch_add(1, Ordering::Release);
        let flags = extract_ancestor_flag_if_compatible(set.hints(), self.dag_version());
        let mut spans = IdSet::empty();
        let mut iter = set.iter().await?.chunks(1 << 17);
        while let Some(names) = iter.next().await {
            let names = names.into_iter().collect::<Result<Vec<_>>>()?;
            let ids = self.vertex_id_batch(&names).await?;
            for id in ids {
                spans.push(id?);
            }
        }
        let result = Set::from_id_set_dag(spans, self)?;
        result.hints().add_flags(flags);
        Ok(result)
    }

    /// Get ordered parent vertexes.
    async fn parent_names(&self, name: Vertex) -> Result<Vec<Vertex>> {
        let id = self.vertex_id(name).await?;
        let parent_ids = self.dag().parent_ids(id)?;
        let mut result = Vec::with_capacity(parent_ids.len());
        for id in parent_ids {
            result.push(self.vertex_name(id).await?);
        }
        Ok(result)
    }

    /// Returns a set that covers all vertexes tracked by this DAG.
    /// Excluding the virtual group.
    async fn all(&self) -> Result<Set> {
        let spans = self.dag().all()?;
        let result = Set::from_id_set_dag(spans, self)?;
        result.hints().add_flags(Flags::FULL);
        Ok(result)
    }

    /// Returns a set that covers all vertexes in the master group.
    async fn master_group(&self) -> Result<Set> {
        let spans = self.dag().master_group()?;
        let result = Set::from_id_set_dag(spans, self)?;
        result.hints().add_flags(Flags::ANCESTORS);
        Ok(result)
    }

    /// Returns a set that covers all vertexes in the virtual group.
    async fn virtual_group(&self) -> Result<Set> {
        let spans = self.dag().all_ids_in_groups(&[Group::VIRTUAL])?;
        let result = Set::from_id_set_dag(spans, self)?;
        Ok(result)
    }

    /// Calculates all ancestors reachable from any name from the given set.
    async fn ancestors(&self, set: Set) -> Result<Set> {
        if set.hints().contains(Flags::ANCESTORS)
            && set.hints().dag_version() <= Some(self.dag_version())
        {
            return Ok(set);
        }
        let spans = self.to_id_set(&set).await?;
        let spans = self.dag().ancestors(spans)?;
        let result = Set::from_id_set_dag(spans, self)?;
        result.hints().add_flags(Flags::ANCESTORS);
        Ok(result)
    }

    /// Like `ancestors` but follows only the first parents.
    async fn first_ancestors(&self, set: Set) -> Result<Set> {
        // If set == ancestors(set), then first_ancestors(set) == set.
        if set.hints().contains(Flags::ANCESTORS)
            && set.hints().dag_version() <= Some(self.dag_version())
        {
            return Ok(set);
        }
        let spans = self.to_id_set(&set).await?;
        let spans = self.dag().first_ancestors(spans)?;
        let result = Set::from_id_set_dag(spans, self)?;
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::first_ancestors(self, set).await?);
        }
        Ok(result)
    }

    /// Calculate merges within the given set.
    async fn merges(&self, set: Set) -> Result<Set> {
        let spans = self.to_id_set(&set).await?;
        let spans = self.dag().merges(spans)?;
        let result = Set::from_id_set_dag(spans, self)?;
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::merges(self, set).await?);
        }
        Ok(result)
    }

    /// Calculates parents of the given set.
    ///
    /// Note: Parent order is not preserved. Use [`Dag::parent_names`]
    /// to preserve order.
    async fn parents(&self, set: Set) -> Result<Set> {
        // Preserve ANCESTORS flag. If ancestors(x) == x, then ancestors(parents(x)) == parents(x).
        let flags = extract_ancestor_flag_if_compatible(set.hints(), self.dag_version());
        let spans = self.dag().parents(self.to_id_set(&set).await?)?;
        let result = Set::from_id_set_dag(spans, self)?;
        result.hints().add_flags(flags);
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::parents(self, set).await?);
        }
        Ok(result)
    }

    /// Calculates the n-th first ancestor.
    async fn first_ancestor_nth(&self, name: Vertex, n: u64) -> Result<Option<Vertex>> {
        #[cfg(test)]
        let name2 = name.clone();
        let id = self.vertex_id(name).await?;
        let id = self.dag().try_first_ancestor_nth(id, n)?;
        let result = match id {
            None => None,
            Some(id) => Some(self.vertex_name(id).await?),
        };
        #[cfg(test)]
        {
            let result2 = crate::default_impl::first_ancestor_nth(self, name2, n).await?;
            assert_eq!(result, result2);
        }
        Ok(result)
    }

    /// Calculates heads of the given set.
    async fn heads(&self, set: Set) -> Result<Set> {
        if set.hints().contains(Flags::ANCESTORS)
            && set.hints().dag_version() <= Some(self.dag_version())
        {
            // heads_ancestors is faster.
            return self.heads_ancestors(set).await;
        }
        let spans = self.dag().heads(self.to_id_set(&set).await?)?;
        let result = Set::from_id_set_dag(spans, self)?;
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::heads(self, set).await?);
        }
        Ok(result)
    }

    /// Calculates children of the given set.
    async fn children(&self, set: Set) -> Result<Set> {
        let spans = self.dag().children(self.to_id_set(&set).await?)?;
        let result = Set::from_id_set_dag(spans, self)?;
        Ok(result)
    }

    /// Calculates roots of the given set.
    async fn roots(&self, set: Set) -> Result<Set> {
        let flags = extract_ancestor_flag_if_compatible(set.hints(), self.dag_version());
        let spans = self.dag().roots(self.to_id_set(&set).await?)?;
        let result = Set::from_id_set_dag(spans, self)?;
        result.hints().add_flags(flags);
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::roots(self, set).await?);
        }
        Ok(result)
    }

    /// Calculates one "greatest common ancestor" of the given set.
    ///
    /// If there are no common ancestors, return None.
    /// If there are multiple greatest common ancestors, pick one arbitrarily.
    /// Use `gca_all` to get all of them.
    async fn gca_one(&self, set: Set) -> Result<Option<Vertex>> {
        let result: Option<Vertex> = match self.dag().gca_one(self.to_id_set(&set).await?)? {
            None => None,
            Some(id) => Some(self.vertex_name(id).await?),
        };
        #[cfg(test)]
        {
            assert_eq!(&result, &crate::default_impl::gca_one(self, set).await?);
        }
        Ok(result)
    }

    /// Calculates all "greatest common ancestor"s of the given set.
    /// `gca_one` is faster if an arbitrary answer is ok.
    async fn gca_all(&self, set: Set) -> Result<Set> {
        let spans = self.dag().gca_all(self.to_id_set(&set).await?)?;
        let result = Set::from_id_set_dag(spans, self)?;
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::gca_all(self, set).await?);
        }
        Ok(result)
    }

    /// Calculates all common ancestors of the given set.
    async fn common_ancestors(&self, set: Set) -> Result<Set> {
        let spans = self.dag().common_ancestors(self.to_id_set(&set).await?)?;
        let result = Set::from_id_set_dag(spans, self)?;
        result.hints().add_flags(Flags::ANCESTORS);
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::common_ancestors(self, set).await?);
        }
        Ok(result)
    }

    /// Tests if `ancestor` is an ancestor of `descendant`.
    async fn is_ancestor(&self, ancestor: Vertex, descendant: Vertex) -> Result<bool> {
        #[cfg(test)]
        let result2 =
            crate::default_impl::is_ancestor(self, ancestor.clone(), descendant.clone()).await?;
        let ancestor_id = self.vertex_id(ancestor).await?;
        let descendant_id = self.vertex_id(descendant).await?;
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
    async fn heads_ancestors(&self, set: Set) -> Result<Set> {
        let spans = self.dag().heads_ancestors(self.to_id_set(&set).await?)?;
        let result = Set::from_id_set_dag(spans, self)?;
        #[cfg(test)]
        {
            // default_impl::heads_ancestors calls `heads` if `Flags::ANCESTORS`
            // is set. Prevent infinite loop.
            if !set.hints().contains(Flags::ANCESTORS) {
                result.assert_eq(crate::default_impl::heads_ancestors(self, set).await?);
            }
        }
        Ok(result)
    }

    /// Calculates the "dag range" - vertexes reachable from both sides.
    async fn range(&self, roots: Set, heads: Set) -> Result<Set> {
        let roots = self.to_id_set(&roots).await?;
        let heads = self.to_id_set(&heads).await?;
        let spans = self.dag().range(roots, heads)?;
        let result = Set::from_id_set_dag(spans, self)?;
        Ok(result)
    }

    /// Calculates the descendants of the given set.
    async fn descendants(&self, set: Set) -> Result<Set> {
        let spans = self.dag().descendants(self.to_id_set(&set).await?)?;
        let result = Set::from_id_set_dag(spans, self)?;
        Ok(result)
    }

    async fn suggest_bisect(
        &self,
        roots: Set,
        heads: Set,
        skip: Set,
    ) -> Result<(Option<Vertex>, Set, Set)> {
        default_impl::suggest_bisect(self, roots, heads, skip).await
    }

    /// Vertexes buffered in memory, not yet written to disk.
    async fn dirty(&self) -> Result<Set> {
        let all = self.dag().all()?;
        let spans = all.difference(&self.persisted_id_set);
        let set = Set::from_id_set_dag(spans, self)?;
        Ok(set)
    }

    fn is_vertex_lazy(&self) -> bool {
        !self.remote_protocol.is_local()
    }

    /// Get a snapshot of the current graph.
    fn dag_snapshot(&self) -> Result<Arc<dyn DagAlgorithm + Send + Sync>> {
        Ok(self.try_snapshot()? as Arc<dyn DagAlgorithm + Send + Sync>)
    }

    fn id_dag_snapshot(&self) -> Result<Arc<dyn IdDagAlgorithm + Send + Sync>> {
        let store = self.dag.try_clone()?.store;
        Ok(Arc::new(store))
    }

    fn dag_id(&self) -> &str {
        &self.id
    }

    fn dag_version(&self) -> &VerLink {
        self.dag.version()
    }
}

/// Extract the ANCESTORS flag if the set with the `hints` is bound to a
/// compatible DAG.
fn extract_ancestor_flag_if_compatible(hints: &Hints, dag_version: &VerLink) -> Flags {
    if hints.dag_version() <= Some(dag_version) {
        hints.flags() & Flags::ANCESTORS
    } else {
        Flags::empty()
    }
}

#[async_trait::async_trait]
impl<I, M, P, S> PrefixLookup for AbstractDag<I, M, P, S>
where
    I: Send + Sync,
    M: PrefixLookup + Send + Sync,
    P: Send + Sync,
    S: Send + Sync,
{
    async fn vertexes_by_hex_prefix(&self, hex_prefix: &[u8], limit: usize) -> Result<Vec<Vertex>> {
        let mut list = self.map.vertexes_by_hex_prefix(hex_prefix, limit).await?;
        let overlay_list = self
            .overlay_map
            .read()
            .unwrap()
            .lookup_vertexes_by_hex_prefix(hex_prefix, limit)?;
        list.extend(overlay_list);
        list.sort_unstable();
        list.dedup();
        list.truncate(limit);
        Ok(list)
    }
}

#[async_trait::async_trait]
impl<IS, M, P, S> IdConvert for AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: IdConvert + TryClone + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Send + Sync + 'static,
{
    async fn vertex_id(&self, name: Vertex) -> Result<Id> {
        match self.map.vertex_id(name.clone()).await {
            Ok(id) => Ok(id),
            Err(crate::Error::VertexNotFound(_)) if self.is_vertex_lazy() => {
                if let Some(id) = self.overlay_map.read().unwrap().lookup_vertex_id(&name) {
                    return Ok(id);
                }
                if self
                    .missing_vertexes_confirmed_by_remote
                    .read()
                    .unwrap()
                    .contains(&name)
                {
                    return name.not_found();
                }
                let ids = self.resolve_vertexes_remotely(&[name.clone()]).await?;
                if let Some(Some(id)) = ids.first() {
                    Ok(*id)
                } else {
                    // ids is empty.
                    name.not_found()
                }
            }
            Err(e) => Err(e),
        }
    }

    async fn vertex_id_with_max_group(
        &self,
        name: &Vertex,
        max_group: Group,
    ) -> Result<Option<Id>> {
        match self.map.vertex_id_with_max_group(name, max_group).await {
            Ok(Some(id)) => Ok(Some(id)),
            Err(err) => Err(err),
            Ok(None) if self.is_vertex_lazy() => {
                // Not exist in max_group from local data.
                if let Some(id) = self.overlay_map.read().unwrap().lookup_vertex_id(name) {
                    return Ok(Some(id));
                }
                if self
                    .missing_vertexes_confirmed_by_remote
                    .read()
                    .unwrap()
                    .contains(name)
                {
                    return Ok(None);
                }
                if max_group != Group::MAX
                    && self
                        .map
                        .vertex_id_with_max_group(name, Group::MAX)
                        .await?
                        .is_some()
                {
                    // If the vertex exists in the non-master groups. Then it must be missing in the
                    // master group.
                    return Ok(None);
                }
                match self.resolve_vertexes_remotely(&[name.clone()]).await {
                    Ok(ids) => match ids.first() {
                        Some(Some(id)) => Ok(Some(*id)),
                        Some(None) | None => Ok(None),
                    },
                    Err(e) => Err(e),
                }
            }
            Ok(None) => Ok(None),
        }
    }

    async fn vertex_name(&self, id: Id) -> Result<Vertex> {
        match self.map.vertex_name(id).await {
            Ok(name) => Ok(name),
            Err(crate::Error::IdNotFound(_)) if self.is_vertex_lazy() => {
                if let Some(name) = self
                    .overlay_map
                    .read()
                    .unwrap()
                    .lookup_vertex_name(id)
                    .cloned()
                {
                    return Ok(name);
                }
                // Only ids <= max(MASTER group) can be lazy.
                let max_master_id = self.dag.master_group()?.max();
                if Some(id) > max_master_id {
                    return id.not_found();
                }
                let names = self.resolve_ids_remotely(&[id]).await?;
                if let Some(name) = names.into_iter().next() {
                    Ok(name)
                } else {
                    id.not_found()
                }
            }
            Err(e) => Err(e),
        }
    }

    async fn contains_vertex_name(&self, name: &Vertex) -> Result<bool> {
        match self.map.contains_vertex_name(name).await {
            Ok(true) => Ok(true),
            Ok(false) if self.is_vertex_lazy() => {
                if self
                    .overlay_map
                    .read()
                    .unwrap()
                    .lookup_vertex_id(name)
                    .is_some()
                {
                    return Ok(true);
                }
                if self
                    .missing_vertexes_confirmed_by_remote
                    .read()
                    .unwrap()
                    .contains(name)
                {
                    return Ok(false);
                }
                match self.resolve_vertexes_remotely(&[name.clone()]).await {
                    Ok(ids) => match ids.first() {
                        Some(Some(_)) => Ok(true),
                        Some(None) | None => Ok(false),
                    },
                    Err(e) => Err(e),
                }
            }
            Ok(false) => Ok(false),
            Err(e) => Err(e),
        }
    }

    async fn contains_vertex_id_locally(&self, ids: &[Id]) -> Result<Vec<bool>> {
        let mut list = self.map.contains_vertex_id_locally(ids).await?;
        let map = self.overlay_map.read().unwrap();
        for (b, id) in list.iter_mut().zip(ids.iter().copied()) {
            if !*b {
                *b = *b || map.has_vertex_id(id);
            }
        }
        Ok(list)
    }

    async fn contains_vertex_name_locally(&self, names: &[Vertex]) -> Result<Vec<bool>> {
        tracing::trace!("contains_vertex_name_locally names: {:?}", &names);
        let mut list = self.map.contains_vertex_name_locally(names).await?;
        tracing::trace!("contains_vertex_name_locally list (local): {:?}", &list);
        assert_eq!(list.len(), names.len());
        let map = self.overlay_map.read().unwrap();
        for (b, name) in list.iter_mut().zip(names.iter()) {
            if !*b && map.has_vertex_name(name) {
                tracing::trace!("contains_vertex_name_locally overlay has {:?}", &name);
                *b = true;
            }
        }
        Ok(list)
    }

    async fn vertex_name_batch(&self, ids: &[Id]) -> Result<Vec<Result<Vertex>>> {
        let mut list = self.map.vertex_name_batch(ids).await?;
        if self.is_vertex_lazy() {
            // Read from overlay map cache.
            {
                let map = self.overlay_map.read().unwrap();
                for (r, id) in list.iter_mut().zip(ids) {
                    if let Some(name) = map.lookup_vertex_name(*id).cloned() {
                        *r = Ok(name);
                    }
                }
            }
            // Read from missing_vertexes_confirmed_by_remote cache.
            let missing_indexes: Vec<usize> = {
                let max_master_id = self.dag.master_group()?.max();
                list.iter()
                    .enumerate()
                    .filter_map(|(i, r)| match r {
                        // Only resolve ids that are <= max(master) remotely.
                        Err(_) if Some(ids[i]) <= max_master_id => Some(i),
                        Err(_) | Ok(_) => None,
                    })
                    .collect()
            };
            let missing_ids: Vec<Id> = missing_indexes.iter().map(|i| ids[*i]).collect();
            let resolved = self.resolve_ids_remotely(&missing_ids).await?;
            for (i, name) in missing_indexes.into_iter().zip(resolved.into_iter()) {
                list[i] = Ok(name);
            }
        }
        Ok(list)
    }

    async fn vertex_id_batch(&self, names: &[Vertex]) -> Result<Vec<Result<Id>>> {
        let mut list = self.map.vertex_id_batch(names).await?;
        if self.is_vertex_lazy() {
            // Read from overlay map cache.
            {
                let map = self.overlay_map.read().unwrap();
                for (r, name) in list.iter_mut().zip(names) {
                    if let Some(id) = map.lookup_vertex_id(name) {
                        *r = Ok(id);
                    }
                }
            }
            // Read from missing_vertexes_confirmed_by_remote cache.
            let missing_indexes: Vec<usize> = {
                let known_missing = self.missing_vertexes_confirmed_by_remote.read().unwrap();
                list.iter()
                    .enumerate()
                    .filter_map(|(i, r)| {
                        if r.is_err() && !known_missing.contains(&names[i]) {
                            Some(i)
                        } else {
                            None
                        }
                    })
                    .collect()
            };
            if !missing_indexes.is_empty() {
                let missing_names: Vec<Vertex> =
                    missing_indexes.iter().map(|i| names[*i].clone()).collect();
                let resolved = self.resolve_vertexes_remotely(&missing_names).await?;
                for (i, id) in missing_indexes.into_iter().zip(resolved.into_iter()) {
                    if let Some(id) = id {
                        list[i] = Ok(id);
                    }
                }
            }
        }
        Ok(list)
    }

    fn map_id(&self) -> &str {
        self.map.map_id()
    }

    fn map_version(&self) -> &VerLink {
        self.map.map_version()
    }
}

impl<IS, M, P, S> AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone + 'static,
    M: TryClone + Persist + IdMapWrite + IdConvert + Sync + Send + 'static,
    P: TryClone + Sync + Send + 'static,
    S: TryClone + Sync + Send + 'static,
{
    /// Build IdMap and Segments for the given heads.
    /// Update IdMap and IdDag to include the given heads and their ancestors.
    ///
    /// Handle "reassign" cases. For example, when adding P to the master group
    /// and one of its parent N2 is in the non-master group:
    ///
    /// ```plain,ignore
    ///     1--2--3             3---P
    ///         \                  /
    ///          N1-N2-N3        N2
    /// ```
    ///
    /// To maintain topological order, N2 need to be re-assigned to the master
    /// group. This is done by temporarily removing N1-N2-N3, re-insert N1-N2
    /// as 4-5 to be able to insert P, then re-insert N3 in the non-master
    /// group:
    ///
    /// ```plain,ignore
    ///     1--2--3 --6 (P)
    ///         \    /
    ///          4--5 --N1
    /// ```
    async fn build_with_lock(
        &mut self,
        parents: &dyn Parents,
        heads: &VertexListWithOptions,
        map_lock: &M::Lock,
    ) -> Result<()> {
        // `std::borrow::Cow` without `Clone` constraint.
        enum Input<'a> {
            Borrowed(&'a dyn Parents, &'a VertexListWithOptions),
            Owned(Box<dyn Parents>, VertexListWithOptions),
        }

        // Manual recursion. async fn does not support recursion.
        let mut stack = vec![Input::Borrowed(parents, heads)];

        // Avoid infinite loop (buggy logic).
        let mut loop_count = 0;

        while let Some(input) = stack.pop() {
            loop_count += 1;
            if loop_count > 2 {
                return bug("should not loop > 2 times (1st insertion+strip, 2nd reinsert)");
            }

            let (parents, heads) = match &input {
                Input::Borrowed(p, h) => (*p, *h),
                Input::Owned(p, h) => (p.as_ref(), h),
            };

            // Populate vertex negative cache to reduce round-trips doing remote lookups.
            if self.is_vertex_lazy() {
                let heads: Vec<Vertex> = heads.vertexes();
                self.populate_missing_vertexes_for_add_heads(parents, &heads)
                    .await?;
            }

            // Backup, then remove vertexes that need to be reassigned. Actual reassignment
            // happens in the next loop iteration.
            let to_reassign: Set = self.find_vertexes_to_reassign(parents, heads).await?;
            if !to_reassign.is_empty().await? {
                let reinsert_heads: VertexListWithOptions = {
                    let heads = self
                        .heads(
                            self.descendants(to_reassign.clone())
                                .await?
                                .difference(&to_reassign),
                        )
                        .await?;
                    tracing::debug!(target: "dag::reassign", "need to rebuild heads: {:?}", &heads);
                    let heads: Vec<Vertex> = heads.iter().await?.try_collect().await?;
                    VertexListWithOptions::from(heads)
                };
                let reinsert_parents: Box<dyn Parents> = Box::new(self.dag_snapshot()?);
                self.strip_with_lock(&to_reassign, map_lock).await?;

                // Rebuild non-master ids and segments on the next iteration.
                stack.push(Input::Owned(reinsert_parents, reinsert_heads));
            };

            // Update IdMap.
            let mut outcome = PreparedFlatSegments::default();
            let mut covered = self.dag().all_ids_in_groups(&Group::ALL)?;
            let mut reserved = calculate_initial_reserved(self, &covered, heads).await?;
            for group in Group::ALL {
                for (vertex, opts) in heads.vertex_options() {
                    if opts.desired_group != group {
                        continue;
                    }
                    // Important: do not call self.map.assign_head. It does not trigger
                    // remote protocol properly. Call self.assign_head instead.
                    let prepared_segments = self
                        .assign_head(vertex.clone(), parents, group, &mut covered, &reserved)
                        .await?;
                    outcome.merge(prepared_segments);
                    // Update reserved.
                    if opts.reserve_size > 0 {
                        let low = self.map.vertex_id(vertex).await? + 1;
                        update_reserved(&mut reserved, &covered, low, opts.reserve_size);
                    }
                }
            }

            // Update segments.
            self.dag
                .build_segments_from_prepared_flat_segments(&outcome)?;

            // The master group might have new vertexes inserted, which will
            // affect the `overlay_map_id_set`.
            self.update_overlay_map_id_set()?;
        }

        Ok(())
    }

    /// Find vertexes that need to be reassigned from the non-master group
    /// to the master group. That is,
    /// `ancestors(master_heads_to_insert) & existing_non_master_group`
    ///
    /// Assume pre-fetching (populate_missing_vertexes_for_add_heads)
    /// was done, so this function can just use naive DFS without worrying
    /// about excessive remote lookups.
    async fn find_vertexes_to_reassign(
        &self,
        parents: &dyn Parents,
        heads: &VertexListWithOptions,
    ) -> Result<Set> {
        // Heads that need to be inserted to the master group.
        let master_heads = heads.vertexes_by_group(Group::MASTER);

        // Visit vertexes recursively.
        let mut id_set = IdSet::empty();
        let mut to_visit: Vec<Vertex> = master_heads;
        let mut visited = HashSet::new();
        while let Some(vertex) = to_visit.pop() {
            if !visited.insert(vertex.clone()) {
                continue;
            }
            let id = self.vertex_id_optional(&vertex).await?;
            // None: The vertex/id is not yet inserted to IdMap.
            if let Some(id) = id {
                if id.group() == Group::MASTER {
                    // Already exist in the master group. Stop visiting.
                    continue;
                } else {
                    // Need reassign. Need to continue visiting.
                    id_set.push(id);
                }
            }
            let parents = parents.parent_names(vertex).await?;
            to_visit.extend(parents);
        }

        let set = Set::from_id_set_dag(id_set, self)?;
        tracing::debug!(target: "dag::reassign", "need to reassign: {:?}", &set);
        Ok(set)
    }
}

/// Calculate the initial "reserved" set used before inserting new vertexes.
/// Only heads that have non-zero reserve_size and are presnet in the graph
/// take effect. In other words, heads that are known to be not present in
/// the local graph (ex. being added), or have zero reserve_size can be
/// skipped as an optimization.
async fn calculate_initial_reserved(
    map: &dyn IdConvert,
    covered: &IdSet,
    heads: &VertexListWithOptions,
) -> Result<IdSet> {
    let mut reserved = IdSet::empty();
    for (vertex, opts) in heads.vertex_options() {
        if opts.reserve_size == 0 {
            // Avoid potentially costly remote lookup.
            continue;
        }
        if let Some(id) = map
            .vertex_id_with_max_group(&vertex, opts.desired_group)
            .await?
        {
            update_reserved(&mut reserved, covered, id + 1, opts.reserve_size);
        }
    }
    Ok(reserved)
}

fn update_reserved(reserved: &mut IdSet, covered: &IdSet, low: Id, reserve_size: u32) {
    if reserve_size == 0 {
        return;
    }
    let span = find_free_span(covered, low, reserve_size as _, true);
    reserved.push(span);
}

/// Find a span with constraints:
/// - does not overlap with `covered`.
/// - `span.low` >= the given `low`.
/// - if `shrink_to_fit` is `false`, `span.high - span.low` must be `reserve_size`.
/// - if `shrink_to_fit` is `true`, the span can be smaller than `reserve_size` to
///   fill up existing gaps in `covered`.
///
/// `reserve_size` cannot be 0.
fn find_free_span(covered: &IdSet, low: Id, reserve_size: u64, shrink_to_fit: bool) -> IdSpan {
    assert!(reserve_size > 0);
    let original_low = low;
    let mut low = low;
    let mut high;
    let mut count = 0;
    loop {
        count += 1;
        // First, bump 'low' to not overlap with a conflicted `span`.
        //   [----covered_span----]
        //      ^                  ^
        //      original_low       bump to here
        if let Some(span) = covered.span_contains(low) {
            low = span.high + 1;
        }
        high = (low + reserve_size - 1).min(low.group().max_id());
        if reserve_size <= 1 && !covered.contains(low) {
            // No need to go through complex (maybe O(N)) logic below.
            break;
        }
        // Try to reserve id..=id+reserve_size-1
        let reserved = IdSet::from_single_span(IdSpan::new(low, high));
        let intersected = reserved.intersection(covered);
        if let Some(span) = intersected.iter_span_asc().next() {
            // Overlap with existing covered spans. Decrease `high` so it
            // no longer overlap.
            if span.low > low && shrink_to_fit {
                // Use the remaining part of the previous reservation.
                //   [----------reserved--------------]
                //             [--intersected--]
                //   ^                                ^
                //   low                              high
                //            ^
                //            last_free
                //   [reserved] <- remaining of the previous reservation
                //            ^
                //            high
                let last_free = span.low - 1;
                high = last_free;
            } else {
                // No space on the left side. Try the right side.
                //   [--------reserved-------]
                //   [--intersected--]
                //   ^                       ^
                //   low                     high
                //        try next -> [------reserved------]
                //  ^                 ^
                //  last_free         low (try next)
                low = span.high + 1;
                continue;
            }
        }
        break;
    }
    if count >= 4096 {
        tracing::warn!(
            target: "dag::perf",
            count=count,
            low=?original_low,
            reserve_size=reserve_size,
            covered=?covered,
            "PERF: find_free_span took too long",
        );
    }
    let span = IdSpan::new(low, high);
    if !shrink_to_fit {
        assert_eq!(span.count(), reserve_size);
    }
    span
}

fn is_ok_some<T>(value: Result<Option<T>>) -> bool {
    match value {
        Ok(Some(_)) => true,
        _ => false,
    }
}

impl<IS, M, P, S> IdMapSnapshot for AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone + 'static,
    M: TryClone + IdConvert + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Send + Sync + 'static,
{
    fn id_map_snapshot(&self) -> Result<Arc<dyn IdConvert + Send + Sync>> {
        Ok(self.try_snapshot()? as Arc<dyn IdConvert + Send + Sync>)
    }
}

impl<IS, M, P, S> fmt::Debug for AbstractDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    M: IdConvert + Send + Sync,
    P: Send + Sync,
    S: Send + Sync,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        debug(&self.dag, &self.map, f)
    }
}

pub(crate) fn debug_segments_by_level_group<S: IdDagStore>(
    iddag: &IdDag<S>,
    idmap: &dyn IdConvert,
    level: Level,
    group: Group,
) -> Vec<String> {
    let mut result = Vec::new();
    // Show Id, with optional hash.
    let show = |id: Id| DebugId {
        id,
        name: non_blocking_result(idmap.vertex_name(id)).ok(),
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

    if let Ok(segments) = iddag.next_segments(group.min_id(), level) {
        for segment in segments.into_iter().rev() {
            if let (Ok(span), Ok(parents), Ok(flags)) =
                (segment.span(), segment.parents(), segment.flags())
            {
                let mut line = format!(
                    "{:.12?} : {:.12?} {:.12?}",
                    show(span.low),
                    show(span.high),
                    parents.into_iter().map(show).collect::<Vec<_>>(),
                );
                let flags = show_flags(flags);
                if !flags.is_empty() {
                    line += &format!(" {}", flags);
                }
                result.push(line);
            }
        }
    }
    result
}

fn debug<S: IdDagStore>(
    iddag: &IdDag<S>,
    idmap: &dyn IdConvert,
    f: &mut fmt::Formatter,
) -> fmt::Result {
    if let Ok(max_level) = iddag.max_level() {
        writeln!(f, "Max Level: {}", max_level)?;
        for lv in (0..=max_level).rev() {
            writeln!(f, " Level {}", lv)?;
            for group in Group::ALL.iter().cloned() {
                writeln!(f, "  {}:", group)?;
                if let Ok(segments) = iddag.next_segments(group.min_id(), lv) {
                    writeln!(f, "   Segments: {}", segments.len())?;
                    for line in debug_segments_by_level_group(iddag, idmap, lv, group) {
                        writeln!(f, "    {}", line)?;
                    }
                }
            }
        }
    }

    Ok(())
}

struct DebugId {
    id: Id,
    name: Option<Vertex>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_free_span_overflow() {
        let covered = IdSet::from(0..=6);
        let reserve_size = 2;
        for shrink_to_fit in [true, false] {
            let span = find_free_span(&covered, Id(0), reserve_size, shrink_to_fit);
            assert_eq!(span, IdSpan::from(7..=8));
        }
    }
}
