/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use super::{BonsaiHgMapping, BonsaiHgMappingEntry, BonsaiOrHgChangesetIds};
use anyhow::Error;
use bonsai_hg_mapping_entry_thrift as thrift;
use bytes::Bytes;
use cachelib::VolatileLruCachePool;
use caching_ext::{
    CachelibHandler, GetOrFillMultipleFromCacheLayers, McErrorKind, McResult, MemcacheHandler,
};
use cloned::cloned;
use context::CoreContext;
use fbinit::FacebookInit;
use fbthrift::compact_protocol;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use heapsize_derive::HeapSizeOf;
use iobuf::IOBuf;
use memcache::{KeyGen, MemcacheClient};
use mercurial_types::HgChangesetId;
use mononoke_types::{ChangesetId, RepositoryId};
use stats::{define_stats, Timeseries};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

define_stats! {
    prefix = "mononoke.bonsai_hg_mapping";
    memcache_hit: timeseries("memcache.hit"; RATE, SUM),
    memcache_miss: timeseries("memcache.miss"; RATE, SUM),
    memcache_internal_err: timeseries("memcache.internal_err"; RATE, SUM),
    memcache_deserialize_err: timeseries("memcache.deserialize_err"; RATE, SUM),
}

/// Used for cache key generation
#[derive(Debug, Clone, Eq, PartialEq, Hash, HeapSizeOf)]
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
            memcache: MemcacheClient::new(fb).into(),
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

        KeyGen::new(
            key_prefix,
            thrift::MC_CODEVER as u32,
            thrift::MC_SITEVER as u32,
        )
    }
}

fn memcache_deserialize(buf: IOBuf) -> Result<BonsaiHgMappingEntry, ()> {
    let bytes: Bytes = buf.into();

    let thrift_entry = compact_protocol::deserialize(bytes).map_err(|_| ());
    thrift_entry.and_then(|entry| BonsaiHgMappingEntry::from_thrift(entry).map_err(|_| ()))
}

fn memcache_serialize(entry: &BonsaiHgMappingEntry) -> Bytes {
    compact_protocol::serialize(&entry.clone().into_thrift())
}

impl BonsaiHgMapping for CachingBonsaiHgMapping {
    fn add(&self, ctx: CoreContext, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error> {
        self.mapping.add(ctx, entry)
    }

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs: BonsaiOrHgChangesetIds,
    ) -> BoxFuture<Vec<BonsaiHgMappingEntry>, Error> {
        let from_bonsai;
        let keys: HashSet<_> = match cs {
            BonsaiOrHgChangesetIds::Bonsai(cs) => {
                from_bonsai = true;
                cs.into_iter().map(BonsaiOrHgChangesetId::Bonsai).collect()
            }
            BonsaiOrHgChangesetIds::Hg(cs) => {
                from_bonsai = false;
                cs.into_iter().map(BonsaiOrHgChangesetId::Hg).collect()
            }
        };

        let report_mc_result = |res: McResult<()>| {
            match res {
                Ok(_) => STATS::memcache_hit.add_value(1),
                Err(McErrorKind::MemcacheInternal) => STATS::memcache_internal_err.add_value(1),
                Err(McErrorKind::Missing) => STATS::memcache_miss.add_value(1),
                Err(McErrorKind::Deserialization) => STATS::memcache_deserialize_err.add_value(1),
            };
        };
        cloned!(self.mapping);
        let get_from_db = move |keys: HashSet<BonsaiOrHgChangesetId>| -> BoxFuture<
            HashMap<BonsaiOrHgChangesetId, BonsaiHgMappingEntry>,
            Error,
        > {
            let mut bcs_ids = vec![];
            let mut hg_cs_ids = vec![];
            for key in keys {
                match key {
                    BonsaiOrHgChangesetId::Bonsai(bcs_id) => {
                        bcs_ids.push(bcs_id);
                    }
                    BonsaiOrHgChangesetId::Hg(hg_cs_id) => {
                        hg_cs_ids.push(hg_cs_id);
                    }
                }
            }

            let bonsai_or_hg_csids = if from_bonsai {
                assert!(hg_cs_ids.is_empty());
                BonsaiOrHgChangesetIds::Bonsai(bcs_ids)
            } else {
                assert!(bcs_ids.is_empty());
                BonsaiOrHgChangesetIds::Hg(hg_cs_ids)
            };

            mapping
                .get(ctx.clone(), repo_id, bonsai_or_hg_csids)
                .map(move |mapping_entries| {
                    mapping_entries
                        .into_iter()
                        .map(|entry| {
                            if from_bonsai {
                                (BonsaiOrHgChangesetId::Bonsai(entry.bcs_id), entry)
                            } else {
                                (BonsaiOrHgChangesetId::Hg(entry.hg_cs_id), entry)
                            }
                        })
                        .collect()
                })
                .boxify()
        };

        let params = GetOrFillMultipleFromCacheLayers {
            repo_id,
            get_cache_key: Arc::new(get_cache_key),
            cachelib: self.cache_pool.clone().into(),
            keygen: self.keygen.clone(),
            memcache: self.memcache.clone(),
            deserialize: Arc::new(memcache_deserialize),
            serialize: Arc::new(memcache_serialize),
            report_mc_result: Arc::new(report_mc_result),
            get_from_db: Arc::new(get_from_db),
        };

        params
            .run(keys)
            .map(|map| map.into_iter().map(|(_, val)| val).collect())
            .boxify()
    }
}

fn get_cache_key(repo_id: RepositoryId, cs: &BonsaiOrHgChangesetId) -> String {
    format!("{}.{:?}", repo_id.prefix(), cs).to_string()
}
