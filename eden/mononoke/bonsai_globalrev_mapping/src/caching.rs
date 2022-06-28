/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use abomonation_derive::Abomonation;
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use cachelib::VolatileLruCachePool;
use caching_ext::get_or_fill;
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
use mononoke_types::ChangesetId;
use mononoke_types::Globalrev;
use mononoke_types::RepositoryId;
use std::collections::HashMap;
use std::collections::HashSet;

use bonsai_globalrev_mapping_thrift as thrift;

use super::BonsaiGlobalrevMapping;
use super::BonsaiGlobalrevMappingEntry;
use super::BonsaisOrGlobalrevs;

#[derive(Abomonation, Clone, Debug, Eq, Hash, PartialEq)]
pub struct BonsaiGlobalrevMappingCacheEntry {
    pub repo_id: RepositoryId,
    pub bcs_id: ChangesetId,
    pub globalrev: Globalrev,
}

impl BonsaiGlobalrevMappingCacheEntry {
    fn into_entry(self, repo_id: RepositoryId) -> Result<BonsaiGlobalrevMappingEntry> {
        if self.repo_id == repo_id {
            Ok(BonsaiGlobalrevMappingEntry {
                bcs_id: self.bcs_id,
                globalrev: self.globalrev,
            })
        } else {
            Err(anyhow!(
                "Cache returned invalid entry: repo {} returned for query to repo {}",
                self.repo_id,
                repo_id
            ))
        }
    }

    fn from_entry(
        entry: BonsaiGlobalrevMappingEntry,
        repo_id: RepositoryId,
    ) -> BonsaiGlobalrevMappingCacheEntry {
        BonsaiGlobalrevMappingCacheEntry {
            repo_id,
            bcs_id: entry.bcs_id,
            globalrev: entry.globalrev,
        }
    }
}

#[derive(Clone)]
pub struct CachingBonsaiGlobalrevMapping<T> {
    cachelib: CachelibHandler<BonsaiGlobalrevMappingCacheEntry>,
    memcache: MemcacheHandler,
    keygen: KeyGen,
    inner: T,
}

impl<T> CachingBonsaiGlobalrevMapping<T> {
    pub fn new(fb: FacebookInit, inner: T, cachelib: VolatileLruCachePool) -> Self {
        Self {
            inner,
            cachelib: cachelib.into(),
            memcache: MemcacheClient::new(fb)
                .expect("Memcache initialization failed")
                .into(),
            keygen: Self::create_key_gen(),
        }
    }

    pub fn new_test(inner: T) -> Self {
        Self {
            inner,
            cachelib: CachelibHandler::create_mock(),
            memcache: MemcacheHandler::create_mock(),
            keygen: Self::create_key_gen(),
        }
    }

    fn create_key_gen() -> KeyGen {
        let key_prefix = "scm.mononoke.bonsai_globalrev_mapping";

        KeyGen::new(
            key_prefix,
            thrift::MC_CODEVER as u32,
            thrift::MC_SITEVER as u32,
        )
    }

    pub fn cachelib(&self) -> &CachelibHandler<BonsaiGlobalrevMappingCacheEntry> {
        &self.cachelib
    }
}

#[async_trait]
impl<T> BonsaiGlobalrevMapping for CachingBonsaiGlobalrevMapping<T>
where
    T: BonsaiGlobalrevMapping + Clone + Sync + Send + 'static,
{
    fn repo_id(&self) -> RepositoryId {
        self.inner.repo_id()
    }

    async fn bulk_import(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGlobalrevMappingEntry],
    ) -> Result<(), Error> {
        self.inner.bulk_import(ctx, entries).await
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        objects: BonsaisOrGlobalrevs,
    ) -> Result<Vec<BonsaiGlobalrevMappingEntry>, Error> {
        let cache_request = (ctx, self);
        let repo_id = self.repo_id();

        let res = match objects {
            BonsaisOrGlobalrevs::Bonsai(cs_ids) => {
                get_or_fill(cache_request, cs_ids.into_iter().collect())
                    .await
                    .with_context(|| "Error fetching globalrevs via cache")?
                    .into_iter()
                    .map(|(_, val)| val.into_entry(repo_id))
                    .collect::<Result<_>>()?
            }
            BonsaisOrGlobalrevs::Globalrev(globalrevs) => {
                get_or_fill(cache_request, globalrevs.into_iter().collect())
                    .await
                    .with_context(|| "Error fetching bonsais via cache")?
                    .into_iter()
                    .map(|(_, val)| val.into_entry(repo_id))
                    .collect::<Result<_>>()?
            }
        };

        Ok(res)
    }

    async fn get_closest_globalrev(
        &self,
        ctx: &CoreContext,
        globalrev: Globalrev,
    ) -> Result<Option<Globalrev>, Error> {
        self.inner.get_closest_globalrev(ctx, globalrev).await
    }

    async fn get_max(&self, ctx: &CoreContext) -> Result<Option<Globalrev>, Error> {
        self.inner.get_max(ctx).await
    }
}

