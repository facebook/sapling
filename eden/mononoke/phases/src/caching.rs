/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::Phase;
use caching_ext::{CacheDisposition, CachelibHandler, MemcacheHandler};
use memcache::KeyGen;
use mononoke_types::{ChangesetId, RepositoryId};
use stats::prelude::*;
use std::time::Duration;

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
    pub memcache: MemcacheHandler, // Memcache Client for temporary caching
    pub cache_pool: CachelibHandler<Phase>,
    pub keygen: KeyGen,
}

impl Caches {
    pub fn new_mock(keygen: KeyGen) -> Self {
        Self {
            memcache: MemcacheHandler::create_mock(),
            cache_pool: CachelibHandler::create_mock(),
            keygen,
        }
    }
}

pub fn phase_caching_determinator(phase: &Phase) -> CacheDisposition {
    if phase == &Phase::Public {
        CacheDisposition::Cache
    } else {
        CacheDisposition::CacheWithTtl(Duration::from_secs(TTL_DRAFT_SEC))
    }
}
