/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::sync::Arc;

use abomonation_derive::Abomonation;
use anyhow::{Context, Result};
use async_trait::async_trait;
use bytes::Bytes;

use caching_ext::{
    get_or_fill, CacheDisposition, CacheTtl, CachelibHandler, EntityStore, KeyedEntityStore,
    MemcacheEntity, MemcacheHandler,
};
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future::{self, TryFutureExt};
use memcache::{KeyGen, MemcacheClient};
use mononoke_types::{ChangesetId, RepositoryId};

use dag::Id as Vertex;

use crate::idmap::IdMap;
use crate::types::IdMapVersion;

#[derive(Clone)]
pub struct CachedIdMap {
    idmap: Arc<dyn IdMap>,
    cache_handlers: CacheHandlers,
    repo_id: RepositoryId,
    keygen: KeyGen,
}

#[derive(Clone)]
pub struct CacheHandlers {
    pub dag_to_cs: CachelibHandler<ChangesetIdWrapper>,
    pub cs_to_dag: CachelibHandler<VertexWrapper>,
    pub memcache: MemcacheHandler,
}

impl CacheHandlers {
    pub fn new(
        dag_to_cs: CachelibHandler<ChangesetIdWrapper>,
        cs_to_dag: CachelibHandler<VertexWrapper>,
        memcache: MemcacheHandler,
    ) -> Self {
        Self {
            dag_to_cs,
            cs_to_dag,
            memcache,
        }
    }

    pub fn prod(fb: FacebookInit, cache_pool: cachelib::VolatileLruCachePool) -> Self {
        Self {
            dag_to_cs: cache_pool.clone().into(),
            cs_to_dag: cache_pool.into(),
            memcache: MemcacheClient::new(fb)
                .expect("Memcache initialization failed")
                .into(),
        }
    }
}

impl CachedIdMap {
    pub fn new(
        idmap: Arc<dyn IdMap>,
        cache_handlers: CacheHandlers,
        repo_id: RepositoryId,
        version: IdMapVersion,
    ) -> Self {
        let codever = 0; // bump when logic changes
        let sitever = version.0 as u32;

        let keygen = KeyGen::new("scm.mononoke.segmented_changelog.idmap", codever, sitever);
        Self {
            idmap,
            cache_handlers,
            repo_id,
            keygen,
        }
    }
}

#[async_trait]
impl IdMap for CachedIdMap {
    async fn insert_many(
        &self,
        ctx: &CoreContext,
        mappings: Vec<(Vertex, ChangesetId)>,
    ) -> Result<()> {
        self.idmap.insert_many(ctx, mappings).await?;
        Ok(())
    }

    async fn find_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        vertexes: Vec<Vertex>,
    ) -> Result<HashMap<Vertex, ChangesetId>> {
        let ctx = (ctx, self);
        let res = get_or_fill(ctx, vertexes.into_iter().collect())
            .await
            .with_context(|| "Error fetching many changeset ids via cache")?
            .into_iter()
            .map(|(k, v)| (k, v.0))
            .collect();
        Ok(res)
    }

    async fn find_many_vertexes(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Vertex>> {
        let ctx = (ctx, self);
        let res = get_or_fill(ctx, cs_ids.into_iter().collect())
            .await
            .with_context(|| "Error fetching many changeset ids via cache")?
            .into_iter()
            .map(|(k, v)| (k, v.0))
            .collect();
        Ok(res)
    }

    async fn get_last_entry(&self, ctx: &CoreContext) -> Result<Option<(Vertex, ChangesetId)>> {
        self.idmap.get_last_entry(ctx).await
    }
}

type CacheRequest<'a> = (&'a CoreContext, &'a CachedIdMap);

#[derive(Clone, Copy, Debug, Abomonation)]
pub struct ChangesetIdWrapper(ChangesetId);

impl From<ChangesetId> for ChangesetIdWrapper {
    fn from(cs_id: ChangesetId) -> Self {
        ChangesetIdWrapper(cs_id)
    }
}

impl MemcacheEntity for ChangesetIdWrapper {
    fn serialize(&self) -> Bytes {
        Bytes::copy_from_slice(self.0.as_ref())
    }

    fn deserialize(bytes: Bytes) -> Result<Self, ()> {
        match ChangesetId::from_bytes(&bytes) {
            Ok(cs_id) => Ok(ChangesetIdWrapper(cs_id)),
            Err(_) => Err(()),
        }
    }
}

impl EntityStore<ChangesetIdWrapper> for CacheRequest<'_> {
    fn cachelib(&self) -> &CachelibHandler<ChangesetIdWrapper> {
        let (_, bag) = self;
        &bag.cache_handlers.dag_to_cs
    }

    fn keygen(&self) -> &KeyGen {
        let (_, bag) = self;
        &bag.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        let (_, bag) = self;
        &bag.cache_handlers.memcache
    }

    fn cache_determinator(&self, _: &ChangesetIdWrapper) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::NoTtl)
    }

    caching_ext::impl_singleton_stats!("segmented_changelog.idmap.dag2cs");
}

