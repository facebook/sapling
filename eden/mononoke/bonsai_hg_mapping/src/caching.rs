/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::BonsaiHgMapping;
use super::BonsaiHgMappingEntry;
use super::BonsaiOrHgChangesetIds;
use abomonation_derive::Abomonation;
use anyhow::anyhow;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bonsai_hg_mapping_entry_thrift as thrift;
use bytes::Bytes;
use cachelib::VolatileLruCachePool;
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
use fbthrift::compact_protocol;
use memcache::KeyGen;
use memcache::MemcacheClient;
use mercurial_types::HgChangesetId;
use mercurial_types::HgNodeHash;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use stats::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use tunables::tunables;

define_stats! {
    prefix = "mononoke.bonsai_hg_mapping";
    memcache_hit: timeseries("memcache.hit"; Rate, Sum),
    memcache_miss: timeseries("memcache.miss"; Rate, Sum),
    memcache_internal_err: timeseries("memcache.internal_err"; Rate, Sum),
    memcache_deserialize_err: timeseries("memcache.deserialize_err"; Rate, Sum),
}

#[derive(Abomonation, Clone, Debug, Eq, Hash, PartialEq)]
pub struct BonsaiHgMappingCacheEntry {
    pub repo_id: RepositoryId,
    pub hg_cs_id: HgChangesetId,
    pub bcs_id: ChangesetId,
}

impl BonsaiHgMappingCacheEntry {
    fn from_thrift(
        entry: bonsai_hg_mapping_entry_thrift::BonsaiHgMappingCacheEntry,
    ) -> Result<Self> {
        Ok(Self {
            repo_id: RepositoryId::new(entry.repo_id.0),
            hg_cs_id: HgChangesetId::new(HgNodeHash::from_thrift(entry.hg_cs_id)?),
            bcs_id: ChangesetId::from_thrift(entry.bcs_id)?,
        })
    }

    fn into_thrift(self) -> bonsai_hg_mapping_entry_thrift::BonsaiHgMappingCacheEntry {
        bonsai_hg_mapping_entry_thrift::BonsaiHgMappingCacheEntry {
            repo_id: bonsai_hg_mapping_entry_thrift::RepoId(self.repo_id.id()),
            hg_cs_id: self.hg_cs_id.into_nodehash().into_thrift(),
            bcs_id: self.bcs_id.into_thrift(),
        }
    }

    pub fn new(repo_id: RepositoryId, hg_cs_id: HgChangesetId, bcs_id: ChangesetId) -> Self {
        BonsaiHgMappingCacheEntry {
            repo_id,
            hg_cs_id,
            bcs_id,
        }
    }

    pub fn into_entry(self, repo_id: RepositoryId) -> Result<BonsaiHgMappingEntry> {
        if self.repo_id == repo_id {
            Ok(BonsaiHgMappingEntry {
                hg_cs_id: self.hg_cs_id,
                bcs_id: self.bcs_id,
            })
        } else {
            Err(anyhow!(
                "Cache returned invalid entry: repo {} returned for query to repo {}",
                self.repo_id,
                repo_id
            ))
        }
    }

    pub fn from_entry(
        entry: BonsaiHgMappingEntry,
        repo_id: RepositoryId,
    ) -> BonsaiHgMappingCacheEntry {
        BonsaiHgMappingCacheEntry {
            repo_id,
            hg_cs_id: entry.hg_cs_id,
            bcs_id: entry.bcs_id,
        }
    }
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
    cache_pool: CachelibHandler<BonsaiHgMappingCacheEntry>,
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

fn memcache_deserialize(bytes: Bytes) -> Result<BonsaiHgMappingCacheEntry, ()> {
    let thrift_entry = compact_protocol::deserialize(bytes).map_err(|_| ());
    thrift_entry.and_then(|entry| BonsaiHgMappingCacheEntry::from_thrift(entry).map_err(|_| ()))
}

fn memcache_serialize(entry: &BonsaiHgMappingCacheEntry) -> Bytes {
    compact_protocol::serialize(&entry.clone().into_thrift())
}

const CHUNK_SIZE: usize = 1000;
const PARALLEL_CHUNKS: usize = 1;

#[async_trait]
impl BonsaiHgMapping for CachingBonsaiHgMapping {
    fn repo_id(&self) -> RepositoryId {
        self.mapping.repo_id()
    }

