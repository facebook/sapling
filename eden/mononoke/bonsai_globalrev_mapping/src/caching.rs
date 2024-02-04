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
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bonsai_globalrev_mapping_thrift as thrift;
use bytes::Bytes;
use caching_ext::get_or_fill;
use caching_ext::CacheDisposition;
use caching_ext::CacheHandlerFactory;
use caching_ext::CacheTtl;
use caching_ext::CachelibHandler;
use caching_ext::EntityStore;
use caching_ext::KeyedEntityStore;
use caching_ext::McErrorKind;
use caching_ext::McResult;
use caching_ext::MemcacheEntity;
use caching_ext::MemcacheHandler;
use context::CoreContext;
use fbthrift::compact_protocol;
use memcache::KeyGen;
use mononoke_types::ChangesetId;
use mononoke_types::Globalrev;
use mononoke_types::RepositoryId;

use super::BonsaiGlobalrevMapping;
use super::BonsaiGlobalrevMappingEntries;
use super::BonsaiGlobalrevMappingEntry;
use super::BonsaisOrGlobalrevs;

#[derive(Abomonation, Clone, Debug, Eq, Hash, PartialEq)]
pub struct BonsaiGlobalrevMappingCacheEntry {
    pub repo_id: RepositoryId,
    pub bcs_id: Option<ChangesetId>,
    pub globalrev: Globalrev,
}

pub struct CachingBonsaiGlobalrevMapping {
    cachelib: CachelibHandler<BonsaiGlobalrevMappingCacheEntry>,
    memcache: MemcacheHandler,
    keygen: KeyGen,
    inner: Arc<dyn BonsaiGlobalrevMapping>,
}

impl CachingBonsaiGlobalrevMapping {
    pub fn new(
        inner: Arc<dyn BonsaiGlobalrevMapping>,
        cache_handler_factory: CacheHandlerFactory,
    ) -> Self {
        Self {
            inner,
            cachelib: cache_handler_factory.cachelib(),
            memcache: cache_handler_factory.memcache(),
            keygen: Self::create_key_gen(),
        }
    }

    pub fn new_test(inner: Arc<dyn BonsaiGlobalrevMapping>) -> Self {
        Self::new(inner, CacheHandlerFactory::Mocked)
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
impl BonsaiGlobalrevMapping for CachingBonsaiGlobalrevMapping {
    fn repo_id(&self) -> RepositoryId {
        self.inner.as_ref().repo_id()
    }

    async fn bulk_import(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGlobalrevMappingEntry],
    ) -> Result<(), Error> {
        self.inner.as_ref().bulk_import(ctx, entries).await
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        objects: BonsaisOrGlobalrevs,
    ) -> Result<BonsaiGlobalrevMappingEntries, Error> {
        let cache_request = (ctx, self);
        let repo_id = self.repo_id();

        let validate_cache_entry = |val: BonsaiGlobalrevMappingCacheEntry| {
            if val.repo_id == repo_id {
                Ok(val)
            } else {
                Err(anyhow!(
                    "Cache returned invalid entry: repo {} returned for query to repo {}",
                    val.repo_id,
                    repo_id
                ))
            }
        };

        let res = match objects {
            BonsaisOrGlobalrevs::Bonsai(cs_ids) => BonsaiGlobalrevMappingEntries {
                cached_data: get_or_fill(&cache_request, cs_ids.into_iter().collect())
                    .await
                    .with_context(|| "Error fetching globalrevs via cache")?
                    .into_values()
                    .map(validate_cache_entry)
                    .collect::<Result<_>>()?,
            },
            BonsaisOrGlobalrevs::Globalrev(globalrevs) => BonsaiGlobalrevMappingEntries {
                cached_data: get_or_fill(&cache_request, globalrevs.into_iter().collect())
                    .await
                    .with_context(|| "Error fetching bonsais via cache")?
                    .into_values()
                    .map(validate_cache_entry)
                    .collect::<Result<_>>()?,
            },
        };

        Ok(res)
    }

    async fn get_closest_globalrev(
        &self,
        ctx: &CoreContext,
        globalrev: Globalrev,
    ) -> Result<Option<Globalrev>, Error> {
        self.inner
            .as_ref()
            .get_closest_globalrev(ctx, globalrev)
            .await
    }

    async fn get_max(&self, ctx: &CoreContext) -> Result<Option<Globalrev>, Error> {
        self.inner.as_ref().get_max(ctx).await
    }

    async fn get_max_custom_repo(
        &self,
        ctx: &CoreContext,
        repo_id: &RepositoryId,
    ) -> Result<Option<Globalrev>, Error> {
        self.inner.as_ref().get_max_custom_repo(ctx, repo_id).await
    }
}

impl MemcacheEntity for BonsaiGlobalrevMappingCacheEntry {
    fn serialize(&self) -> Bytes {
        let entry = thrift::BonsaiGlobalrevMappingEntry {
            repo_id: self.repo_id.id(),
            bcs_id: self.bcs_id.map(|bcs_id| bcs_id.into_thrift()),
            globalrev: self
                .globalrev
                .id()
                .try_into()
                .expect("Globalrevs must fit within a i64"),
        };
        compact_protocol::serialize(&entry)
    }

