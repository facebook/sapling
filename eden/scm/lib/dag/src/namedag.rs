/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! # namedag
//!
//! Combination of IdMap and IdDag.

use crate::clone::CloneData;
use crate::delegate;
use crate::errors::programming;
use crate::errors::NotFoundError;
use crate::id::Group;
use crate::id::Id;
use crate::id::VertexName;
use crate::iddag::IdDag;
use crate::iddag::IdDagAlgorithm;
use crate::iddagstore::IdDagStore;
use crate::idmap::CoreMemIdMap;
use crate::idmap::IdMapAssignHead;
use crate::idmap::IdMapWrite;
use crate::locked::Locked;
use crate::nameset::hints::Flags;
use crate::nameset::hints::Hints;
use crate::nameset::NameSet;
use crate::ops::DagAddHeads;
use crate::ops::DagAlgorithm;
use crate::ops::DagExportCloneData;
use crate::ops::DagImportCloneData;
use crate::ops::DagPersistent;
use crate::ops::IdConvert;
use crate::ops::IdMapSnapshot;
use crate::ops::Open;
use crate::ops::Parents;
use crate::ops::Persist;
use crate::ops::PrefixLookup;
use crate::ops::ToIdSet;
use crate::ops::TryClone;
use crate::protocol;
use crate::protocol::AncestorPath;
use crate::protocol::Process;
use crate::protocol::RemoteIdConvertProtocol;
use crate::segment::PreparedFlatSegments;
use crate::segment::SegmentFlags;
use crate::IdSet;
use crate::Result;
use crate::VerLink;
use dag_types::FlatSegment;
use futures::future::join_all;
use futures::future::BoxFuture;
use futures::StreamExt;
use nonblocking::non_blocking_result;
use parking_lot::Mutex;
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::ops::Deref;
use std::sync::Arc;

#[cfg(any(test, feature = "indexedlog-backend"))]
mod indexedlog_namedag;
mod mem_namedag;

#[cfg(any(test, feature = "indexedlog-backend"))]
pub use indexedlog_namedag::IndexedLogNameDagPath;
#[cfg(any(test, feature = "indexedlog-backend"))]
pub use indexedlog_namedag::NameDag;
pub use mem_namedag::MemNameDag;
pub use mem_namedag::MemNameDagPath;

pub struct AbstractNameDag<I, M, P, S>
where
    I: Send + Sync,
    M: Send + Sync,
    P: Send + Sync,
    S: Send + Sync,
{
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

    /// Identity of the dag. Derived from `path`.
    id: String,

    /// Overlay IdMap. Used to store IdMap results resolved using remote
    /// protocols.
    overlay_map: Arc<RwLock<CoreMemIdMap>>,

    /// Max ID + 1 in the `overlay_map`. A protection. The `overlay_map` is
    /// shared (Arc) and its ID should not exceed the existing maximum ID at
    /// `map` open time. The IDs from 0..overlay_map_next_id are considered
    /// immutable, but lazy.
    overlay_map_next_id: Id,

    /// The source of `overlay_map`s. This avoids absolute Ids, and is
    /// used to flush overlay_map content shall the IdMap change on
    /// disk.
    overlay_map_paths: Arc<Mutex<Vec<(AncestorPath, Vec<VertexName>)>>>,

    /// Defines how to communicate with a remote service.
    /// The actual logic probably involves networking like HTTP etc
    /// and is intended to be implemented outside the `dag` crate.
    remote_protocol: Arc<dyn RemoteIdConvertProtocol>,
}

