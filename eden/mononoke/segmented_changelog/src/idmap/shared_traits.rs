/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::ChangesetId;
use slog::trace;
use stats::prelude::*;
use tunables::tunables;

use crate::dag::errors;
use crate::dag::errors::programming;
use crate::dag::errors::BackendError;
use crate::dag::errors::DagError;
use crate::dag::id::Group;
use crate::dag::id::Id;
use crate::dag::idmap::IdMapWrite;
use crate::dag::ops::IdConvert;
use crate::dag::ops::PrefixLookup;
use crate::dag::Result;
use crate::dag::VerLink;
use crate::dag::VertexName;
use crate::idmap::ConcurrentMemIdMap;
use crate::idmap::IdMapVersion;
use crate::DagId;
use crate::IdMap;

define_stats! {
    prefix = "mononoke.segmented_changelog.idmap.memory";
    insert_many: timeseries(Sum),
    flush_writes: timeseries(Sum),
}

const DEFAULT_LOG_SAMPLING_RATE: usize = 20000;

/// Type conversion - `VertexName` is used in the `dag` crate to abstract over different ID types
/// such as Mercurial IDs, Bonsai, a theoretical git ID and more.
/// but should always name a Bonsai `ChangesetId` on the server.
///
/// This converts a `VertexName` to a `ChangesetId`
///
/// # Panics
///
/// This conversion will panic if the `VertexName` is not a Bonsai `ChangesetId`
pub fn cs_id_from_vertex_name(name: &VertexName) -> ChangesetId {
    ChangesetId::from_bytes(name).expect("VertexName is not a valid ChangesetId")
}

/// Type conversion - `VertexName` is used in the `dag` crate to abstract over different ID types
/// such as Mercurial IDs, Bonsai, a theoretical git ID and more.
/// but should always name a Bonsai `ChangesetId` on the server.
///
/// This converts a `ChangesetId` to a `VertexName`
pub fn vertex_name_from_cs_id(cs_id: &ChangesetId) -> VertexName {
    VertexName::copy_from(cs_id.blake2().as_ref())
}

#[derive(Clone)]
pub struct IdMapMemWrites {
    /// The actual IdMap
    inner: Arc<dyn IdMap>,
    /// Stores recent writes that haven't yet been persisted to the backing store
    mem: Arc<ConcurrentMemIdMap>,
}

impl IdMapMemWrites {
    pub fn new(inner: Arc<dyn IdMap>) -> Self {
        Self {
            inner,
            mem: Arc::new(ConcurrentMemIdMap::new()),
        }
    }

    pub async fn flush_writes(&self, ctx: &CoreContext) -> anyhow::Result<()> {
        trace!(
            ctx.logger(),
            "flushing {} in-memory IdMap entries to SQL",
            self.mem.len(),
        );
        let writes = self.mem.drain();
        STATS::flush_writes.add_value(
            writes
                .len()
                .try_into()
                .expect("More than an i64 of writes before a flush!"),
        );
        self.inner.insert_many(ctx, writes).await
    }
}

#[async_trait]
impl IdMap for IdMapMemWrites {
    async fn insert_many(
        &self,
        ctx: &CoreContext,
        mappings: Vec<(DagId, ChangesetId)>,
    ) -> anyhow::Result<()> {
        let mappings_size = mappings.len();
        STATS::insert_many.add_value(
            mappings_size
                .try_into()
                .expect("More than an i64 of writes in one go!"),
        );
        let res = self.mem.insert_many(ctx, mappings).await;
        if res.is_ok() {
            let new_size = self.mem.len();
            let old_size = new_size - mappings_size;
            let sampling_rate = tunables().get_segmented_changelog_idmap_log_sampling_rate();
            let sampling_rate = if sampling_rate <= 0 {
                DEFAULT_LOG_SAMPLING_RATE
            } else {
                sampling_rate as usize
            };
            if new_size / sampling_rate != old_size / sampling_rate {
                trace!(
                    ctx.logger(),
                    "{} entries inserted into in-memory IdMap, new size: {}",
                    mappings_size,
                    new_size,
                );
            }
        }
        res
    }