    fn deserialize(bytes: Bytes) -> McResult<Self> {
        let thrift::BonsaiGlobalrevMappingEntry {
            repo_id,
            bcs_id,
            globalrev,
        } = compact_protocol::deserialize(bytes).map_err(|_| McErrorKind::Deserialization)?;

        let repo_id = RepositoryId::new(repo_id);
        let bcs_id = bcs_id
            .map(|bcs_id| {
                ChangesetId::from_thrift(bcs_id).map_err(|_| McErrorKind::Deserialization)
            })
            .transpose()?;
        let globalrev = Globalrev::new(
            globalrev
                .try_into()
                .map_err(|_| McErrorKind::Deserialization)?,
        );

        Ok(BonsaiGlobalrevMappingCacheEntry {
            repo_id,
            bcs_id,
            globalrev,
        })
    }
}

type CacheRequest<'a> = (&'a CoreContext, &'a CachingBonsaiGlobalrevMapping);

impl EntityStore<BonsaiGlobalrevMappingCacheEntry> for CacheRequest<'_> {
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
impl KeyedEntityStore<ChangesetId, BonsaiGlobalrevMappingCacheEntry> for CacheRequest<'_> {
    fn get_cache_key(&self, key: &ChangesetId) -> String {
        let (_, mapping) = self;
        format!("{}.bonsai.{}", mapping.repo_id(), key)
    }

    async fn get_from_db(
        &self,
        keys: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, BonsaiGlobalrevMappingCacheEntry>, Error> {
        let (ctx, mapping) = self;
        let res = mapping
            .inner
            .as_ref()
            .get(ctx, BonsaisOrGlobalrevs::Bonsai(keys.into_iter().collect()))
            .await
            .with_context(|| "Error fetching globalrevs from bonsais from SQL")?;

        Result::<_, Error>::Ok(
            res.cached_data
                .into_iter()
                .filter_map(|e| e.bcs_id.map(|bcs_id| (bcs_id, e)))
                .collect(),
        )
    }
}

#[async_trait]
impl KeyedEntityStore<Globalrev, BonsaiGlobalrevMappingCacheEntry> for CacheRequest<'_> {
    fn get_cache_key(&self, key: &Globalrev) -> String {
        let (_, mapping) = self;
        format!("{}.globalrev.{}", mapping.repo_id(), key.id())
    }

    async fn get_from_db(
        &self,
        keys: HashSet<Globalrev>,
    ) -> Result<HashMap<Globalrev, BonsaiGlobalrevMappingCacheEntry>, Error> {
        let (ctx, mapping) = self;
        let res = mapping
            .inner
            .as_ref()
            .get(
                ctx,
                BonsaisOrGlobalrevs::Globalrev(keys.into_iter().collect()),
            )
            .await
            .with_context(|| "Error fetching bonsais from globalrevs from SQL")?;

        Result::<_, Error>::Ok(
            res.cached_data
                .into_iter()
                .map(|e| (e.globalrev, e))
                .collect(),
        )
    }
}
