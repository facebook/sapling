/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation_derive::Abomonation;
use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use caching_ext::get_or_fill;
use caching_ext::get_or_fill_chunked;
use caching_ext::CacheDisposition;
use caching_ext::CacheTtl;
use caching_ext::CachelibHandler;
use caching_ext::EntityStore;
use caching_ext::KeyedEntityStore;
use caching_ext::MemcacheEntity;
use caching_ext::MemcacheHandler;
use changeset_entry_thrift as thrift;
use changesets::ChangesetEntry;
use changesets::ChangesetInsert;
use changesets::Changesets;
use changesets::SortOrder;
use context::CoreContext;
use fbinit::FacebookInit;
use fbthrift::compact_protocol;
use futures::stream::BoxStream;
use maplit::hashset;
use memcache::KeyGen;
use memcache::MemcacheClient;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::RepositoryId;
use ref_cast::RefCast;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

#[cfg(test)]
use caching_ext::MockStoreStats;

pub fn get_cache_key(repo_id: RepositoryId, cs_id: &ChangesetId) -> String {
    format!("{}.{}", repo_id.prefix(), cs_id)
}

#[derive(Clone, Debug, Abomonation, RefCast)]
#[repr(transparent)]
pub struct ChangesetEntryWrapper(ChangesetEntry);

#[derive(Clone)]
pub struct CachingChangesets {
    changesets: Arc<dyn Changesets>,
    cachelib: CachelibHandler<ChangesetEntryWrapper>,
    memcache: MemcacheHandler,
    keygen: KeyGen,
    repo_id: RepositoryId,
}

fn get_keygen() -> KeyGen {
    let key_prefix = "scm.mononoke.changesets";

    KeyGen::new(
        key_prefix,
        thrift::MC_CODEVER as u32,
        thrift::MC_SITEVER as u32,
    )
}

impl CachingChangesets {
    pub fn new(
        fb: FacebookInit,
        changesets: Arc<dyn Changesets>,
        cache_pool: cachelib::VolatileLruCachePool,
    ) -> Self {
        Self {
            repo_id: changesets.repo_id(),
            changesets,
            cachelib: cache_pool.into(),
            memcache: MemcacheClient::new(fb)
                .expect("Memcache initialization failed")
                .into(),
            keygen: get_keygen(),
        }
    }

    #[cfg(test)]
    pub fn mocked(changesets: Arc<dyn Changesets>) -> Self {
        let cachelib = CachelibHandler::create_mock();
        let memcache = MemcacheHandler::create_mock();

        Self {
            repo_id: changesets.repo_id(),
            changesets,
            cachelib,
            memcache,
            keygen: get_keygen(),
        }
    }

    #[cfg(test)]
    pub fn fork_cachelib(&self) -> Self {
        Self {
            repo_id: self.repo_id,
            changesets: self.changesets.clone(),
            cachelib: CachelibHandler::create_mock(),
            memcache: self.memcache.clone(),
            keygen: self.keygen.clone(),
        }
    }

    #[cfg(test)]
    pub fn cachelib_stats(&self) -> MockStoreStats {
        match self.cachelib {
            CachelibHandler::Real(_) => unimplemented!(),
            CachelibHandler::Mock(ref mock) => mock.stats(),
        }
    }

    #[cfg(test)]
    pub fn memcache_stats(&self) -> MockStoreStats {
        match self.memcache {
            MemcacheHandler::Real(_) => unimplemented!(),
            MemcacheHandler::Mock(ref mock) => mock.stats(),
        }
    }
}

#[async_trait]
impl Changesets for CachingChangesets {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add(&self, ctx: CoreContext, cs: ChangesetInsert) -> Result<bool, Error> {
        self.changesets.add(ctx, cs).await
    }

