/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use abomonation_derive::Abomonation;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;

use caching_ext::get_or_fill_chunked;
use caching_ext::CacheDisposition;
use caching_ext::CacheTtl;
use caching_ext::CachelibHandler;
use caching_ext::EntityStore;
use caching_ext::KeyedEntityStore;
use caching_ext::MemcacheEntity;
use caching_ext::MemcacheHandler;
use context::CoreContext;
use fbinit::FacebookInit;
use futures::future;
use futures::future::FutureExt;
use futures::future::TryFutureExt;
use memcache::KeyGen;
use memcache::MemcacheClient;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;

use crate::idmap::IdMap;
use crate::types::IdMapVersion;
use crate::DagId;

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
    pub cs_to_dag: CachelibHandler<DagIdWrapper>,
    pub memcache: MemcacheHandler,
}

impl CacheHandlers {
    pub fn new(
        dag_to_cs: CachelibHandler<ChangesetIdWrapper>,
        cs_to_dag: CachelibHandler<DagIdWrapper>,
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

    pub fn mock() -> Self {
        Self {
            dag_to_cs: CachelibHandler::create_mock(),
            cs_to_dag: CachelibHandler::create_mock(),
            memcache: MemcacheHandler::create_mock(),
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

// Number of entries to fetch from DB into cache at a time
const CHUNK_SIZE: usize = 1000;
const PARALLEL_CHUNKS: usize = 10;

#[async_trait]
impl IdMap for CachedIdMap {
    async fn insert_many(
        &self,
        ctx: &CoreContext,
        mappings: Vec<(DagId, ChangesetId)>,
    ) -> Result<()> {
        self.idmap.insert_many(ctx, mappings).await?;
        Ok(())
    }

    async fn find_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        dag_ids: Vec<DagId>,
    ) -> Result<HashMap<DagId, ChangesetId>> {
        let ctx = (ctx, self);
        let res = get_or_fill_chunked(
            ctx,
            dag_ids.into_iter().collect(),
            CHUNK_SIZE,
            PARALLEL_CHUNKS,
        )
        .await
        .with_context(|| "Error fetching many changeset ids via cache")?
        .into_iter()
        .map(|(k, v)| (k, v.0))
        .collect();
        Ok(res)
    }

    async fn find_many_dag_ids(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, DagId>> {
        let ctx = (ctx, self, DagIdStaleness::Fresh);
        let res = get_or_fill_chunked(
            ctx,
            cs_ids.into_iter().collect(),
            CHUNK_SIZE,
            PARALLEL_CHUNKS,
        )
        .await
        .with_context(|| "Error fetching many changeset ids via cache")?
        .into_iter()
        .map(|(k, v)| (k, v.0))
        .collect();
        Ok(res)
    }

    async fn find_many_dag_ids_maybe_stale(
        &self,
        ctx: &CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, DagId>> {
        let ctx = (ctx, self, DagIdStaleness::MaybeStale);
        let res = get_or_fill_chunked(
            ctx,
            cs_ids.into_iter().collect(),
            CHUNK_SIZE,
            PARALLEL_CHUNKS,
        )
        .await
        .with_context(|| "Error fetching many changeset ids via cache")?
        .into_iter()
        .map(|(k, v)| (k, v.0))
        .collect();
        Ok(res)
    }

    async fn get_last_entry(&self, ctx: &CoreContext) -> Result<Option<(DagId, ChangesetId)>> {
        self.idmap.get_last_entry(ctx).await
    }

    fn idmap_version(&self) -> Option<IdMapVersion> {
        self.idmap.idmap_version()
    }
}

type ChangesetIdCacheRequest<'a> = (&'a CoreContext, &'a CachedIdMap);

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

impl EntityStore<ChangesetIdWrapper> for ChangesetIdCacheRequest<'_> {
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
impl KeyedEntityStore<DagId, ChangesetIdWrapper> for ChangesetIdCacheRequest<'_> {
    fn get_cache_key(&self, dag_id: &DagId) -> String {
        let (_, bag) = self;
        format!("{}.dag_id.{}", bag.repo_id, dag_id)
    }

    async fn get_from_db(
        &self,
        keys: HashSet<DagId>,
    ) -> Result<HashMap<DagId, ChangesetIdWrapper>> {
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

enum DagIdStaleness {
    MaybeStale,
    Fresh,
}

type DagIdCacheRequest<'a> = (&'a CoreContext, &'a CachedIdMap, DagIdStaleness);

#[derive(Clone, Copy, Debug, Abomonation)]
pub struct DagIdWrapper(DagId);

impl From<DagId> for DagIdWrapper {
    fn from(dag_id: DagId) -> Self {
        DagIdWrapper(dag_id)
    }
}

impl MemcacheEntity for DagIdWrapper {
    fn serialize(&self) -> Bytes {
        Bytes::copy_from_slice(&self.0.0.to_be_bytes())
    }

    fn deserialize(bytes: Bytes) -> Result<Self, ()> {
        let arr = bytes.as_ref().try_into().map_err(|_| ())?;
        Ok(DagIdWrapper(DagId(u64::from_be_bytes(arr))))
    }
}

impl EntityStore<DagIdWrapper> for DagIdCacheRequest<'_> {
    fn cachelib(&self) -> &CachelibHandler<DagIdWrapper> {
        let (_, bag, _) = self;
        &bag.cache_handlers.cs_to_dag
    }

    fn keygen(&self) -> &KeyGen {
        let (_, bag, _) = self;
        &bag.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        let (_, bag, _) = self;
        &bag.cache_handlers.memcache
    }

    fn cache_determinator(&self, _: &DagIdWrapper) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::NoTtl)
    }

    caching_ext::impl_singleton_stats!("segmented_changelog.idmap.cs2dag");
}

#[async_trait]
impl KeyedEntityStore<ChangesetId, DagIdWrapper> for DagIdCacheRequest<'_> {
    fn get_cache_key(&self, cs_id: &ChangesetId) -> String {
        let (_, bag, _) = self;
        format!("{}.cs.{}", bag.repo_id, cs_id)
    }

    async fn get_from_db(
        &self,
        keys: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, DagIdWrapper>> {
        let (ctx, bag, staleness) = self;

        let futures = keys.into_iter().map(|cs_id| match staleness {
            DagIdStaleness::Fresh => bag
                .idmap
                .find_dag_id(ctx, cs_id)
                .map_ok(move |v| (cs_id, v))
                .left_future(),
            DagIdStaleness::MaybeStale => bag
                .idmap
                .find_dag_id_maybe_stale(ctx, cs_id)
                .map_ok(move |v| (cs_id, v))
                .right_future(),
        });

        let res = future::try_join_all(futures)
            .await?
            .into_iter()
            .filter_map(|(cs_id, opt)| opt.map(move |v| (cs_id, DagIdWrapper(v))))
            .collect();

        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fbinit::FacebookInit;

    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::TWOS_CSID;
    use sql_construct::SqlConstruct;
    use sql_ext::replication::NoReplicaLagMonitor;

    use crate::builder::SegmentedChangelogSqlConnections;
    use crate::idmap::SqlIdMap;

    #[fbinit::test]
    async fn test_no_key_colisions(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);

        let cache_handlers = CacheHandlers::new(
            CachelibHandler::create_mock(),
            CachelibHandler::create_mock(),
            MemcacheHandler::create_mock(),
        );
        let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;
        let new_cached_idmap = |repo_id| {
            let idmap_version = IdMapVersion(0);
            let sql_idmap = SqlIdMap::new(
                conns.0.clone(),
                Arc::new(NoReplicaLagMonitor()),
                repo_id,
                idmap_version,
            );
            CachedIdMap::new(
                Arc::new(sql_idmap),
                cache_handlers.clone(),
                repo_id,
                idmap_version,
            )
        };
        let idmap1 = new_cached_idmap(RepositoryId::new(1));
        let idmap2 = new_cached_idmap(RepositoryId::new(2));

        idmap1.insert(&ctx, DagId(0), ONES_CSID).await?;
        idmap1.insert(&ctx, DagId(1), TWOS_CSID).await?;
        idmap2.insert(&ctx, DagId(0), TWOS_CSID).await?;

        // prime the caches
        assert_eq!(idmap1.get_changeset_id(&ctx, DagId(0)).await?, ONES_CSID);
        assert_eq!(idmap1.get_changeset_id(&ctx, DagId(1)).await?, TWOS_CSID);
        assert_eq!(idmap2.get_changeset_id(&ctx, DagId(0)).await?, TWOS_CSID);

        assert_eq!(idmap1.get_dag_id(&ctx, TWOS_CSID).await?, DagId(1));
        assert_eq!(idmap2.get_dag_id(&ctx, TWOS_CSID).await?, DagId(0));

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
        assert_eq!(idmap1.get_changeset_id(&ctx, DagId(0)).await?, ONES_CSID);
        assert_eq!(idmap1.get_changeset_id(&ctx, DagId(1)).await?, TWOS_CSID);
        assert_eq!(idmap2.get_changeset_id(&ctx, DagId(0)).await?, TWOS_CSID);

        assert_eq!(idmap1.get_dag_id(&ctx, TWOS_CSID).await?, DagId(1));
        assert_eq!(idmap2.get_dag_id(&ctx, TWOS_CSID).await?, DagId(0));

        Ok(())
    }
}