impl MemcacheEntity for BonsaiGlobalrevMappingCacheEntry {
    fn serialize(&self) -> Bytes {
        let entry = thrift::BonsaiGlobalrevMappingEntry {
            repo_id: self.repo_id.id(),
            bcs_id: self.bcs_id.into_thrift(),
            globalrev: self
                .globalrev
                .id()
                .try_into()
                .expect("Globalrevs must fit within a i64"),
        };
        compact_protocol::serialize(&entry)
    }

    fn deserialize(bytes: Bytes) -> Result<Self, ()> {
        let thrift::BonsaiGlobalrevMappingEntry {
            repo_id,
            bcs_id,
            globalrev,
        } = compact_protocol::deserialize(bytes).map_err(|_| ())?;

        let repo_id = RepositoryId::new(repo_id);
        let bcs_id = ChangesetId::from_thrift(bcs_id).map_err(|_| ())?;
        let globalrev = Globalrev::new(globalrev.try_into().map_err(|_| ())?);

        Ok(BonsaiGlobalrevMappingCacheEntry {
            repo_id,
            bcs_id,
            globalrev,
        })
    }
}

type CacheRequest<'a, T> = (&'a CoreContext, &'a CachingBonsaiGlobalrevMapping<T>);

impl<T> EntityStore<BonsaiGlobalrevMappingCacheEntry> for CacheRequest<'_, T> {
    fn cachelib(&self) -> &CachelibHandler<BonsaiGlobalrevMappingCacheEntry> {
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

    fn cache_determinator(&self, _: &BonsaiGlobalrevMappingCacheEntry) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::NoTtl)
    }

    caching_ext::impl_singleton_stats!("bonsai_globalrev_mapping");
}

#[async_trait]
impl<T> KeyedEntityStore<ChangesetId, BonsaiGlobalrevMappingCacheEntry> for CacheRequest<'_, T>
where
    T: BonsaiGlobalrevMapping + Send + Sync + Clone + 'static,
{
    fn get_cache_key(&self, key: &ChangesetId) -> String {
        let (_, mapping) = self;
        format!("{}.bonsai.{}", mapping.repo_id(), key)
    }

    async fn get_from_db(
        &self,
        keys: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, BonsaiGlobalrevMappingCacheEntry>, Error> {
        let (ctx, mapping) = self;
        let repo_id = mapping.repo_id();

        let res = mapping
            .inner
            .get(ctx, BonsaisOrGlobalrevs::Bonsai(keys.into_iter().collect()))
            .await
            .with_context(|| "Error fetching globalrevs from bonsais from SQL")?;

        Result::<_, Error>::Ok(
            res.into_iter()
                .map(|e| {
                    (
                        e.bcs_id,
                        BonsaiGlobalrevMappingCacheEntry::from_entry(e, repo_id),
                    )
                })
                .collect(),
        )
    }
}

#[async_trait]
impl<T> KeyedEntityStore<Globalrev, BonsaiGlobalrevMappingCacheEntry> for CacheRequest<'_, T>
where
    T: BonsaiGlobalrevMapping + Send + Sync + Clone + 'static,
{
    fn get_cache_key(&self, key: &Globalrev) -> String {
        let (_, mapping) = self;
        format!("{}.globalrev.{}", mapping.repo_id(), key.id())
    }

    async fn get_from_db(
        &self,
        keys: HashSet<Globalrev>,
    ) -> Result<HashMap<Globalrev, BonsaiGlobalrevMappingCacheEntry>, Error> {
        let (ctx, mapping) = self;
        let repo_id = mapping.repo_id();

        let res = mapping
            .inner
            .get(
                ctx,
                BonsaisOrGlobalrevs::Globalrev(keys.into_iter().collect()),
            )
            .await
            .with_context(|| "Error fetching bonsais from globalrevs from SQL")?;

        Result::<_, Error>::Ok(
            res.into_iter()
                .map(|e| {
                    (
                        e.globalrev,
                        BonsaiGlobalrevMappingCacheEntry::from_entry(e, repo_id),
                    )
                })
                .collect(),
        )
    }
}