    async fn get(
        &self,
        ctx: CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEntry>, Error> {
        let ctx = (&ctx, self);
        let mut map = get_or_fill(ctx, hashset![cs_id]).await?;
        Ok(map.remove(&cs_id).map(|entry| entry.0))
    }

    async fn get_many(
        &self,
        ctx: CoreContext,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<ChangesetEntry>, Error> {
        let ctx = (&ctx, self);
        let res = get_or_fill_chunked(ctx, cs_ids.into_iter().collect(), 1000, 2)
            .await?
            .into_iter()
            .map(|(_, val)| val.0)
            .collect();
        Ok(res)
    }

    /// Use caching for the full changeset ids and slower path otherwise.
    async fn get_many_by_prefix(
        &self,
        ctx: CoreContext,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix, Error> {
        if let Some(id) = cs_prefix.into_changeset_id() {
            let res = self.get(ctx, id).await?;
            return match res {
                Some(_) if limit > 0 => Ok(ChangesetIdsResolvedFromPrefix::Single(id)),
                _ => Ok(ChangesetIdsResolvedFromPrefix::NoMatch),
            };
        }
        self.changesets
            .get_many_by_prefix(ctx, cs_prefix, limit)
            .await
    }

    fn prime_cache(&self, _ctx: &CoreContext, changesets: &[ChangesetEntry]) {
        for cs in changesets {
            assert_eq!(cs.repo_id, self.repo_id);
            let key = get_cache_key(self.repo_id, &cs.cs_id);
            let _ = self
                .cachelib
                .set_cached(&key, ChangesetEntryWrapper::ref_cast(cs), None);
        }
    }

    async fn enumeration_bounds(
        &self,
        ctx: &CoreContext,
        read_from_master: bool,
        known_heads: Vec<ChangesetId>,
    ) -> Result<Option<(u64, u64)>, Error> {
        self.changesets
            .enumeration_bounds(ctx, read_from_master, known_heads)
            .await
    }

    fn list_enumeration_range(
        &self,
        ctx: &CoreContext,
        min_id: u64,
        max_id: u64,
        sort_and_limit: Option<(SortOrder, u64)>,
        read_from_master: bool,
    ) -> BoxStream<'_, Result<(ChangesetId, u64), Error>> {
        self.changesets.list_enumeration_range(
            ctx,
            min_id,
            max_id,
            sort_and_limit,
            read_from_master,
        )
    }
}

impl MemcacheEntity for ChangesetEntryWrapper {
    fn serialize(&self) -> Bytes {
        compact_protocol::serialize(&self.0.clone().into_thrift())
    }

    fn deserialize(bytes: Bytes) -> Result<Self, ()> {
        compact_protocol::deserialize(bytes)
            .and_then(ChangesetEntry::from_thrift)
            .map(ChangesetEntryWrapper)
            .map_err(|_| ())
    }
}

type CacheRequest<'a> = (&'a CoreContext, &'a CachingChangesets);

impl EntityStore<ChangesetEntryWrapper> for CacheRequest<'_> {
    fn cachelib(&self) -> &CachelibHandler<ChangesetEntryWrapper> {
        let (_, mapping) = self;
        &mapping.cachelib
    }

    fn keygen(&self) -> &KeyGen {
        let (_, mapping) = self;
        &mapping.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        let (_, mapping) = self;
        &mapping.memcache
    }

    fn cache_determinator(&self, _: &ChangesetEntryWrapper) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::NoTtl)
    }

    caching_ext::impl_singleton_stats!("changesets");

    #[cfg(test)]
    fn spawn_memcache_writes(&self) -> bool {
        let (_, mapping) = self;

        match mapping.memcache {
            MemcacheHandler::Real(_) => true,
            MemcacheHandler::Mock(..) => false,
        }
    }
}

#[async_trait]
impl KeyedEntityStore<ChangesetId, ChangesetEntryWrapper> for CacheRequest<'_> {
    fn get_cache_key(&self, cs_id: &ChangesetId) -> String {
        let (_, mapping) = self;
        get_cache_key(mapping.repo_id, cs_id)
    }

    async fn get_from_db(
        &self,
        keys: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, ChangesetEntryWrapper>, Error> {
        let (ctx, mapping) = self;

        let res = mapping
            .changesets
            .get_many((*ctx).clone(), keys.into_iter().collect())
            .await?;

        Result::<_, Error>::Ok(
            res.into_iter()
                .map(|e| (e.cs_id, ChangesetEntryWrapper(e)))
                .collect(),
        )
    }
}