    async fn find_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        dag_ids: Vec<DagId>,
    ) -> anyhow::Result<HashMap<DagId, ChangesetId>> {
        let mut result = self
            .mem
            .find_many_changeset_ids(ctx, dag_ids.clone())
            .await?;
        let missing: Vec<_> = dag_ids
            .iter()
            .filter(|v| !result.contains_key(v))
            .copied()
            .collect();
        if !missing.is_empty() {
            let inner_result = self.inner.find_many_changeset_ids(ctx, missing).await?;
            result.extend(inner_result);
        }
        Ok(result)
    }

    async fn find_many_dag_ids(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> anyhow::Result<HashMap<ChangesetId, DagId>> {
        let mut result = self.mem.find_many_dag_ids(ctx, cs_ids.clone()).await?;
        let missing: Vec<_> = cs_ids
            .iter()
            .filter(|id| !result.contains_key(id))
            .copied()
            .collect();
        if !missing.is_empty() {
            let inner_result = self.inner.find_many_dag_ids(ctx, missing).await?;
            result.extend(inner_result);
        }
        Ok(result)
    }

    /// Finds the dag ID for given changeset - if possible to do so quickly.
    /// Might return no answers for changesets that have dag ids assigned.
    ///
    /// Should be used by callers that can deal with missing information.
    async fn find_many_dag_ids_maybe_stale(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> anyhow::Result<HashMap<ChangesetId, DagId>> {
        let mut result = self
            .mem
            .find_many_dag_ids_maybe_stale(ctx, cs_ids.clone())
            .await?;
        let missing: Vec<_> = cs_ids
            .iter()
            .filter(|id| !result.contains_key(id))
            .copied()
            .collect();
        if !missing.is_empty() {
            let inner_result = self
                .inner
                .find_many_dag_ids_maybe_stale(ctx, missing)
                .await?;
            result.extend(inner_result);
        }
        Ok(result)
    }

    async fn get_last_entry(
        &self,
        ctx: &CoreContext,
    ) -> anyhow::Result<Option<(DagId, ChangesetId)>> {
        let mem_last = self.mem.get_last_entry(ctx).await?;
        match mem_last {
            Some(_) => return Ok(mem_last),
            None => self.inner.get_last_entry(ctx).await,
        }
    }

    fn idmap_version(&self) -> Option<IdMapVersion> {
        self.inner.idmap_version()
    }
}

/// The server needs metadata that isn't normally available in the `dag` crate
/// for normal operation.
///
/// This wrapper provides that for any `IdMap`, so that the `dag` crate
/// traits can be used on a server `IdMap`
///
/// # Performance
///
/// This function creates a new `VerLink` of `IdMap` every time for convenience.
///  `dag::NameSet` will conservatively assumes `IdMap` are totally different,
/// and conservatively avoid fast paths when it sees those different `verlink`s.
/// Practically, that means you need to avoid capturing `NameSet` produced
/// outside the `closure` to avoid performance issues. Having all `NameSet`
/// calculations inside the `closure` is fine, even if the final result is
/// passed out to the enclosing scope
#[derive(Clone)]
pub struct IdMapWrapper {
    verlink: VerLink,
    inner: IdMapMemWrites,
    ctx: CoreContext,
}

impl IdMapWrapper {
    /// Create a new wrapper around the server IdMap, using CoreContext
    /// for calling update functions
    pub fn new(ctx: CoreContext, idmap: Arc<dyn IdMap>) -> Self {
        let idmap_memwrites = IdMapMemWrites::new(idmap);
        Self {
            verlink: VerLink::new(),
            inner: idmap_memwrites,
            ctx,
        }
    }

    /// If not called, IdMap changes are discarded when this is dropped
    pub async fn flush_writes(&self) -> anyhow::Result<()> {
        self.inner.flush_writes(&self.ctx).await
    }

    /// Flushes writes and then returns the original IdMap
    pub async fn finish(self) -> anyhow::Result<Arc<dyn IdMap>> {
        self.flush_writes().await?;
        Ok(self.inner.inner)
    }

    /// Access to the inner IdMap
    pub fn as_inner(&self) -> &IdMapMemWrites {
        &self.inner
    }

    /// Get a clone of the original IdMap fed in.
    /// If `flush_writes` has not been called, this will not be updated
    pub fn clone_idmap(&self) -> Arc<dyn IdMap> {
        self.inner.inner.clone()
    }
}

#[async_trait]
impl PrefixLookup for IdMapWrapper {
    async fn vertexes_by_hex_prefix(
        &self,
        _hex_prefix: &[u8],
        _limit: usize,
    ) -> Result<Vec<VertexName>> {
        errors::programming("Server-side IdMap does not support prefix lookup")
    }
}
#[async_trait]
impl IdConvert for IdMapWrapper {
    async fn vertex_id(&self, name: VertexName) -> Result<Id> {
        // NOTE: The server implementation puts all Ids in the "master" group.
        self.vertex_id_with_max_group(&name, Group::MASTER)
            .await?
            .ok_or(DagError::VertexNotFound(name))
    }
    async fn vertex_id_with_max_group(
        &self,
        name: &VertexName,
        _max_group: Group,
    ) -> Result<Option<Id>> {
        // NOTE: The server implementation puts all Ids in the "master" group.
        let cs_id = cs_id_from_vertex_name(name);
        Ok(self
            .inner
            .find_dag_id(&self.ctx, cs_id)
            .await
            .map_err(BackendError::from)?)
    }

    async fn vertex_name(&self, id: Id) -> Result<VertexName> {
        self.inner
            .find_changeset_id(&self.ctx, id)
            .await
            .map_err(BackendError::from)?
            .map(|id| vertex_name_from_cs_id(&id))
            .ok_or(DagError::IdNotFound(id))
    }
    async fn contains_vertex_name(&self, name: &VertexName) -> Result<bool> {
        self.vertex_id_with_max_group(name, Group::MASTER)
            .await
            .map(|d| d.is_some())
    }

    async fn contains_vertex_id_locally(&self, id: &[Id]) -> Result<Vec<bool>> {
        let ids = Vec::from(id);
        let found = self
            .inner
            .find_many_changeset_ids(&self.ctx, ids)
            .await
            .map_err(BackendError::from)?;

        Ok(id.iter().map(|id| found.contains_key(id)).collect())
    }

    async fn contains_vertex_name_locally(&self, name: &[VertexName]) -> Result<Vec<bool>> {
        let cs_ids: Vec<_> = name.iter().map(cs_id_from_vertex_name).collect();
        let found = self
            .inner
            .find_many_dag_ids(&self.ctx, cs_ids)
            .await
            .map_err(BackendError::from)?;

        Ok(name
            .iter()
            .map(|name| found.contains_key(&cs_id_from_vertex_name(name)))
            .collect())
    }

    /// Convert [`Id`]s to [`VertexName`]s in batch.
    async fn vertex_name_batch(&self, id: &[Id]) -> Result<Vec<Result<VertexName>>> {
        let ids = Vec::from(id);
        let found = self
            .inner
            .find_many_changeset_ids(&self.ctx, ids)
            .await
            .map_err(BackendError::from)?;

        Ok(id
            .iter()
            .map(|id| {
                found
                    .get(id)
                    .map(vertex_name_from_cs_id)
                    .ok_or(DagError::IdNotFound(*id))
            })
            .collect())
    }

    /// Convert [`VertexName`]s to [`Id`]s in batch.
    async fn vertex_id_batch(&self, names: &[VertexName]) -> Result<Vec<Result<Id>>> {
        let cs_ids: Vec<_> = names.iter().map(cs_id_from_vertex_name).collect();

        let found = self
            .inner
            .find_many_dag_ids(&self.ctx, cs_ids)
            .await
            .map_err(BackendError::from)?;

        Ok(names
            .iter()
            .map(|name| {
                found
                    .get(&cs_id_from_vertex_name(name))
                    .copied()
                    .ok_or_else(|| DagError::VertexNotFound(name.clone()))
            })
            .collect())
    }

    fn map_id(&self) -> &str {
        "Mononoke segmented changelog"
    }

    fn map_version(&self) -> &VerLink {
        &self.verlink
    }
}

#[async_trait]
impl IdMapWrite for IdMapWrapper {
    async fn insert(&mut self, id: Id, name: &[u8]) -> Result<()> {
        // NB: This is only suitable for tailing right now, as it writes on every call
        // Eventually, this needs to use a batching interface
        let cs_id = ChangesetId::from_bytes(name).map_err(BackendError::from)?;
        Ok(self
            .inner
            .insert(&self.ctx, id, cs_id)
            .await
            .map_err(BackendError::from)?)
    }
    async fn remove_range(&mut self, low: Id, high: Id) -> Result<Vec<VertexName>> {
        let _ = (low, high);
        programming("remove_range() is not implemented server-side")
    }
}
