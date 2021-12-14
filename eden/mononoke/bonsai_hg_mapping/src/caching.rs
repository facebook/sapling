/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::{BonsaiHgMapping, BonsaiHgMappingEntry, BonsaiOrHgChangesetIds};
use anyhow::Error;
use async_trait::async_trait;
use bonsai_hg_mapping_entry_thrift as thrift;
use bytes::Bytes;
use cachelib::VolatileLruCachePool;
use caching_ext::{
    get_or_fill, CacheDisposition, CacheTtl, CachelibHandler, EntityStore, KeyedEntityStore,
    MemcacheEntity, MemcacheHandler,
};
use context::CoreContext;
use fbinit::FacebookInit;
use fbthrift::compact_protocol;
use memcache::{KeyGen, MemcacheClient};
use mercurial_types::HgChangesetId;
use mononoke_types::{ChangesetId, RepositoryId};
use stats::prelude::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tunables::tunables;

define_stats! {
    prefix = "mononoke.bonsai_hg_mapping";
    memcache_hit: timeseries("memcache.hit"; Rate, Sum),
    memcache_miss: timeseries("memcache.miss"; Rate, Sum),
    memcache_internal_err: timeseries("memcache.internal_err"; Rate, Sum),
    memcache_deserialize_err: timeseries("memcache.deserialize_err"; Rate, Sum),
}

/// Used for cache key generation
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
enum BonsaiOrHgChangesetId {
    Bonsai(ChangesetId),
    Hg(HgChangesetId),
}

impl From<ChangesetId> for BonsaiOrHgChangesetId {
    fn from(cs_id: ChangesetId) -> Self {
        BonsaiOrHgChangesetId::Bonsai(cs_id)
    }
}

impl From<HgChangesetId> for BonsaiOrHgChangesetId {
    fn from(cs_id: HgChangesetId) -> Self {
        BonsaiOrHgChangesetId::Hg(cs_id)
    }
}

#[derive(Clone)]
pub struct CachingBonsaiHgMapping {
    mapping: Arc<dyn BonsaiHgMapping>,
    cache_pool: CachelibHandler<BonsaiHgMappingEntry>,
    memcache: MemcacheHandler,
    keygen: KeyGen,
}

impl CachingBonsaiHgMapping {
    pub fn new(
        fb: FacebookInit,
        mapping: Arc<dyn BonsaiHgMapping>,
        cache_pool: VolatileLruCachePool,
    ) -> Self {
        Self {
            mapping,
            cache_pool: cache_pool.into(),
            memcache: MemcacheClient::new(fb)
                .expect("Memcache initialization failed")
                .into(),
            keygen: CachingBonsaiHgMapping::create_key_gen(),
        }
    }

    pub fn new_test(mapping: Arc<dyn BonsaiHgMapping>) -> Self {
        Self {
            mapping,
            cache_pool: CachelibHandler::create_mock(),
            memcache: MemcacheHandler::create_mock(),
            keygen: CachingBonsaiHgMapping::create_key_gen(),
        }
    }

    fn create_key_gen() -> KeyGen {
        let key_prefix = "scm.mononoke.bonsai_hg_mapping";

        let sitever = if tunables().get_bonsai_hg_mapping_sitever() > 0 {
            tunables().get_bonsai_hg_mapping_sitever() as u32
        } else {
            thrift::MC_SITEVER as u32
        };

        KeyGen::new(key_prefix, thrift::MC_CODEVER as u32, sitever)
    }
}

fn memcache_deserialize(bytes: Bytes) -> Result<BonsaiHgMappingEntry, ()> {
    let thrift_entry = compact_protocol::deserialize(bytes).map_err(|_| ());
    thrift_entry.and_then(|entry| BonsaiHgMappingEntry::from_thrift(entry).map_err(|_| ()))
}

fn memcache_serialize(entry: &BonsaiHgMappingEntry) -> Bytes {
    compact_protocol::serialize(&entry.clone().into_thrift())
}

