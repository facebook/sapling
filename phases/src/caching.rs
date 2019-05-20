// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::errors::*;
use crate::{Phase, Phases};
use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use futures::{future, stream, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use memcache::{KeyGen, MemcacheClient};
use mononoke_types::{ChangesetId, RepositoryId};
use stats::{define_stats, Timeseries};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use try_from::TryInto;

// Memcache constants, should be changed when we want to invalidate memcache
// entries
const MC_CODEVER: u32 = 0;
const MC_SITEVER: u32 = 0;

define_stats! {
    prefix = "mononoke.phases";
    memcache_hit: timeseries("memcache.hit"; RATE, SUM),
    memcache_miss: timeseries("memcache.miss"; RATE, SUM),
    memcache_internal_err: timeseries("memcache.internal_err"; RATE, SUM),
}

// 6 hours in sec
const TTL_DRAFT_SEC: u64 = 21600;

pub fn get_cache_key(repo_id: RepositoryId, cs_id: ChangesetId) -> String {
    format!("{}.{}", repo_id.prefix(), cs_id)
}

pub struct CachingPhases {
    phases_store: Arc<dyn Phases>, // phases_store is the underlying persistent storage (db)
    memcache: MemcacheClient,      // Memcache Client for temporary caching
    keygen: KeyGen,
}

impl CachingPhases {
    pub fn new(phases_store: Arc<dyn Phases>) -> Self {
        let key_prefix = "scm.mononoke.phases";
        Self {
            phases_store,
            memcache: MemcacheClient::new(),
            keygen: KeyGen::new(key_prefix, MC_CODEVER, MC_SITEVER),
        }
    }
}

impl Phases for CachingPhases {
    fn get_public(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_ids: Vec<ChangesetId>,
    ) -> BoxFuture<HashSet<ChangesetId>, Error> {
        cloned!(self.keygen, self.memcache, self.phases_store);
        let repo_id = repo.get_repoid();
        get_phases_from_memcache(&memcache, &keygen, repo_id, cs_ids.clone())
            .and_then(move |phases_memcache| {
                let unknown: Vec<_> = cs_ids
                    .into_iter()
                    .filter(|csid| !phases_memcache.contains_key(csid))
                    .collect();
                let public_memcache = phases_memcache
                    .into_iter()
                    .filter_map(
                        |(csid, p)| {
                            if p == Phase::Public {
                                Some(csid)
                            } else {
                                None
                            }
                        },
                    )
                    .collect();
                if unknown.is_empty() {
                    return future::ok(public_memcache).left_future();
                }
                phases_store
                    .get_public(ctx, repo, unknown)
                    .and_then(move |public_store| {
                        set_phases_to_memcache(
                            &memcache,
                            &keygen,
                            repo_id,
                            public_store
                                .iter()
                                .map(|csid| (*csid, Phase::Public))
                                .collect(),
                        )
                        .map(move |_| public_store.into_iter().chain(public_memcache).collect())
                    })
                    .right_future()
            })
            .boxify()
    }

    fn add_reachable_as_public(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        heads: Vec<ChangesetId>,
    ) -> BoxFuture<Vec<ChangesetId>, Error> {
        cloned!(self.keygen, self.memcache);
        let repoid = repo.get_repoid();
        self.phases_store
            .add_reachable_as_public(ctx, repo, heads)
            .and_then(move |marked| {
                set_phases_to_memcache(
                    &memcache,
                    &keygen,
                    repoid,
                    marked.iter().map(|csid| (*csid, Phase::Public)).collect(),
                )
                .map(move |_| marked)
            })
            .boxify()
    }
}

// Memcache getter
fn get_phase_from_memcache(
    memcache: &MemcacheClient,
    keygen: &KeyGen,
    repo_id: RepositoryId,
    cs_id: ChangesetId,
) -> impl Future<Item = Option<Phase>, Error = Error> {
    memcache
        .get(keygen.key(get_cache_key(repo_id, cs_id)))
        .map(|val| match val {
            Some(x) => {
                STATS::memcache_hit.add_value(1);
                x.try_into().ok()
            }
            _ => {
                STATS::memcache_miss.add_value(1);
                None
            }
        })
        .then(move |res| match res {
            Err(_) => {
                STATS::memcache_miss.add_value(1);
                Ok(None)
            }
            Ok(res) => Ok(res),
        })
}

// Memcache getter
// Memcache client doesn't have bulk api.
fn get_phases_from_memcache(
    memcache: &MemcacheClient,
    keygen: &KeyGen,
    repo_id: RepositoryId,
    cs_ids: Vec<ChangesetId>,
) -> impl Future<Item = HashMap<ChangesetId, Phase>, Error = Error> {
    stream::futures_unordered(cs_ids.into_iter().map(move |cs_id| {
        cloned!(memcache, keygen, repo_id);
        get_phase_from_memcache(&memcache, &keygen, repo_id, cs_id)
            .map(move |maybe_phase| maybe_phase.map(move |phase| (cs_id, phase)))
    }))
    .collect()
    .map(|vec| vec.into_iter().flatten().collect())
}

// Memcache setter (with TTL)
fn set_phase_to_memcache(
    memcache: &MemcacheClient,
    keygen: &KeyGen,
    repo_id: RepositoryId,
    cs_id: ChangesetId,
    phase: &Phase,
) -> impl Future<Item = (), Error = Error> {
    match phase {
        Phase::Draft => memcache
            .set_with_ttl(
                keygen.key(get_cache_key(repo_id, cs_id)),
                phase.to_string(),
                Duration::from_secs(TTL_DRAFT_SEC),
            )
            .left_future(),
        _ => memcache
            .set(keygen.key(get_cache_key(repo_id, cs_id)), phase.to_string())
            .right_future(),
    }
    .then(move |res| match res {
        Err(_) => {
            STATS::memcache_miss.add_value(1);
            Ok(())
        }
        Ok(_res) => Ok(()),
    })
}

// Memcache setter (with TTL)
// Memcache client doesn't have bulk api.
fn set_phases_to_memcache(
    memcache: &MemcacheClient,
    keygen: &KeyGen,
    repo_id: RepositoryId,
    phases: Vec<(ChangesetId, Phase)>,
) -> impl Future<Item = (), Error = Error> + 'static {
    cloned!(memcache, keygen, repo_id);
    stream::futures_unordered(
        phases.into_iter().map(|(cs_id, phase)| {
            set_phase_to_memcache(&memcache, &keygen, repo_id, cs_id, &phase)
        }),
    )
    .collect()
    .map(|_| ())
}
