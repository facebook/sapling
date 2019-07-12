// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use super::{ChangesetEntry, ChangesetInsert, Changesets};
use bytes::Bytes;
use cachelib;
#[cfg(test)]
use caching_ext::MockStoreStats;
use caching_ext::{
    CachelibHandler, GetOrFillMultipleFromCacheLayers, McErrorKind, McResult, MemcacheHandler,
};
use changeset_entry_thrift as thrift;
use context::CoreContext;
use futures::Future;
use futures_ext::{BoxFuture, FutureExt};
use iobuf::IOBuf;
use maplit::hashset;
use memcache::{KeyGen, MemcacheClient};
use mononoke_types::{ChangesetId, RepositoryId};
use rust_thrift::compact_protocol;
use stats::{define_stats, Timeseries};
use std::collections::HashSet;
use std::iter::FromIterator;
use std::sync::Arc;

use crate::errors::*;

define_stats! {
    prefix = "mononoke.changesets";
    memcache_hit: timeseries("memcache.hit"; RATE, SUM),
    memcache_miss: timeseries("memcache.miss"; RATE, SUM),
    memcache_internal_err: timeseries("memcache.internal_err"; RATE, SUM),
    memcache_deserialize_err: timeseries("memcache.deserialize_err"; RATE, SUM),
}

pub fn get_cache_key(repo_id: RepositoryId, cs_id: &ChangesetId) -> String {
    format!("{}.{}", repo_id.prefix(), cs_id).to_string()
}

pub struct CachingChangesets {
    changesets: Arc<dyn Changesets>,
    cachelib: CachelibHandler<ChangesetEntry>,
    memcache: MemcacheHandler,
    keygen: KeyGen,
}

fn get_keygen() -> KeyGen {
    let key_prefix = "scm.mononoke.changesets";

    KeyGen::new(
        key_prefix,
        thrift::MC_CODEVER as u32,
        thrift::MC_SITEVER as u32,
    )
}

impl CachingChangesets {
    pub fn new(
        changesets: Arc<dyn Changesets>,
        cache_pool: cachelib::VolatileLruCachePool,
    ) -> Self {
        Self {
            changesets,
            cachelib: cache_pool.into(),
            memcache: MemcacheClient::new().into(),
            keygen: get_keygen(),
        }
    }

    #[cfg(test)]
    pub fn mocked(changesets: Arc<dyn Changesets>) -> Self {
        let cachelib = CachelibHandler::create_mock();
        let memcache = MemcacheHandler::create_mock();

        Self {
            changesets,
            cachelib,
            memcache,
            keygen: get_keygen(),
        }
    }

    #[cfg(test)]
    pub fn fork_cachelib(&self) -> Self {
        Self {
            changesets: self.changesets.clone(),
            cachelib: CachelibHandler::create_mock(),
            memcache: self.memcache.clone(),
            keygen: self.keygen.clone(),
        }
    }

    #[cfg(test)]
    pub fn cachelib_stats(&self) -> MockStoreStats {
        match self.cachelib {
            CachelibHandler::Real(_) => unimplemented!(),
            CachelibHandler::Mock(ref mock) => mock.stats(),
        }
    }

    #[cfg(test)]
    pub fn memcache_stats(&self) -> MockStoreStats {
        match self.memcache {
            MemcacheHandler::Real(_) => unimplemented!(),
            MemcacheHandler::Mock(ref mock) => mock.stats(),
        }
    }

    fn req(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
    ) -> GetOrFillMultipleFromCacheLayers<ChangesetId, ChangesetEntry> {
        let get_cache_key = Arc::new(get_cache_key);

        let changesets = self.changesets.clone();

        let get_from_db = move |keys: HashSet<ChangesetId>| {
            changesets
                .get_many(ctx.clone(), repo_id, keys.into_iter().collect())
                .map(|entries| entries.into_iter().map(|e| (e.cs_id, e)).collect())
                .boxify()
        };

        GetOrFillMultipleFromCacheLayers {
            repo_id,
            get_cache_key,
            cachelib: self.cachelib.clone(),
            keygen: self.keygen.clone(),
            memcache: self.memcache.clone(),
            deserialize: Arc::new(deserialize_changeset_entry),
            serialize: Arc::new(serialize_changeset_entry),
            report_mc_result: Arc::new(report_mc_result),
            get_from_db: Arc::new(get_from_db),
        }
    }
}

impl Changesets for CachingChangesets {
    fn add(&self, ctx: CoreContext, cs: ChangesetInsert) -> BoxFuture<bool, Error> {
        self.changesets.add(ctx, cs)
    }

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<ChangesetEntry>, Error> {
        self.req(ctx, repo_id)
            .run(hashset![cs_id])
            .map(move |mut map| map.remove(&cs_id))
            .boxify()
    }

    fn get_many(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_ids: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetEntry>, Error> {
        let keys = HashSet::from_iter(cs_ids);

        self.req(ctx, repo_id)
            .run(keys)
            .map(|map| map.into_iter().map(|(_, val)| val).collect())
            .boxify()
    }
}

fn deserialize_changeset_entry(buf: IOBuf) -> ::std::result::Result<ChangesetEntry, ()> {
    let bytes: Bytes = buf.into();

    compact_protocol::deserialize(bytes)
        .and_then(|entry| ChangesetEntry::from_thrift(entry))
        .map_err(|_| ())
}

fn serialize_changeset_entry(entry: &ChangesetEntry) -> Bytes {
    compact_protocol::serialize(&entry.clone().into_thrift())
}

fn report_mc_result<T>(res: McResult<T>) {
    match res {
        Ok(..) => STATS::memcache_hit.add_value(1),
        Err(McErrorKind::MemcacheInternal) => STATS::memcache_internal_err.add_value(1),
        Err(McErrorKind::Missing) => STATS::memcache_miss.add_value(1),
        Err(McErrorKind::Deserialization) => STATS::memcache_deserialize_err.add_value(1),
    };
}
