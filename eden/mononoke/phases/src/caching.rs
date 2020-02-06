/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::{Phase, Phases, SqlPhases};
use anyhow::Error;
use bytes::Bytes;
use caching_ext::{
    CacheDisposition, CachelibHandler, GetOrFillMultipleFromCacheLayers, McErrorKind, McResult,
};
use cloned::cloned;
use context::CoreContext;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use memcache::{KeyGen, MemcacheClient};
use mononoke_types::{ChangesetId, RepositoryId};
use stats::prelude::*;
use std::{collections::HashSet, convert::TryInto, sync::Arc, time::Duration};

define_stats! {
    prefix = "mononoke.phases";
    memcache_hit: timeseries("memcache.hit"; Rate, Sum),
    memcache_miss: timeseries("memcache.miss"; Rate, Sum),
    memcache_internal_err: timeseries("memcache.internal_err"; Rate, Sum),
    memcache_deserialize_err: timeseries("memcache.deserialize_err"; Rate, Sum),
}

// 6 hours in sec
const TTL_DRAFT_SEC: u64 = 21600;

pub fn get_cache_key(repo_id: RepositoryId, cs_id: &ChangesetId) -> String {
    format!("{}.{}", repo_id.prefix(), cs_id)
}

pub struct Caches {
    pub memcache: MemcacheClient, // Memcache Client for temporary caching
    pub cache_pool: CachelibHandler<Phase>,
    pub keygen: KeyGen,
}

pub struct CachingPhases {
    phases_store: SqlPhases,
    caches: Arc<Caches>,
}

impl CachingPhases {
    pub fn new(phases_store: SqlPhases, caches: Arc<Caches>) -> Self {
        Self {
            phases_store,
            caches,
        }
    }

    fn get_cacher(
        &self,
        ctx: &CoreContext,
        ephemeral_derive: bool,
    ) -> GetOrFillMultipleFromCacheLayers<ChangesetId, Phase> {
        let repo_id = self.phases_store.get_repoid();
        let report_mc_result = |res: McResult<()>| {
            match res {
                Ok(_) => STATS::memcache_hit.add_value(1),
                Err(McErrorKind::MemcacheInternal) => STATS::memcache_internal_err.add_value(1),
                Err(McErrorKind::Missing) => STATS::memcache_miss.add_value(1),
                Err(McErrorKind::Deserialization) => STATS::memcache_deserialize_err.add_value(1),
            };
        };

        cloned!(self.phases_store);
        let get_from_db = {
            cloned!(ctx);
            move |cs_ids: HashSet<ChangesetId>| {
                phases_store
                    .get_public(ctx.clone(), cs_ids.into_iter().collect(), ephemeral_derive)
                    .map(move |public| public.into_iter().map(|key| (key, Phase::Public)).collect())
                    .boxify()
            }
        };

        let determinator = if ephemeral_derive {
            do_not_cache_determinator
        } else {
            phase_caching_determinator
        };

        GetOrFillMultipleFromCacheLayers {
            repo_id,
            get_cache_key: Arc::new(get_cache_key),
            cachelib: self.caches.cache_pool.clone().into(),
            keygen: self.caches.keygen.clone(),
            memcache: self.caches.memcache.clone().into(),
            deserialize: Arc::new(|buf| buf.try_into().map_err(|_| ())),
            serialize: Arc::new(|phase| Bytes::from(phase.to_string())),
            report_mc_result: Arc::new(report_mc_result),
            get_from_db: Arc::new(get_from_db),
            determinator,
        }
    }
}

fn phase_caching_determinator(phase: &Phase) -> CacheDisposition {
    if phase == &Phase::Public {
        CacheDisposition::Cache
    } else {
        CacheDisposition::CacheWithTtl(Duration::from_secs(TTL_DRAFT_SEC))
    }
}

fn do_not_cache_determinator(_phase: &Phase) -> CacheDisposition {
    CacheDisposition::Ignore
}

impl Phases for CachingPhases {
    fn get_public(
        &self,
        ctx: CoreContext,
        cs_ids: Vec<ChangesetId>,
        ephemeral_derive: bool,
    ) -> BoxFuture<HashSet<ChangesetId>, Error> {
        let cacher = self.get_cacher(&ctx, ephemeral_derive);
        cacher
            .run(cs_ids.into_iter().collect())
            .map(|cs_to_phase| {
                cs_to_phase
                    .into_iter()
                    .filter_map(|(key, value)| {
                        if value == Phase::Public {
                            Some(key)
                        } else {
                            None
                        }
                    })
                    .collect()
            })
            .boxify()
    }

    fn add_reachable_as_public(
        &self,
        ctx: CoreContext,
        heads: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error> {
        let cacher = self.get_cacher(&ctx, false);
        self.phases_store
            .add_reachable_as_public(ctx, heads)
            .map(move |marked| {
                cacher.fill_caches(marked.iter().map(|csid| (*csid, Phase::Public)).collect());
                marked
            })
            .boxify()
    }

    fn get_sql_phases(&self) -> &SqlPhases {
        &self.phases_store
    }
}