#[async_trait::async_trait]
impl<IS, M, P, S> DagPersistent for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore + Persist,
    IdDag<IS>: TryClone + 'static,
    M: TryClone + IdMapAssignHead + Persist + Send + Sync + 'static,
    P: Open<OpenTarget = Self> + Send + Sync + 'static,
    S: TryClone + Persist + Send + Sync + 'static,
{
    /// Add vertexes and their ancestors to the on-disk DAG.
    ///
    /// This is similar to calling `add_heads` followed by `flush`.
    /// But is faster.
    async fn add_heads_and_flush(
        &mut self,
        parent_names_func: &dyn Parents,
        master_names: &[VertexName],
        non_master_names: &[VertexName],
    ) -> Result<()> {
        if !self.pending_heads.is_empty() {
            return programming(format!(
                "ProgrammingError: add_heads_and_flush called with pending heads ({:?})",
                &self.pending_heads,
            ));
        }

        self.invalidate_snapshot();

        // Take lock.
        //
        // Reload meta and logs. This drops in-memory changes, which is fine because we have
        // checked there are no in-memory changes at the beginning.
        //
        // Also see comments in `NameDagState::lock()`.
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
        )
        .await?;

        // Write to disk.
        map.sync()?;
        dag.sync()?;
        locked.sync()?;

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
    async fn flush(&mut self, master_heads: &[VertexName]) -> Result<()> {
        // Sanity check.
        for head in master_heads.iter() {
            if !self.map.contains_vertex_name(head).await? {
                return head.not_found();
            }
        }

        // Write cached IdMap to disk.
        self.flush_cached_idmap().await?;

        // Constructs a new graph so we can copy pending data from the existing graph.
        let mut new_name_dag: Self = self.path.open()?;
        let parents: &(dyn DagAlgorithm + Send + Sync) = self;
        let non_master_heads = &self.pending_heads;
        let seg_size = self.dag.get_new_segment_size();
        new_name_dag.dag.set_new_segment_size(seg_size);
        new_name_dag
            .add_heads_and_flush(&parents, master_heads, non_master_heads)
            .await?;
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
        let mut to_insert: Vec<(AncestorPath, Vec<VertexName>)> = Vec::new();
        std::mem::swap(&mut to_insert, &mut *self.overlay_map_paths.lock());
        if to_insert.is_empty() {
            return Ok(());
        }

        // Lock, reload from disk. Use a new state so the existing dag is not affected.
        let mut new: Self = self.path.open()?;
        let locked = new.state.prepare_filesystem_sync()?;
        let mut map = new.map.prepare_filesystem_sync()?;
        let dag = new.dag.prepare_filesystem_sync()?;

        let id_names =
            calculate_id_name_from_paths(&*map, &**dag, new.overlay_map_next_id, &to_insert)
                .await?;
        for (id, name) in id_names {
            map.insert(id, name.as_ref())?;
        }

        map.sync()?;
        dag.sync()?;
        locked.sync()?;

        Ok(())
    }
}

#[async_trait::async_trait]
impl<IS, M, P, S> DagAddHeads for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: TryClone + IdMapAssignHead + Send + Sync,
    P: TryClone + Send + Sync,
    S: TryClone + Send + Sync,
{
    /// Add vertexes and their ancestors to the in-memory DAG.
    ///
    /// This does not write to disk. Use `add_heads_and_flush` to add heads
    /// and write to disk more efficiently.
    ///
    /// The added vertexes are immediately query-able. They will get Ids
    /// assigned to the NON_MASTER group internally. The `flush` function
    /// can re-assign Ids to the MASTER group.
    async fn add_heads(&mut self, parents: &dyn Parents, heads: &[VertexName]) -> Result<()> {
        self.invalidate_snapshot();

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
        let mut outcome = PreparedFlatSegments::default();
        for head in heads.iter() {
            if !self.contains_vertex_name(head).await? {
                outcome.merge(self.assign_head(head.clone(), parents, group).await?);
                self.pending_heads.push(head.clone());
            }
        }

        // Update segments in the NON_MASTER group.
        self.dag
            .build_segments_volatile_from_prepared_flat_segments(&outcome)?;

        Ok(())
    }
}

