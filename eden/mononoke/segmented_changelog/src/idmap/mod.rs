/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

mod mem;
mod sql;
mod version;

pub use self::mem::MemIdMap;
pub use self::sql::SqlIdMap;
pub use self::version::SqlIdMapVersionStore;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{format_err, Result};
use async_trait::async_trait;

use dag::Id as Vertex;

use context::CoreContext;
use mononoke_types::ChangesetId;

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

    async fn find_vertex(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<Option<Vertex>>;

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
        let result = self.find_many_changeset_ids(ctx, vec![vertex]).await?;
        Ok(result.get(&vertex).copied())
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

    async fn find_vertex(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<Option<Vertex>> {
        (**self).find_vertex(ctx, cs_id).await
    }

    async fn get_last_entry(&self, ctx: &CoreContext) -> Result<Option<(Vertex, ChangesetId)>> {
        (**self).get_last_entry(ctx).await
    }
}
