/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use context::CoreContext;
use mononoke_types::ChangesetId;
use std::future::Future;
use std::sync::Arc;

use crate::dag::errors::{self, BackendError, DagError};
use crate::dag::id::{Group, Id};
use crate::dag::idmap::IdMapWrite;
use crate::dag::ops::{IdConvert, PrefixLookup};
use crate::dag::{Result, VerLink, VertexName};
use crate::IdMap;

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
pub struct IdMapWrapper {
    verlink: VerLink,
    inner: Arc<dyn IdMap>,
    ctx: CoreContext,
}

impl IdMapWrapper {
    /// Run the given closure with a [`IdMapWrapper`] around the supplied [`IdMap`] and [`CoreContext`]
    /// This lets you use `dag` crate methods on a server `IdMap`
    pub async fn run<Fut, T>(
        ctx: CoreContext,
        idmap: Arc<dyn IdMap>,
        closure: impl FnOnce(IdMapWrapper) -> Fut,
    ) -> anyhow::Result<T>
    where
        Fut: Future<Output = anyhow::Result<T>>,
    {
        let wrapper = Self {
            verlink: VerLink::new(),
            inner: idmap,
            ctx,
        };
        closure(wrapper).await
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
            .ok_or_else(|| DagError::IdNotFound(id))
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
            .into_iter()
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
            .into_iter()
            .map(|id| {
                found
                    .get(&id)
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
            .into_iter()
            .map(|name| {
                found
                    .get(&cs_id_from_vertex_name(name))
                    .copied()
                    .ok_or(DagError::VertexNotFound(name.clone()))
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
    async fn remove_non_master(&mut self) -> Result<()> {
        // We don't handle non-master in the server
        Ok(())
    }

    async fn need_rebuild_non_master(&self) -> bool {
        // We don't handle non-master in the server
        false
    }
}
