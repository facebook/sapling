// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use super::{bonsai_hg_mapping_entry_thrift as thrift, BonsaiHgMapping, BonsaiHgMappingEntry,
            BonsaiOrHgChangesetId};
use cachelib::{get_cached_or_fill, LruCachePool};
use errors::Error;
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use memcache::{KeyGen, MemcacheClient, MEMCACHE_VALUE_MAX_SIZE};
use mercurial_types::RepositoryId;
use rust_thrift::compact_protocol;
use stats::Timeseries;
use std::sync::Arc;
use tokio;

define_stats! {
    prefix = "mononoke.bonsai_hg_mapping";
    memcache_hit: timeseries("memcache.hit"; RATE, SUM),
    memcache_miss: timeseries("memcache.miss"; RATE, SUM),
    memcache_internal_err: timeseries("memcache.internal_err"; RATE, SUM),
    memcache_deserialize_err: timeseries("memcache.deserialize_err"; RATE, SUM),
}

pub struct CachingBonsaiHgMapping {
    mapping: Arc<BonsaiHgMapping>,
    cache_pool: LruCachePool,
    memcache: MemcacheClient,
    keygen: KeyGen,
}

impl CachingBonsaiHgMapping {
    pub fn new(mapping: Arc<BonsaiHgMapping>, cache_pool: LruCachePool) -> Self {
        let key_prefix = "scm.mononoke.bonsai_hg_mapping";

        Self {
            mapping,
            cache_pool,
            memcache: MemcacheClient::new(),
            keygen: KeyGen::new(
                key_prefix,
                thrift::MC_CODEVER as u32,
                thrift::MC_SITEVER as u32,
            ),
        }
    }
}

impl BonsaiHgMapping for CachingBonsaiHgMapping {
    fn add(&self, entry: BonsaiHgMappingEntry) -> BoxFuture<bool, Error> {
        self.mapping.add(entry)
    }

    fn get(
        &self,
        repo_id: RepositoryId,
        cs: BonsaiOrHgChangesetId,
    ) -> BoxFuture<Option<BonsaiHgMappingEntry>, Error> {
        let cache_key = get_cache_key(&repo_id, &cs);
        get_cached_or_fill(&self.cache_pool, cache_key, || {
            cloned!(self.keygen, self.mapping, self.memcache);
            get_mapping_from_memcache(&memcache, &keygen, &repo_id, &cs)
                .then(move |res| match res {
                    Ok(res) => {
                        return future::ok(Some(res)).left_future();
                    }
                    Err(()) => mapping
                        .get(repo_id, cs)
                        .inspect(move |res| {
                            if let Some(cs_entry) = res {
                                schedule_fill_mapping_memcache(
                                    memcache,
                                    keygen,
                                    repo_id,
                                    &cs,
                                    cs_entry.clone(),
                                )
                            }
                        })
                        .right_future(),
                })
                .boxify()
        })
    }
}

// Local error type to help with proper logging metrics
enum ErrorKind {
    // error came from calling memcache API
    MemcacheInternal,
    // value returned from memcache was None
    Missing,
    // deserialization of memcache data to Rust structures via thrift failed
    Deserialization,
}

fn get_cache_key(repo_id: &RepositoryId, cs: &BonsaiOrHgChangesetId) -> String {
    format!("{}.{:?}", repo_id.prefix(), cs).to_string()
}

fn get_mc_key_for_mapping(
    keygen: &KeyGen,
    repo_id: &RepositoryId,
    bonsai_or_hg: &BonsaiOrHgChangesetId,
) -> String {
    keygen.key(get_cache_key(repo_id, bonsai_or_hg))
}

fn get_mapping_from_memcache(
    memcache: &MemcacheClient,
    keygen: &KeyGen,
    repo_id: &RepositoryId,
    bonsai_or_hg: &BonsaiOrHgChangesetId,
) -> impl Future<Item = BonsaiHgMappingEntry, Error = ()> {
    memcache
        .get(get_mc_key_for_mapping(keygen, repo_id, bonsai_or_hg))
        .map_err(|()| ErrorKind::MemcacheInternal)
        .and_then(|maybe_serialized| maybe_serialized.ok_or(ErrorKind::Missing))
        .and_then(|serialized| {
            let thrift_entry: ::std::result::Result<
                thrift::BonsaiHgMappingEntry,
                ErrorKind,
            > = compact_protocol::deserialize(Vec::from(serialized))
                .map_err(|_| ErrorKind::Deserialization);

            let thrift_entry = thrift_entry.and_then(|entry| {
                BonsaiHgMappingEntry::from_thrift(entry).map_err(|_| ErrorKind::Deserialization)
            });
            thrift_entry
        })
        .then(move |res| {
            match res {
                Ok(res) => {
                    STATS::memcache_hit.add_value(1);
                    return Ok(res);
                }
                Err(ErrorKind::MemcacheInternal) => STATS::memcache_internal_err.add_value(1),
                Err(ErrorKind::Missing) => STATS::memcache_miss.add_value(1),
                Err(ErrorKind::Deserialization) => STATS::memcache_deserialize_err.add_value(1),
            }
            Err(())
        })
}

fn schedule_fill_mapping_memcache(
    memcache: MemcacheClient,
    keygen: KeyGen,
    repo_id: RepositoryId,
    bonsai_or_hg: &BonsaiOrHgChangesetId,
    mapping_entry: BonsaiHgMappingEntry,
) {
    let serialized = compact_protocol::serialize(&mapping_entry.into_thrift());

    // Quite unlikely that single changeset id will be bigger than MEMCACHE_VALUE_MAX_SIZE
    // It's probably not even worth logging it
    if serialized.len() < MEMCACHE_VALUE_MAX_SIZE {
        tokio::spawn(memcache.set(
            get_mc_key_for_mapping(&keygen, &repo_id, &bonsai_or_hg),
            serialized,
        ));
    }
}