    async fn add(&self, ctx: &CoreContext, entry: BonsaiHgMappingEntry) -> Result<bool, Error> {
        self.mapping.add(ctx, entry).await
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        cs: BonsaiOrHgChangesetIds,
    ) -> Result<Vec<BonsaiHgMappingEntry>, Error> {
        let cache_request = (ctx, self);
        let repo_id = self.repo_id();

        let cache_entry = match cs {
            BonsaiOrHgChangesetIds::Bonsai(cs_ids) => get_or_fill_chunked(
                cache_request,
                cs_ids.into_iter().collect(),
                CHUNK_SIZE,
                PARALLEL_CHUNKS,
            )
            .await?
            .into_iter()
            .map(|(_, val)| val.into_entry(repo_id))
            .collect::<Result<_>>()?,
            BonsaiOrHgChangesetIds::Hg(hg_ids) => get_or_fill_chunked(
                cache_request,
                hg_ids.into_iter().collect(),
                CHUNK_SIZE,
                PARALLEL_CHUNKS,
            )
            .await?
            .into_iter()
            .map(|(_, val)| val.into_entry(repo_id))
            .collect::<Result<_>>()?,
        };

        Ok(cache_entry)
    }

    /// Use caching for the ranges of one element, use slower path otherwise.
    async fn get_hg_in_range(
        &self,
        ctx: &CoreContext,
        low: HgChangesetId,
        high: HgChangesetId,
        limit: usize,
    ) -> Result<Vec<HgChangesetId>, Error> {
        if low == high {
            let res = self.get(ctx, low.into()).await?;
            if res.is_empty() {
                return Ok(vec![]);
            } else {
                return Ok(vec![low]);
            }
        }

        self.mapping.get_hg_in_range(ctx, low, high, limit).await
    }
}

fn get_cache_key(repo_id: RepositoryId, cs: &BonsaiOrHgChangesetId) -> String {
    format!("{}.{:?}", repo_id.prefix(), cs)
}

impl MemcacheEntity for BonsaiHgMappingCacheEntry {
    fn serialize(&self) -> Bytes {
        memcache_serialize(self)
    }

    fn deserialize(bytes: Bytes) -> Result<Self, ()> {
        memcache_deserialize(bytes)
    }
}

type CacheRequest<'a> = (&'a CoreContext, &'a CachingBonsaiHgMapping);

impl EntityStore<BonsaiHgMappingCacheEntry> for CacheRequest<'_> {
    fn cachelib(&self) -> &CachelibHandler<BonsaiHgMappingCacheEntry> {
        let (_, mapping) = self;
        &mapping.cache_pool
    }

    fn keygen(&self) -> &KeyGen {
        let (_, mapping) = self;
        &mapping.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        let (_, mapping) = self;
        &mapping.memcache
    }

    fn cache_determinator(&self, _: &BonsaiHgMappingCacheEntry) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::NoTtl)
    }

    caching_ext::impl_singleton_stats!("bonsai_hg_mapping");
}

#[async_trait]
impl KeyedEntityStore<ChangesetId, BonsaiHgMappingCacheEntry> for CacheRequest<'_> {
    fn get_cache_key(&self, key: &ChangesetId) -> String {
        let (_, mapping) = self;
        get_cache_key(mapping.repo_id(), &BonsaiOrHgChangesetId::Bonsai(*key))
    }

    async fn get_from_db(
        &self,
        keys: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, BonsaiHgMappingCacheEntry>, Error> {
        let (ctx, mapping) = self;
        let repo_id = mapping.repo_id();

        let res = mapping
            .mapping
            .get(
                ctx,
                BonsaiOrHgChangesetIds::Bonsai(keys.into_iter().collect()),
            )
            .await?;

        Result::<_, Error>::Ok(
            res.into_iter()
                .map(|e| (e.bcs_id, BonsaiHgMappingCacheEntry::from_entry(e, repo_id)))
                .collect(),
        )
    }
}

#[async_trait]
impl KeyedEntityStore<HgChangesetId, BonsaiHgMappingCacheEntry> for CacheRequest<'_> {
    fn get_cache_key(&self, key: &HgChangesetId) -> String {
        let (_, mapping) = self;
        get_cache_key(mapping.repo_id(), &BonsaiOrHgChangesetId::Hg(*key))
    }

    async fn get_from_db(
        &self,
        keys: HashSet<HgChangesetId>,
    ) -> Result<HashMap<HgChangesetId, BonsaiHgMappingCacheEntry>, Error> {
        let (ctx, mapping) = self;
        let repo_id = mapping.repo_id();

        let res = mapping
            .mapping
            .get(ctx, BonsaiOrHgChangesetIds::Hg(keys.into_iter().collect()))
            .await?;

        Result::<_, Error>::Ok(
            res.into_iter()
                .map(|e| {
                    (
                        e.hg_cs_id,
                        BonsaiHgMappingCacheEntry::from_entry(e, repo_id),
                    )
                })
                .collect(),
        )
    }
}
