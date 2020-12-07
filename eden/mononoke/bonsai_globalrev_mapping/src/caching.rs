/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{Context as _, Error};
use async_trait::async_trait;
use bytes::Bytes;
use cachelib::VolatileLruCachePool;
use caching_ext::{
    cache_all_determinator, CachelibHandler, GetOrFillMultipleFromCacheLayers, McResult,
    MemcacheHandler,
};
use cloned::cloned;
use fbinit::FacebookInit;
use fbthrift::compact_protocol;
use futures::{
    compat::Future01CompatExt,
    future::{FutureExt, TryFutureExt},
};
use futures_ext::{BoxFuture as OldBoxFuture, FutureExt as _};
use memcache::{KeyGen, MemcacheClient};
use mononoke_types::{ChangesetId, Globalrev, RepositoryId};
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;
use std::sync::Arc;

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
    async fn bulk_import(&self, entries: &[BonsaiGlobalrevMappingEntry]) -> Result<(), Error> {
        self.inner.bulk_import(entries).await
    }

    async fn get(
        &self,
        repo_id: RepositoryId,
        objects: BonsaisOrGlobalrevs,
    ) -> Result<Vec<BonsaiGlobalrevMappingEntry>, Error> {
        let res = match objects {
            BonsaisOrGlobalrevs::Bonsai(cs_ids) => {
                let get_from_db = {
                    cloned!(self.inner);
                    move |keys: HashSet<ChangesetId>| -> OldBoxFuture<HashMap<ChangesetId, BonsaiGlobalrevMappingEntry>, Error> {
                        cloned!(inner);
                        async move {
                            let res = inner.get(repo_id, BonsaisOrGlobalrevs::Bonsai(keys.into_iter().collect()))
                                .await
                                .with_context(|| "Error fetching globalrevs from bonsais from SQL")?;
                            Result::<_, Error>::Ok(res.into_iter().map(|e| (e.bcs_id, e)).collect())
                        }.boxed().compat().boxify()
                    }
                };

                let params = GetOrFillMultipleFromCacheLayers {
                    repo_id,
                    get_cache_key: Arc::new(get_bonsai_cache_key),
                    cachelib: self.cachelib.clone(),
                    keygen: self.keygen.clone(),
                    memcache: self.memcache.clone(),
                    deserialize: Arc::new(memcache_deserialize),
                    serialize: Arc::new(memcache_serialize),
                    report_mc_result: Arc::new(report_mc_result),
                    get_from_db: Arc::new(get_from_db),
                    determinator: cache_all_determinator::<BonsaiGlobalrevMappingEntry>,
                };

                params
                    .run(cs_ids.into_iter().collect())
                    .compat()
                    .await
                    .with_context(|| "Error fetching globalrevs via cache")?
                    .into_iter()
                    .map(|(_, val)| val)
                    .collect()
            }
            BonsaisOrGlobalrevs::Globalrev(globalrevs) => {
                let get_from_db = {
                    cloned!(self.inner);
                    move |keys: HashSet<Globalrev>| -> OldBoxFuture<HashMap<Globalrev, BonsaiGlobalrevMappingEntry>, Error> {
                        cloned!(inner);
                        async move {
                            let res = inner.get(repo_id, BonsaisOrGlobalrevs::Globalrev(keys.into_iter().collect()))
                                .await
                                .with_context(|| "Error fetching bonsais from globalrevs from SQL")?;
                            Result::<_, Error>::Ok(res.into_iter().map(|e| (e.globalrev, e)).collect())
                        }.boxed().compat().boxify()
                    }
                };

                let params = GetOrFillMultipleFromCacheLayers {
                    repo_id,
                    get_cache_key: Arc::new(get_globalrev_cache_key),
                    cachelib: self.cachelib.clone(),
                    keygen: self.keygen.clone(),
                    memcache: self.memcache.clone(),
                    deserialize: Arc::new(memcache_deserialize),
                    serialize: Arc::new(memcache_serialize),
                    report_mc_result: Arc::new(report_mc_result),
                    get_from_db: Arc::new(get_from_db),
                    determinator: cache_all_determinator::<BonsaiGlobalrevMappingEntry>,
                };

                params
                    .run(globalrevs.into_iter().collect())
                    .compat()
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
        repo_id: RepositoryId,
        globalrev: Globalrev,
    ) -> Result<Option<Globalrev>, Error> {
        self.inner.get_closest_globalrev(repo_id, globalrev).await
    }

    async fn get_max(&self, repo_id: RepositoryId) -> Result<Option<Globalrev>, Error> {
        self.inner.get_max(repo_id).await
    }
}

fn get_bonsai_cache_key(repo_id: RepositoryId, cs: &ChangesetId) -> String {
    format!("{}.bonsai.{}", repo_id, cs)
}

fn get_globalrev_cache_key(repo_id: RepositoryId, g: &Globalrev) -> String {
    format!("{}.globalrev.{}", repo_id, g.id())
}

fn memcache_deserialize(bytes: Bytes) -> Result<BonsaiGlobalrevMappingEntry, ()> {
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

fn memcache_serialize(entry: &BonsaiGlobalrevMappingEntry) -> Bytes {
    let entry = thrift::BonsaiGlobalrevMappingEntry {
        repo_id: entry.repo_id.id(),
        bcs_id: entry.bcs_id.into_thrift(),
        globalrev: entry
            .globalrev
            .id()
            .try_into()
            .expect("Globalrevs must fit within a i64"),
    };
    compact_protocol::serialize(&entry)
}

fn report_mc_result(_: McResult<()>) {}