#[async_trait]
impl KeyedEntityStore<Vertex, ChangesetIdWrapper> for CacheRequest<'_> {
    fn get_cache_key(&self, vertex: &Vertex) -> String {
        let (_, bag) = self;
        format!("{}.vertex.{}", bag.repo_id, vertex)
    }

    async fn get_from_db(
        &self,
        keys: HashSet<Vertex>,
    ) -> Result<HashMap<Vertex, ChangesetIdWrapper>> {
        let (ctx, bag) = self;

        let res = bag
            .idmap
            .find_many_changeset_ids(ctx, keys.into_iter().collect())
            .await?
            .into_iter()
            .map(|(k, v)| (k, v.into()))
            .collect();

        Ok(res)
    }
}

#[derive(Clone, Copy, Debug, Abomonation)]
pub struct VertexWrapper(Vertex);

impl From<Vertex> for VertexWrapper {
    fn from(vertex: Vertex) -> Self {
        VertexWrapper(vertex)
    }
}

impl MemcacheEntity for VertexWrapper {
    fn serialize(&self) -> Bytes {
        Bytes::copy_from_slice(&self.0.0.to_be_bytes())
    }

    fn deserialize(bytes: Bytes) -> Result<Self, ()> {
        let arr = bytes.as_ref().try_into().map_err(|_| ())?;
        Ok(VertexWrapper(Vertex(u64::from_be_bytes(arr))))
    }
}

impl EntityStore<VertexWrapper> for CacheRequest<'_> {
    fn cachelib(&self) -> &CachelibHandler<VertexWrapper> {
        let (_, bag) = self;
        &bag.cache_handlers.cs_to_dag
    }

    fn keygen(&self) -> &KeyGen {
        let (_, bag) = self;
        &bag.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        let (_, bag) = self;
        &bag.cache_handlers.memcache
    }

    fn cache_determinator(&self, _: &VertexWrapper) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::NoTtl)
    }

    caching_ext::impl_singleton_stats!("segmented_changelog.idmap.cs2dag");
}

#[async_trait]
impl KeyedEntityStore<ChangesetId, VertexWrapper> for CacheRequest<'_> {
    fn get_cache_key(&self, cs_id: &ChangesetId) -> String {
        let (_, bag) = self;
        format!("{}.cs.{}", bag.repo_id, cs_id)
    }

    async fn get_from_db(
        &self,
        keys: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, VertexWrapper>> {
        let (ctx, bag) = self;

        let futures = keys.into_iter().map(|cs_id| {
            bag.idmap
                .find_vertex(ctx, cs_id)
                .map_ok(move |v| (cs_id, v))
        });

        let res = future::try_join_all(futures)
            .await?
            .into_iter()
            .filter_map(|(cs_id, opt)| opt.map(move |v| (cs_id, VertexWrapper(v))))
            .collect();

        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fbinit::FacebookInit;

    use mononoke_types_mocks::changesetid::{ONES_CSID, TWOS_CSID};
    use sql_construct::SqlConstruct;

    use crate::builder::{SegmentedChangelogBuilder, SegmentedChangelogSqlConnections};

    #[fbinit::test]
    async fn test_no_key_colisions(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        let cache_handlers = CacheHandlers::new(
            CachelibHandler::create_mock(),
            CachelibHandler::create_mock(),
            MemcacheHandler::create_mock(),
        );
        let mut builder = SegmentedChangelogBuilder::new()
            .with_sql_connections(SegmentedChangelogSqlConnections::with_sqlite_in_memory()?)
            .with_repo_id(RepositoryId::new(1))
            .with_cache_handlers(cache_handlers.clone());
        let idmap1 = builder.clone().build_idmap()?;
        builder = builder.with_repo_id(RepositoryId::new(2));
        let idmap2 = builder.build_idmap()?;

        idmap1.insert(&ctx, Vertex(0), ONES_CSID).await?;
        idmap1.insert(&ctx, Vertex(1), TWOS_CSID).await?;
        idmap2.insert(&ctx, Vertex(0), TWOS_CSID).await?;

        // prime the caches
        assert_eq!(idmap1.get_changeset_id(&ctx, Vertex(0)).await?, ONES_CSID);
        assert_eq!(idmap1.get_changeset_id(&ctx, Vertex(1)).await?, TWOS_CSID);
        assert_eq!(idmap2.get_changeset_id(&ctx, Vertex(0)).await?, TWOS_CSID);

        assert_eq!(idmap1.get_vertex(&ctx, TWOS_CSID).await?, Vertex(1));
        assert_eq!(idmap2.get_vertex(&ctx, TWOS_CSID).await?, Vertex(0));

        // check caches are set
        assert_eq!(
            cache_handlers
                .dag_to_cs
                .mock_store()
                .expect("mock handler has mock store")
                .stats()
                .gets,
            3
        );
        assert_eq!(
            cache_handlers
                .cs_to_dag
                .mock_store()
                .expect("mock handler has mock store")
                .stats()
                .gets,
            2
        );

        // fetch from caches
        assert_eq!(idmap1.get_changeset_id(&ctx, Vertex(0)).await?, ONES_CSID);
        assert_eq!(idmap1.get_changeset_id(&ctx, Vertex(1)).await?, TWOS_CSID);
        assert_eq!(idmap2.get_changeset_id(&ctx, Vertex(0)).await?, TWOS_CSID);

        assert_eq!(idmap1.get_vertex(&ctx, TWOS_CSID).await?, Vertex(1));
        assert_eq!(idmap2.get_vertex(&ctx, TWOS_CSID).await?, Vertex(0));

        Ok(())
    }
}
