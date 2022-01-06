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
use mononoke_types::{ChangesetId, RepositoryId, Svnrev};
use std::collections::{HashMap, HashSet};

use bonsai_svnrev_mapping_thrift as thrift;

use super::{BonsaiSvnrevMapping, BonsaiSvnrevMappingEntry, BonsaisOrSvnrevs};

#[derive(Clone)]
pub struct CachingBonsaiSvnrevMapping<T> {
    cachelib: CachelibHandler<BonsaiSvnrevMappingEntry>,
    memcache: MemcacheHandler,
    keygen: KeyGen,
    inner: T,
}

impl<T> CachingBonsaiSvnrevMapping<T> {
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
        let key_prefix = "scm.mononoke.bonsai_svnrev_mapping";

        KeyGen::new(
            key_prefix,
            thrift::MC_CODEVER as u32,
            thrift::MC_SITEVER as u32,
        )
    }

    pub fn cachelib(&self) -> &CachelibHandler<BonsaiSvnrevMappingEntry> {
        &self.cachelib
    }
}

#[async_trait]
impl<T> BonsaiSvnrevMapping for CachingBonsaiSvnrevMapping<T>
where
    T: BonsaiSvnrevMapping + Clone + Sync + Send + 'static,
{
    async fn bulk_import(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiSvnrevMappingEntry],
    ) -> Result<(), Error> {
        self.inner.bulk_import(ctx, entries).await
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        objects: BonsaisOrSvnrevs,
    ) -> Result<Vec<BonsaiSvnrevMappingEntry>, Error> {
        let ctx = (ctx, repo_id, self);

        let res = match objects {
            BonsaisOrSvnrevs::Bonsai(cs_ids) => get_or_fill(ctx, cs_ids.into_iter().collect())
                .await
                .with_context(|| "Error fetching svnrevs via cache")?
                .into_iter()
                .map(|(_, val)| val)
                .collect(),
            BonsaisOrSvnrevs::Svnrev(svnrevs) => get_or_fill(ctx, svnrevs.into_iter().collect())
                .await
                .with_context(|| "Error fetching bonsais via cache")?
                .into_iter()
                .map(|(_, val)| val)
                .collect(),
        };


        Ok(res)
    }
}

impl MemcacheEntity for BonsaiSvnrevMappingEntry {
    fn serialize(&self) -> Bytes {
        let entry = thrift::BonsaiSvnrevMappingEntry {
            repo_id: self.repo_id.id(),
            bcs_id: self.bcs_id.into_thrift(),
            svnrev: self
                .svnrev
                .id()
                .try_into()
                .expect("Svnrevs must fit within a i64"),
        };
        compact_protocol::serialize(&entry)
    }

    fn deserialize(bytes: Bytes) -> Result<Self, ()> {
        let thrift::BonsaiSvnrevMappingEntry {
            repo_id,
            bcs_id,
            svnrev,
        } = compact_protocol::deserialize(bytes).map_err(|_| ())?;

        let repo_id = RepositoryId::new(repo_id);
        let bcs_id = ChangesetId::from_thrift(bcs_id).map_err(|_| ())?;
        let svnrev = Svnrev::new(svnrev.try_into().map_err(|_| ())?);

        Ok(BonsaiSvnrevMappingEntry {
            repo_id,
            bcs_id,
            svnrev,
        })
    }
}

type CacheRequest<'a, T> = (
    &'a CoreContext,
    RepositoryId,
    &'a CachingBonsaiSvnrevMapping<T>,
);

impl<T> EntityStore<BonsaiSvnrevMappingEntry> for CacheRequest<'_, T> {
    fn cachelib(&self) -> &CachelibHandler<BonsaiSvnrevMappingEntry> {
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

    fn cache_determinator(&self, _: &BonsaiSvnrevMappingEntry) -> CacheDisposition {
        CacheDisposition::Cache(CacheTtl::NoTtl)
    }

    caching_ext::impl_singleton_stats!("bonsai_svnrev_mapping");
}

#[async_trait]
impl<T> KeyedEntityStore<ChangesetId, BonsaiSvnrevMappingEntry> for CacheRequest<'_, T>
where
    T: BonsaiSvnrevMapping,
{
    fn get_cache_key(&self, key: &ChangesetId) -> String {
        let (_, repo_id, _) = self;
        format!("{}.bonsai.{}", repo_id, key)
    }

    async fn get_from_db(
        &self,
        keys: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, BonsaiSvnrevMappingEntry>, Error> {
        let (ctx, repo_id, mapping) = self;

        let res = mapping
            .inner
            .get(
                ctx,
                *repo_id,
                BonsaisOrSvnrevs::Bonsai(keys.into_iter().collect()),
            )
            .await
            .with_context(|| "Error fetching svnrevs from bonsais from SQL")?;

        Result::<_, Error>::Ok(res.into_iter().map(|e| (e.bcs_id, e)).collect())
    }
}

#[async_trait]
impl<T> KeyedEntityStore<Svnrev, BonsaiSvnrevMappingEntry> for CacheRequest<'_, T>
where
    T: BonsaiSvnrevMapping,
{
    fn get_cache_key(&self, key: &Svnrev) -> String {
        let (_, repo_id, _) = self;
        format!("{}.svnrev.{}", repo_id, key.id())
    }

    async fn get_from_db(
        &self,
        keys: HashSet<Svnrev>,
    ) -> Result<HashMap<Svnrev, BonsaiSvnrevMappingEntry>, Error> {
        let (ctx, repo_id, mapping) = self;

        let res = mapping
            .inner
            .get(
                ctx,
                *repo_id,
                BonsaisOrSvnrevs::Svnrev(keys.into_iter().collect()),
            )
            .await
            .with_context(|| "Error fetching bonsais from svnrevs from SQL")?;

        Result::<_, Error>::Ok(res.into_iter().map(|e| (e.svnrev, e)).collect())
    }
}