impl<IS, M, P, S> IdMapWrite for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: TryClone + IdMapAssignHead + Send + Sync,
    P: TryClone + Send + Sync,
    S: TryClone + Send + Sync,
{
    fn insert(&mut self, id: Id, name: &[u8]) -> Result<()> {
        self.map.insert(id, name)
    }

    fn next_free_id(&self, group: Group) -> Result<Id> {
        self.map.next_free_id(group)
    }

    fn remove_non_master(&mut self) -> Result<()> {
        self.map.remove_non_master()
    }

    fn need_rebuild_non_master(&self) -> bool {
        self.map.need_rebuild_non_master()
    }
}

#[async_trait::async_trait]
impl<IS, M, P, S> DagImportCloneData for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore + Persist,
    M: IdMapAssignHead + Persist + Send + Sync,
    P: Send + Sync,
    S: Persist + Send + Sync,
{
    async fn import_clone_data(&mut self, clone_data: CloneData<VertexName>) -> Result<()> {
        // Write directly to disk. Bypassing "flush()" that re-assigns Ids
        // using parent functions.
        let locked = self.state.prepare_filesystem_sync()?;
        let mut map = self.map.prepare_filesystem_sync()?;
        let mut dag = self.dag.prepare_filesystem_sync()?;

        if !dag.all()?.is_empty() {
            return programming("Cannot import clone data for non-empty graph");
        }
        for (id, name) in clone_data.idmap {
            map.insert(id, name.as_ref())?;
        }
        dag.build_segments_volatile_from_prepared_flat_segments(&clone_data.flat_segments)?;

        // Verify that universally known vertexes and heads are present in IdMap.
        let missing: Vec<Id> = {
            let universal_ids: Vec<Id> = dag.universal_ids()?.into_iter().collect();
            tracing::debug!("clone: {} universally known vertexes", universal_ids.len());
            let exists = map.contains_vertex_id_locally(&universal_ids).await?;
            universal_ids
                .into_iter()
                .zip(exists)
                .filter_map(|(id, b)| if b { None } else { Some(id) })
                .collect()
        };
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

        let next_id = dag.next_free_id(0, Group::MASTER)?;

        map.sync()?;
        dag.sync()?;
        locked.sync()?;

        // Reset overlay map state.
        self.overlay_map = Default::default();
        self.overlay_map_next_id = next_id;

        Ok(())
    }
}

#[async_trait::async_trait]
impl<IS, M, P, S> DagExportCloneData for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: IdConvert + TryClone + Send + Sync,
    P: TryClone + Send + Sync,
    S: TryClone + Send + Sync,
{
    async fn export_clone_data(&self) -> Result<CloneData<VertexName>> {
        let head_id: Id = match self.dag.master_group()?.max() {
            Some(id) => id,
            None => {
                // If we lift the limitation, CloneData struct needs to change.
                return programming("Cannot export DAG with empty master group");
            }
        };
        assert_eq!(head_id.group(), Group::MASTER);

        let idmap: HashMap<Id, VertexName> = {
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
            PreparedFlatSegments { segments: prepared }
        };

        let data = CloneData {
            head_id,
            flat_segments,
            idmap,
        };
        Ok(data)
    }
}

