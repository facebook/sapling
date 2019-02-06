// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use super::{changeset_entry_thrift, ChangesetEntry, ChangesetInsert, Changesets};
use cachelib;
use changeset_entry_thrift as thrift;
use context::CoreContext;
use errors::*;
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use memcache::{KeyGen, MemcacheClient, MEMCACHE_VALUE_MAX_SIZE};
use mononoke_types::{ChangesetId, RepositoryId};
use rust_thrift::compact_protocol;
use stats::Timeseries;
use std::sync::Arc;
use tokio;

define_stats! {
    prefix = "mononoke.changesets";
    memcache_hit: timeseries("memcache.hit"; RATE, SUM),
    memcache_miss: timeseries("memcache.miss"; RATE, SUM),
    memcache_internal_err: timeseries("memcache.internal_err"; RATE, SUM),
    memcache_deserialize_err: timeseries("memcache.deserialize_err"; RATE, SUM),
}

pub fn get_cache_key(repo_id: RepositoryId, cs_id: ChangesetId) -> String {
    format!("{}.{}", repo_id.prefix(), cs_id).to_string()
}

pub struct CachingChangests {
    changesets: Arc<Changesets>,
    cache_pool: cachelib::LruCachePool,
    memcache: MemcacheClient,
    keygen: KeyGen,
}

impl CachingChangests {
    pub fn new(changesets: Arc<Changesets>, cache_pool: cachelib::LruCachePool) -> Self {
        let key_prefix = "scm.mononoke.changesets";

        Self {
            changesets,
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

impl Changesets for CachingChangests {
    fn add(&self, ctx: CoreContext, cs: ChangesetInsert) -> BoxFuture<bool, Error> {
        self.changesets.add(ctx, cs)
    }

    fn get(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<ChangesetEntry>, Error> {
        let cache_key = get_cache_key(repo_id, cs_id);

        cloned!(self.changesets, self.keygen, self.memcache);
        cachelib::get_cached_or_fill(&self.cache_pool, cache_key, || {
            get_changeset_from_memcache(&self.memcache, &self.keygen, repo_id, cs_id)
                .then(move |res| match res {
                    Ok(res) => {
                        return future::ok(Some(res)).boxify();
                    }
                    Err(()) => changesets
                        .get(ctx, repo_id, cs_id)
                        .inspect(move |res| {
                            if let Some(cs_entry) = res {
                                schedule_fill_changesets_memcache(
                                    memcache,
                                    keygen,
                                    repo_id,
                                    cs_entry.clone(),
                                )
                            }
                        })
                        .boxify(),
                })
                .boxify()
        })
    }

    fn get_many(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        cs_id: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetEntry>, Error> {
        // TODO(stash): T39204057 add caching
        self.changesets.get_many(ctx, repo_id, cs_id)
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

fn get_mc_key_for_changeset(
    keygen: &KeyGen,
    repo_id: RepositoryId,
    changeset: ChangesetId,
) -> String {
    keygen.key(get_cache_key(repo_id, changeset))
}

fn get_changeset_from_memcache(
    memcache: &MemcacheClient,
    keygen: &KeyGen,
    repo_id: RepositoryId,
    cs_id: ChangesetId,
) -> impl Future<Item = ChangesetEntry, Error = ()> {
    memcache
        .get(get_mc_key_for_changeset(keygen, repo_id, cs_id))
        .map_err(|()| ErrorKind::MemcacheInternal)
        .and_then(|maybe_serialized| maybe_serialized.ok_or(ErrorKind::Missing))
        .and_then(|serialized| {
            let thrift_entry: ::std::result::Result<
                changeset_entry_thrift::ChangesetEntry,
                ErrorKind,
            > = compact_protocol::deserialize(Vec::from(serialized))
                .map_err(|_| ErrorKind::Deserialization);

            let thrift_entry = thrift_entry.and_then(|entry| {
                ChangesetEntry::from_thrift(entry).map_err(|_| ErrorKind::Deserialization)
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

fn schedule_fill_changesets_memcache(
    memcache: MemcacheClient,
    keygen: KeyGen,
    repo_id: RepositoryId,
    changeset: ChangesetEntry,
) {
    let cs_id = changeset.cs_id.clone();
    let serialized = compact_protocol::serialize(&changeset.into_thrift());

    // Quite unlikely that single changeset will be bigger than MEMCACHE_VALUE_MAX_SIZE
    // It's probably not even worth logging it
    if serialized.len() < MEMCACHE_VALUE_MAX_SIZE {
        tokio::spawn(memcache.set(
            get_mc_key_for_changeset(&keygen, repo_id, cs_id),
            serialized,
        ));
    }
}