#[async_trait]
impl BonsaiHgMapping for CachingBonsaiHgMapping {
    async fn add(&self, ctx: &CoreContext, entry: BonsaiHgMappingEntry) -> Result<bool, Error> {
        self.mapping.add(ctx, entry).await
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        cs: BonsaiOrHgChangesetIds,
    ) -> Result<Vec<BonsaiHgMappingEntry>, Error> {
        let ctx = (ctx, repo_id, self);

        let res = match cs {
            BonsaiOrHgChangesetIds::Bonsai(cs_ids) => {
                get_or_fill(ctx, cs_ids.into_iter().collect())
                    .await?
                    .into_iter()
                    .map(|(_, val)| val)
                    .collect()
            }
            BonsaiOrHgChangesetIds::Hg(hg_ids) => get_or_fill(ctx, hg_ids.into_iter().collect())
                .await?
                .into_iter()
                .map(|(_, val)| val)
                .collect(),
        };

        Ok(res)
    }

    /// Use caching for the ranges of one element, use slower path otherwise.
    async fn get_hg_in_range(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        low: HgChangesetId,
        high: HgChangesetId,
        limit: usize,
    ) -> Result<Vec<HgChangesetId>, Error> {
        if low == high {
            let res = self.get(ctx, repo_id, low.into()).await?;
            if res.is_empty() {
                return Ok(vec![]);
            } else {
                return Ok(vec![low]);
            }
        }

        self.mapping
            .get_hg_in_range(ctx, repo_id, low, high, limit)
            .await
    }
}

fn get_cache_key(repo_id: RepositoryId, cs: &BonsaiOrHgChangesetId) -> String {
    format!("{}.{:?}", repo_id.prefix(), cs).to_string()
}

impl MemcacheEntity for BonsaiHgMappingEntry {
    fn serialize(&self) -> Bytes {
        memcache_serialize(self)
    }

    fn deserialize(bytes: Bytes) -> Result<Self, ()> {
        memcache_deserialize(bytes)
    }
}

type CacheRequest<'a> = (&'a CoreContext, RepositoryId, &'a CachingBonsaiHgMapping);

impl EntityStore<BonsaiHgMappingEntry> for CacheRequest<'_> {
    fn cachelib(&self) -> &CachelibHandler<BonsaiHgMappingEntry> {
        let (_, _, mapping) = self;
        &mapping.cache_pool
    }

    fn keygen(&self) -> &KeyGen {
        let (_, _, mapping) = self;
        &mapping.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        let (_, _, mapping) = self;
        &mapping.memcache
    }

    fn cache_determinator(&self, _: &BonsaiHgMappingEntry) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::NoTtl)
    }

    caching_ext::impl_singleton_stats!("bonsai_hg_mapping");
}

#[async_trait]
impl KeyedEntityStore<ChangesetId, BonsaiHgMappingEntry> for CacheRequest<'_> {
    fn get_cache_key(&self, key: &ChangesetId) -> String {
        let (_, repo_id, _) = self;
        get_cache_key(*repo_id, &BonsaiOrHgChangesetId::Bonsai(*key))
    }

    async fn get_from_db(
        &self,
        keys: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, BonsaiHgMappingEntry>, Error> {
        let (ctx, repo_id, mapping) = self;

        let res = mapping
            .mapping
            .get(
                ctx,
                *repo_id,
                BonsaiOrHgChangesetIds::Bonsai(keys.into_iter().collect()),
            )
            .await?;

        Result::<_, Error>::Ok(res.into_iter().map(|e| (e.bcs_id, e)).collect())
    }
}

#[async_trait]
impl KeyedEntityStore<HgChangesetId, BonsaiHgMappingEntry> for CacheRequest<'_> {
    fn get_cache_key(&self, key: &HgChangesetId) -> String {
        let (_, repo_id, _) = self;
        get_cache_key(*repo_id, &BonsaiOrHgChangesetId::Hg(*key))
    }

    async fn get_from_db(
        &self,
        keys: HashSet<HgChangesetId>,
    ) -> Result<HashMap<HgChangesetId, BonsaiHgMappingEntry>, Error> {
        let (ctx, repo_id, mapping) = self;

        let res = mapping
            .mapping
            .get(
                ctx,
                *repo_id,
                BonsaiOrHgChangesetIds::Hg(keys.into_iter().collect()),
            )
            .await?;

        Result::<_, Error>::Ok(res.into_iter().map(|e| (e.hg_cs_id, e)).collect())
    }
}