impl<IS, M, P, S> AbstractNameDag<IdDag<IS>, M, P, S>
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
        *self.snapshot.write() = None;
    }

    /// Attempt to get a snapshot of this graph.
    fn try_snapshot(&self) -> Result<Arc<Self>> {
        if let Some(s) = self.snapshot.read().deref() {
            if s.dag.version() == self.dag.version() {
                return Ok(Arc::clone(s));
            }
        }

        let mut snapshot = self.snapshot.write();
        match snapshot.deref() {
            Some(s) if s.dag.version() == self.dag.version() => Ok(s.clone()),
            _ => {
                let cloned = Self {
                    dag: self.dag.try_clone()?,
                    map: self.map.try_clone()?,
                    snapshot: Default::default(),
                    pending_heads: self.pending_heads.clone(),
                    path: self.path.try_clone()?,
                    state: self.state.try_clone()?,
                    id: self.id.clone(),
                    // If we do deep clone here we can remove `overlay_map_next_id`
                    // protection. However that could be too expensive.
                    overlay_map: Arc::clone(&self.overlay_map),
                    overlay_map_next_id: self.overlay_map_next_id,
                    overlay_map_paths: Arc::clone(&self.overlay_map_paths),
                    remote_protocol: self.remote_protocol.clone(),
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

    /// Set the remote protocol for converting between Id and Vertex remotely.
    ///
    /// This is usually used on "sparse" ("lazy") Dag where the IdMap is incomplete
    /// for vertexes in the master groups.
    pub fn set_remote_protocol(&mut self, protocol: Arc<dyn RemoteIdConvertProtocol>) {
        self.remote_protocol = protocol;
    }
}

// The "client" Dag. Using a remote protocol to fill lazy part of the vertexes.
impl<IS, M, P, S> AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: IdConvert + TryClone + Send + Sync,
    P: TryClone + Send + Sync,
    S: TryClone + Send + Sync,
{
    /// Resolve vertexes remotely and cache the result in the overlay map.
    /// Return the resolved ids in the given order. Not all names are resolved.
    async fn resolve_vertexes_remotely(&self, names: &[VertexName]) -> Result<Vec<Option<Id>>> {
        if names.is_empty() {
            return Ok(Vec::new());
        }
        if names.len() < 30 {
            tracing::debug!("resolve names {:?} remotely", &names);
        } else {
            tracing::debug!("resolve names ({}) remotely", names.len());
        }
        let request: protocol::RequestNameToLocation =
            (self.map(), self.dag()).process(names.to_vec()).await?;
        let path_names = self
            .remote_protocol
            .resolve_names_to_relative_paths(request.heads, request.names)
            .await?;
        self.insert_relative_paths(path_names).await?;
        let overlay = self.overlay_map.read();
        let mut ids = Vec::with_capacity(names.len());
        for name in names {
            if let Some(id) = overlay.lookup_vertex_id(name) {
                ids.push(Some(id));
            } else {
                ids.push(None);
            }
        }
        Ok(ids)
    }

    /// Resolve ids remotely and cache the result in the overlay map.
    /// Return the resolved ids in the given order. All ids must be resolved.
    async fn resolve_ids_remotely(&self, ids: &[Id]) -> Result<Vec<VertexName>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        if ids.len() < 30 {
            tracing::debug!("resolve ids {:?} remotely", &ids);
        } else {
            tracing::debug!("resolve ids ({}) remotely", ids.len());
        }
        let request: protocol::RequestLocationToName = (self.map(), self.dag())
            .process(IdSet::from_spans(ids.iter().copied()))
            .await?;
        let path_names = self
            .remote_protocol
            .resolve_relative_paths_to_names(request.paths)
            .await?;
        self.insert_relative_paths(path_names).await?;
        let overlay = self.overlay_map.read();
        let mut names = Vec::with_capacity(ids.len());
        for &id in ids {
            if let Some(name) = overlay.lookup_vertex_name(id) {
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
        path_names: Vec<(AncestorPath, Vec<VertexName>)>,
    ) -> Result<()> {
        if path_names.is_empty() {
            return Ok(());
        }
        let to_insert: Vec<(Id, VertexName)> = calculate_id_name_from_paths(
            self.map(),
            self.dag().deref(),
            self.overlay_map_next_id,
            &path_names,
        )
        .await?;

        let mut paths = self.overlay_map_paths.lock();
        paths.extend(path_names);
        drop(paths);

        let mut overlay = self.overlay_map.write();
        for (id, name) in to_insert {
            overlay.insert_vertex_id_name(id, name);
        }

        Ok(())
    }
}

/// Calculate (id, name) pairs to insert from (path, [name]) pairs.
async fn calculate_id_name_from_paths(
    map: &dyn IdConvert,
    dag: &dyn IdDagAlgorithm,
    max_id_plus_1: Id,
    path_names: &[(AncestorPath, Vec<VertexName>)],
) -> Result<Vec<(Id, VertexName)>> {
    if path_names.is_empty() {
        return Ok(Vec::new());
    }
    let mut to_insert: Vec<(Id, VertexName)> =
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
        if x_id >= max_id_plus_1 {
            let msg = format!(
                concat!(
                    "Server returned x~n (x = {:?} {}, n = {}). But x exceeds the head in the ",
                    "local master group. This is not expected and indicates some ",
                    "logic error on the server side."
                ),
                &path.x, x_id, path.n,
            );
            return programming(msg);
        }
        let mut id = dag.first_ancestor_nth(x_id, path.n).map_err(|e| {
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
        })?;
        if names.len() < 30 {
            tracing::debug!("resolved {:?} => {} {:?}", &path, id, &names);
        } else {
            tracing::debug!("resolved {:?} => {} {:?} ...", &path, id, &names[0]);
        }
        for (i, name) in names.into_iter().enumerate() {
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
impl<IS, M, P, S> RemoteIdConvertProtocol for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: IdConvert + TryClone + Send + Sync + 'static,
    P: TryClone + Send + Sync + 'static,
    S: TryClone + Send + Sync + 'static,
{
    async fn resolve_names_to_relative_paths(
        &self,
        heads: Vec<VertexName>,
        names: Vec<VertexName>,
    ) -> Result<Vec<(AncestorPath, Vec<VertexName>)>> {
        let request = protocol::RequestNameToLocation { names, heads };
        let response: protocol::ResponseIdNamePair =
            (self.map(), self.dag()).process(request).await?;
        Ok(response.path_names)
    }

    async fn resolve_relative_paths_to_names(
        &self,
        paths: Vec<AncestorPath>,
    ) -> Result<Vec<(AncestorPath, Vec<VertexName>)>> {
        let request = protocol::RequestLocationToName { paths };
        let response: protocol::ResponseIdNamePair =
            (self.map(), self.dag()).process(request).await?;
        Ok(response.path_names)
    }
}

// Dag operations. Those are just simple wrappers around [`IdDag`].
// See [`IdDag`] for the actual implementations of these algorithms.

/// DAG related read-only algorithms.
#[async_trait::async_trait]
impl<IS, M, P, S> DagAlgorithm for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone + 'static,
    M: TryClone + IdConvert + Sync + Send + 'static,
    P: TryClone + Sync + Send + 'static,
    S: TryClone + Sync + Send + 'static,
{
    /// Sort a `NameSet` topologically.
    async fn sort(&self, set: &NameSet) -> Result<NameSet> {
        if set.hints().contains(Flags::TOPO_DESC)
            && set.hints().dag_version() <= Some(self.dag_version())
        {
            Ok(set.clone())
        } else {
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
            let result = NameSet::from_spans_dag(spans, self)?;
            result.hints().add_flags(flags);
            Ok(result)
        }
    }

    /// Get ordered parent vertexes.
    async fn parent_names(&self, name: VertexName) -> Result<Vec<VertexName>> {
        let id = self.vertex_id(name).await?;
        let parent_ids = self.dag().parent_ids(id)?;
        let mut result = Vec::with_capacity(parent_ids.len());
        for id in parent_ids {
            result.push(self.vertex_name(id).await?);
        }
        Ok(result)
    }

    /// Returns a set that covers all vertexes tracked by this DAG.
    async fn all(&self) -> Result<NameSet> {
        let spans = self.dag().all()?;
        let result = NameSet::from_spans_dag(spans, self)?;
        result.hints().add_flags(Flags::FULL);
        Ok(result)
    }

    /// Calculates all ancestors reachable from any name from the given set.
    async fn ancestors(&self, set: NameSet) -> Result<NameSet> {
        if set.hints().contains(Flags::ANCESTORS)
            && set.hints().dag_version() <= Some(self.dag_version())
        {
            return Ok(set);
        }
        let spans = self.to_id_set(&set).await?;
        let spans = self.dag().ancestors(spans)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        result.hints().add_flags(Flags::ANCESTORS);
        Ok(result)
    }

    /// Like `ancestors` but follows only the first parents.
    async fn first_ancestors(&self, set: NameSet) -> Result<NameSet> {
        // If set == ancestors(set), then first_ancestors(set) == set.
        if set.hints().contains(Flags::ANCESTORS)
            && set.hints().dag_version() <= Some(self.dag_version())
        {
            return Ok(set);
        }
        let spans = self.to_id_set(&set).await?;
        let spans = self.dag().first_ancestors(spans)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::first_ancestors(self, set).await?);
        }
        Ok(result)
    }

    /// Calculate merges within the given set.
    async fn merges(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.to_id_set(&set).await?;
        let spans = self.dag().merges(spans)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::merges(self, set).await?);
        }
        Ok(result)
    }

    /// Calculates parents of the given set.
    ///
    /// Note: Parent order is not preserved. Use [`NameDag::parent_names`]
    /// to preserve order.
    async fn parents(&self, set: NameSet) -> Result<NameSet> {
        // Preserve ANCESTORS flag. If ancestors(x) == x, then ancestors(parents(x)) == parents(x).
        let flags = extract_ancestor_flag_if_compatible(set.hints(), self.dag_version());
        let spans = self.dag().parents(self.to_id_set(&set).await?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        result.hints().add_flags(flags);
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::parents(self, set).await?);
        }
        Ok(result)
    }

    /// Calculates the n-th first ancestor.
    async fn first_ancestor_nth(&self, name: VertexName, n: u64) -> Result<Option<VertexName>> {
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
    async fn heads(&self, set: NameSet) -> Result<NameSet> {
        if set.hints().contains(Flags::ANCESTORS)
            && set.hints().dag_version() <= Some(self.dag_version())
        {
            // heads_ancestors is faster.
            return self.heads_ancestors(set).await;
        }
        let spans = self.dag().heads(self.to_id_set(&set).await?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::heads(self, set).await?);
        }
        Ok(result)
    }

    /// Calculates children of the given set.
    async fn children(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag().children(self.to_id_set(&set).await?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        Ok(result)
    }

    /// Calculates roots of the given set.
    async fn roots(&self, set: NameSet) -> Result<NameSet> {
        let flags = extract_ancestor_flag_if_compatible(set.hints(), self.dag_version());
        let spans = self.dag().roots(self.to_id_set(&set).await?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
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
    async fn gca_one(&self, set: NameSet) -> Result<Option<VertexName>> {
        let result: Option<VertexName> = match self.dag().gca_one(self.to_id_set(&set).await?)? {
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
    async fn gca_all(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag().gca_all(self.to_id_set(&set).await?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::gca_all(self, set).await?);
        }
        Ok(result)
    }

    /// Calculates all common ancestors of the given set.
    async fn common_ancestors(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag().common_ancestors(self.to_id_set(&set).await?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        result.hints().add_flags(Flags::ANCESTORS);
        #[cfg(test)]
        {
            result.assert_eq(crate::default_impl::common_ancestors(self, set).await?);
        }
        Ok(result)
    }

    /// Tests if `ancestor` is an ancestor of `descendant`.
    async fn is_ancestor(&self, ancestor: VertexName, descendant: VertexName) -> Result<bool> {
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
    async fn heads_ancestors(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag().heads_ancestors(self.to_id_set(&set).await?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
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
    async fn range(&self, roots: NameSet, heads: NameSet) -> Result<NameSet> {
        let roots = self.to_id_set(&roots).await?;
        let heads = self.to_id_set(&heads).await?;
        let spans = self.dag().range(roots, heads)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        Ok(result)
    }

    /// Calculates the descendants of the given set.
    async fn descendants(&self, set: NameSet) -> Result<NameSet> {
        let spans = self.dag().descendants(self.to_id_set(&set).await?)?;
        let result = NameSet::from_spans_dag(spans, self)?;
        Ok(result)
    }

    /// Get a snapshot of the current graph.
    fn dag_snapshot(&self) -> Result<Arc<dyn DagAlgorithm + Send + Sync>> {
        Ok(self.try_snapshot()? as Arc<dyn DagAlgorithm + Send + Sync>)
    }

    fn dag_id(&self) -> &str {
        &self.id
    }

    fn dag_version(&self) -> &VerLink {
        &self.dag.version()
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

delegate! {
    PrefixLookup {
        impl<I: Send + Sync, M: PrefixLookup + Send + Sync, P: Send + Sync, S: Send + Sync> PrefixLookup for AbstractNameDag<I, M, P, S>
    } => self.map
}

#[async_trait::async_trait]
impl<IS, M, P, S> IdConvert for AbstractNameDag<IdDag<IS>, M, P, S>
where
    IS: IdDagStore,
    IdDag<IS>: TryClone,
    M: IdConvert + TryClone + Send + Sync,
    P: TryClone + Send + Sync,
    S: TryClone + Send + Sync,
{
    async fn vertex_id(&self, name: VertexName) -> Result<Id> {
        match self.map.vertex_id(name.clone()).await {
            Ok(id) => Ok(id),
            Err(crate::Error::VertexNotFound(_)) => {
                if let Some(id) = self.overlay_map.read().lookup_vertex_id(&name) {
                    return Ok(id);
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
        name: &VertexName,
        max_group: Group,
    ) -> Result<Option<Id>> {
        match self.map.vertex_id_with_max_group(name, max_group).await {
            Ok(Some(id)) => Ok(Some(id)),
            Err(err) => Err(err),
            Ok(None) => {
                if let Some(id) = self.overlay_map.read().lookup_vertex_id(&name) {
                    return Ok(Some(id));
                }
                match self.resolve_vertexes_remotely(&[name.clone()]).await {
                    Ok(ids) => match ids.first() {
                        Some(Some(id)) => Ok(Some(*id)),
                        Some(None) | None => Ok(None),
                    },
                    Err(e) => Err(e),
                }
            }
        }
    }

    async fn vertex_name(&self, id: Id) -> Result<VertexName> {
        match self.map.vertex_name(id).await {
            Ok(name) => Ok(name),
            Err(crate::Error::IdNotFound(_)) => {
                if let Some(name) = self.overlay_map.read().lookup_vertex_name(id) {
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

    async fn contains_vertex_name(&self, name: &VertexName) -> Result<bool> {
        match self.map.contains_vertex_name(name).await {
            Ok(true) => Ok(true),
            Ok(false) => {
                if self.overlay_map.read().lookup_vertex_id(name).is_some() {
                    return Ok(true);
                }
                // PERF: Need some kind of negative cache?
                match self.resolve_vertexes_remotely(&[name.clone()]).await {
                    Ok(ids) => match ids.first() {
                        Some(Some(_)) => Ok(true),
                        Some(None) | None => Ok(false),
                    },
                    Err(e) => Err(e),
                }
            }
            Err(e) => Err(e),
        }
    }

    async fn contains_vertex_id_locally(&self, ids: &[Id]) -> Result<Vec<bool>> {
        let mut list = self.map.contains_vertex_id_locally(ids).await?;
        let map = self.overlay_map.read();
        for (b, id) in list.iter_mut().zip(ids.iter().copied()) {
            if !*b {
                *b = *b || map.has_vertex_id(id);
            }
        }
        Ok(list)
    }

    async fn contains_vertex_name_locally(&self, names: &[VertexName]) -> Result<Vec<bool>> {
        let mut list = self.map.contains_vertex_name_locally(names).await?;
        let map = self.overlay_map.read();
        for (b, name) in list.iter_mut().zip(names.iter()) {
            if !*b {
                *b = *b || map.has_vertex_name(name);
            }
        }
        Ok(list)
    }

    async fn vertex_name_batch(&self, ids: &[Id]) -> Result<Vec<Result<VertexName>>> {
        let mut list = self.map.vertex_name_batch(ids).await?;
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
        Ok(list)
    }

    async fn vertex_id_batch(&self, names: &[VertexName]) -> Result<Vec<Result<Id>>> {
        let mut list = self.map.vertex_id_batch(names).await?;
        let missing_indexes: Vec<usize> = list
            .iter()
            .enumerate()
            .filter_map(|(i, r)| if r.is_err() { Some(i) } else { None })
            .collect();
        if !missing_indexes.is_empty() {
            let missing_names: Vec<VertexName> =
                missing_indexes.iter().map(|i| names[*i].clone()).collect();
            let resolved = self.resolve_vertexes_remotely(&missing_names).await?;
            for (i, id) in missing_indexes.into_iter().zip(resolved.into_iter()) {
                if let Some(id) = id {
                    list[i] = Ok(id);
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

/// Export non-master DAG as parent_names_func on HashMap.
///
/// This can be expensive. It is expected to be either called infrequently,
/// or called with a small amount of data. For example, bounded amount of
/// non-master commits.
async fn non_master_parent_names<M, S>(
    map: &Locked<'_, M>,
    dag: &Locked<'_, IdDag<S>>,
) -> Result<HashMap<VertexName, Vec<VertexName>>>
where
    M: IdConvert + Persist,
    S: IdDagStore + Persist,
{
    let parent_ids = dag.non_master_parent_ids()?;
    // PERF: This is suboptimal async iteration. It might be okay if non-master
    // part is not lazy.
    //
    // Map id to name.
    let mut parent_names_map = HashMap::with_capacity(parent_ids.len());
    for (id, parent_ids) in parent_ids.into_iter() {
        let name = map.vertex_name(id).await?;
        let parent_names = join_all(parent_ids.into_iter().map(|p| map.vertex_name(p)))
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        parent_names_map.insert(name, parent_names);
    }
    Ok(parent_names_map)
}

/// Re-assign ids and segments for non-master group.
pub fn rebuild_non_master<'a: 's, 'b: 's, 's, M, S>(
    map: &'a mut Locked<M>,
    dag: &'b mut Locked<IdDag<S>>,
) -> BoxFuture<'s, Result<()>>
where
    M: IdMapAssignHead + Persist,
    S: IdDagStore + Persist,
    M: Send,
{
    let fut = async move {
        // backup part of the named graph in memory.
        let parents = non_master_parent_names(map, dag).await?;
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
        build(map, dag, &parents, &[], &heads[..]).await?;

        Ok(())
    };
    Box::pin(fut)
}

/// Build IdMap and Segments for the given heads.
pub async fn build<IS, M>(
    map: &mut Locked<'_, M>,
    dag: &mut Locked<'_, IdDag<IS>>,
    parent_names_func: &dyn Parents,
    master_heads: &[VertexName],
    non_master_heads: &[VertexName],
) -> Result<()>
where
    IS: IdDagStore + Persist,
    M: IdMapAssignHead + Persist,
    M: Send,
{
    // Update IdMap.
    let mut outcome = PreparedFlatSegments::default();
    for (nodes, group) in [
        (master_heads, Group::MASTER),
        (non_master_heads, Group::NON_MASTER),
    ]
    .iter()
    {
        for node in nodes.iter() {
            outcome.merge(
                map.assign_head(node.clone(), parent_names_func, *group)
                    .await?,
            );
        }
    }

    // Update segments.
    dag.build_segments_volatile_from_prepared_flat_segments(&outcome)?;

    // Rebuild non-master ids and segments.
    if map.need_rebuild_non_master() {
        rebuild_non_master(map, dag).await?;
    }

    Ok(())
}

fn is_ok_some<T>(value: Result<Option<T>>) -> bool {
    match value {
        Ok(Some(_)) => true,
        _ => false,
    }
}

impl<IS, M, P, S> IdMapSnapshot for AbstractNameDag<IdDag<IS>, M, P, S>
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

impl<IS, M, P, S> fmt::Debug for AbstractNameDag<IdDag<IS>, M, P, S>
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

fn debug<S: IdDagStore>(
    iddag: &IdDag<S>,
    idmap: &dyn IdConvert,
    f: &mut fmt::Formatter,
) -> fmt::Result {
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
