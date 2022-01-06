/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context as _, Error};
use async_trait::async_trait;
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
use mononoke_types::{ChangesetId, Globalrev, RepositoryId};
use std::collections::{HashMap, HashSet};

use bonsai_globalrev_mapping_thrift as thrift;

use super::{BonsaiGlobalrevMapping, BonsaiGlobalrevMappingEntry, BonsaisOrGlobalrevs};

#[derive(Clone)]
pub struct CachingBonsaiGlobalrevMapping<T> {
    cachelib: CachelibHandler<BonsaiGlobalrevMappingEntry>,
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

    pub fn cachelib(&self) -> &CachelibHandler<BonsaiGlobalrevMappingEntry> {
        &self.cachelib
    }
}

#[async_trait]
impl<T> BonsaiGlobalrevMapping for CachingBonsaiGlobalrevMapping<T>
where
    T: BonsaiGlobalrevMapping + Clone + Sync + Send + 'static,
{
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
        repo_id: RepositoryId,
        objects: BonsaisOrGlobalrevs,
    ) -> Result<Vec<BonsaiGlobalrevMappingEntry>, Error> {
        let ctx = (ctx, repo_id, self);

        let res = match objects {
            BonsaisOrGlobalrevs::Bonsai(cs_ids) => get_or_fill(ctx, cs_ids.into_iter().collect())
                .await
                .with_context(|| "Error fetching globalrevs via cache")?
                .into_iter()
                .map(|(_, val)| val)
                .collect(),
            BonsaisOrGlobalrevs::Globalrev(globalrevs) => {
                get_or_fill(ctx, globalrevs.into_iter().collect())
                    .await
                    .with_context(|| "Error fetching bonsais via cache")?
                    .into_iter()
                    .map(|(_, val)| val)
                    .collect()
            }
        };


        Ok(res)
    }

    async fn get_closest_globalrev(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        globalrev: Globalrev,
    ) -> Result<Option<Globalrev>, Error> {
        self.inner
            .get_closest_globalrev(ctx, repo_id, globalrev)
            .await
    }

    async fn get_max(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<Option<Globalrev>, Error> {
        self.inner.get_max(ctx, repo_id).await
    }
}

impl MemcacheEntity for BonsaiGlobalrevMappingEntry {
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

        Ok(BonsaiGlobalrevMappingEntry {
            repo_id,
            bcs_id,
            globalrev,
        })
    }
}

type CacheRequest<'a, T> = (
    &'a CoreContext,
    RepositoryId,
    &'a CachingBonsaiGlobalrevMapping<T>,
);

impl<T> EntityStore<BonsaiGlobalrevMappingEntry> for CacheRequest<'_, T> {
    fn cachelib(&self) -> &CachelibHandler<BonsaiGlobalrevMappingEntry> {
        let (_, _, mapping) = self;
        &mapping.cachelib
    }

    fn keygen(&self) -> &KeyGen {
        let (_, _, mapping) = self;
        &mapping.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        let (_, _, mapping) = self;
        &mapping.memcache
    }

    fn cache_determinator(&self, _: &BonsaiGlobalrevMappingEntry) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::NoTtl)
    }

    caching_ext::impl_singleton_stats!("bonsai_globalrev_mapping");
}

#[async_trait]
impl<T> KeyedEntityStore<ChangesetId, BonsaiGlobalrevMappingEntry> for CacheRequest<'_, T>
where
    T: BonsaiGlobalrevMapping,
{
    fn get_cache_key(&self, key: &ChangesetId) -> String {
        let (_, repo_id, _) = self;
        format!("{}.bonsai.{}", repo_id, key)
    }

    async fn get_from_db(
        &self,
        keys: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, BonsaiGlobalrevMappingEntry>, Error> {
        let (ctx, repo_id, mapping) = self;

        let res = mapping
            .inner
            .get(
                ctx,
                *repo_id,
                BonsaisOrGlobalrevs::Bonsai(keys.into_iter().collect()),
            )
            .await
            .with_context(|| "Error fetching globalrevs from bonsais from SQL")?;

        Result::<_, Error>::Ok(res.into_iter().map(|e| (e.bcs_id, e)).collect())
    }
}

#[async_trait]
impl<T> KeyedEntityStore<Globalrev, BonsaiGlobalrevMappingEntry> for CacheRequest<'_, T>
where
    T: BonsaiGlobalrevMapping,
{
    fn get_cache_key(&self, key: &Globalrev) -> String {
        let (_, repo_id, _) = self;
        format!("{}.globalrev.{}", repo_id, key.id())
    }

    async fn get_from_db(
        &self,
        keys: HashSet<Globalrev>,
    ) -> Result<HashMap<Globalrev, BonsaiGlobalrevMappingEntry>, Error> {
        let (ctx, repo_id, mapping) = self;

        let res = mapping
            .inner
            .get(
                ctx,
                *repo_id,
                BonsaisOrGlobalrevs::Globalrev(keys.into_iter().collect()),
            )
            .await
            .with_context(|| "Error fetching bonsais from globalrevs from SQL")?;

        Result::<_, Error>::Ok(res.into_iter().map(|e| (e.globalrev, e)).collect())
    }
}
