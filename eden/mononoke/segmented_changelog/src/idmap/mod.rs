/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod cache;
mod mem;
mod sql;
mod version;

pub use self::cache::{CacheHandlers, CachedIdMap};
pub use self::mem::{ConcurrentMemIdMap, MemIdMap};
pub use self::sql::SqlIdMap;
pub use self::version::SqlIdMapVersionStore;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{format_err, Result};
use async_trait::async_trait;

use dag::Id as Vertex;
use sql_ext::replication::ReplicaLagMonitor;
use sql_ext::SqlConnections;

use context::CoreContext;
use mononoke_types::{ChangesetId, RepositoryId};

use crate::types::IdMapVersion;

#[async_trait]
pub trait IdMap: Send + Sync {
    async fn insert_many(
        &self,
        ctx: &CoreContext,
        mappings: Vec<(Vertex, ChangesetId)>,
    ) -> Result<()>;

    async fn find_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        vertexes: Vec<Vertex>,
    ) -> Result<HashMap<Vertex, ChangesetId>>;


    async fn find_many_vertexes(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Vertex>>;

    async fn get_last_entry(&self, ctx: &CoreContext) -> Result<Option<(Vertex, ChangesetId)>>;

    // Default implementations

    async fn insert(&self, ctx: &CoreContext, vertex: Vertex, cs_id: ChangesetId) -> Result<()> {
        self.insert_many(ctx, vec![(vertex, cs_id)]).await
    }

    async fn find_changeset_id(
        &self,
        ctx: &CoreContext,
        vertex: Vertex,
    ) -> Result<Option<ChangesetId>> {
        Ok(self
            .find_many_changeset_ids(ctx, vec![vertex])
            .await?
            .remove(&vertex))
    }

    async fn find_vertex(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<Option<Vertex>> {
        Ok(self
            .find_many_vertexes(ctx, vec![cs_id])
            .await?
            .remove(&cs_id))
    }

    async fn get_changeset_id(&self, ctx: &CoreContext, vertex: Vertex) -> Result<ChangesetId> {
        self.find_changeset_id(ctx, vertex)
            .await?
            .ok_or_else(|| format_err!("Failed to find segmented changelog id {} in IdMap", vertex))
    }

    async fn get_vertex(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<Vertex> {
        self.find_vertex(ctx, cs_id)
            .await?
            .ok_or_else(|| format_err!("Failed to find changeset id {} in IdMap", cs_id))
    }
}

#[async_trait]
impl IdMap for Arc<dyn IdMap> {
    async fn insert_many(
        &self,
        ctx: &CoreContext,
        mappings: Vec<(Vertex, ChangesetId)>,
    ) -> Result<()> {
        (**self).insert_many(ctx, mappings).await
    }

    async fn find_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        vertexes: Vec<Vertex>,
    ) -> Result<HashMap<Vertex, ChangesetId>> {
        (**self).find_many_changeset_ids(ctx, vertexes).await
    }

    async fn find_many_vertexes(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Vertex>> {
        (**self).find_many_vertexes(ctx, cs_ids).await
    }

    async fn get_last_entry(&self, ctx: &CoreContext) -> Result<Option<(Vertex, ChangesetId)>> {
        (**self).get_last_entry(ctx).await
    }
}

pub struct OverlayIdMap {
    a: Arc<dyn IdMap>,
    b: Arc<dyn IdMap>,
}

impl OverlayIdMap {
    #[allow(dead_code)]
    pub fn new(a: Arc<dyn IdMap>, b: Arc<dyn IdMap>) -> Self {
        Self { a, b }
    }
}

#[async_trait]
impl IdMap for OverlayIdMap {
    async fn insert_many(
        &self,
        ctx: &CoreContext,
        mappings: Vec<(Vertex, ChangesetId)>,
    ) -> Result<()> {
        self.a.insert_many(ctx, mappings).await
    }

    async fn find_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        vertexes: Vec<Vertex>,
    ) -> Result<HashMap<Vertex, ChangesetId>> {
        let mut result = self
            .a
            .find_many_changeset_ids(ctx, vertexes.clone())
            .await?;
        let to_get_b = vertexes
            .into_iter()
            .filter(|v| !result.contains_key(&v))
            .collect();
        let from_b = self.b.find_many_changeset_ids(ctx, to_get_b).await?;
        for (v, cs) in from_b {
            result.insert(v, cs);
        }
        Ok(result)
    }

    async fn find_many_vertexes(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Vertex>> {
        let mut result = self.a.find_many_vertexes(ctx, cs_ids.clone()).await?;
        let to_get_b = cs_ids
            .into_iter()
            .filter(|cs_id| !result.contains_key(&cs_id))
            .collect();
        let from_b = self.b.find_many_vertexes(ctx, to_get_b).await?;
        for (cs, v) in from_b {
            result.insert(cs, v);
        }
        Ok(result)
    }

    async fn get_last_entry(&self, ctx: &CoreContext) -> Result<Option<(Vertex, ChangesetId)>> {
        match self.a.get_last_entry(ctx).await? {
            Some(x) => Ok(Some(x)),
            None => self.b.get_last_entry(ctx).await,
        }
    }
}

// The builder for the standard IdMap
// Our layers are: SqlIdMap, CachedIdMap, OverlayIdMap
#[derive(Clone)]
pub struct IdMapFactory {
    connections: SqlConnections,
    replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
    repo_id: RepositoryId,
    cache_handlers: Option<CacheHandlers>,
}

impl IdMapFactory {
    pub fn new(
        connections: SqlConnections,
        replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
        repo_id: RepositoryId,
    ) -> Self {
        Self {
            connections,
            replica_lag_monitor,
            repo_id,
            cache_handlers: None,
        }
    }

    // Writes go to the SQL table.
    pub fn for_writer(&self, ctx: &CoreContext, version: IdMapVersion) -> Arc<dyn IdMap> {
        let sql_idmap = SqlIdMap::new(
            self.connections.clone(),
            self.replica_lag_monitor.clone(),
            self.repo_id,
            version,
        );
        slog::debug!(
            ctx.logger(),
            "segmented changelog idmap instantiated - version: {}",
            version
        );
        let mut idmap: Arc<dyn IdMap> = Arc::new(sql_idmap);
        if let Some(cache_handlers) = &self.cache_handlers {
            idmap = Arc::new(CachedIdMap::new(
                idmap,
                cache_handlers.clone(),
                self.repo_id,
                version,
            ));
        }
        idmap
    }

    // Servers have an overlay idmap which means that all their updates to the idmap stay confined
    // to the Dag that performed the updates.
    pub fn for_server(&self, ctx: &CoreContext, version: IdMapVersion) -> Arc<dyn IdMap> {
        Arc::new(OverlayIdMap::new(
            Arc::new(ConcurrentMemIdMap::new()),
            self.for_writer(ctx, version),
        ))
    }

    pub fn with_cache_handlers(mut self, cache_handlers: CacheHandlers) -> Self {
        self.cache_handlers = Some(cache_handlers);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use maplit::hashmap;

    use fbinit::FacebookInit;

    use mononoke_types_mocks::changesetid::{AS_CSID, ONES_CSID, THREES_CSID, TWOS_CSID};

    #[fbinit::test]
    async fn test_write_a_read_ab(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        let a = Arc::new(ConcurrentMemIdMap::new());
        let b = Arc::new(ConcurrentMemIdMap::new());

        b.insert_many(&ctx, vec![(Vertex(0), AS_CSID), (Vertex(1), ONES_CSID)])
            .await?;

        let both = OverlayIdMap::new(a, b);

        assert_eq!(
            both.find_many_changeset_ids(&ctx, vec![Vertex(0), Vertex(1), Vertex(2)])
                .await?,
            hashmap![Vertex(0) => AS_CSID, Vertex(1) => ONES_CSID]
        );

        both.insert_many(&ctx, vec![(Vertex(2), TWOS_CSID), (Vertex(3), THREES_CSID)])
            .await?;
        assert_eq!(
            both.find_many_changeset_ids(&ctx, vec![Vertex(2), Vertex(3)])
                .await?,
            hashmap![Vertex(2) => TWOS_CSID, Vertex(3) => THREES_CSID]
        );

        assert_eq!(
            both.b
                .find_many_changeset_ids(&ctx, vec![Vertex(2), Vertex(3)])
                .await?,
            hashmap![]
        );
        Ok(())
    }
}
