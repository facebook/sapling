// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use {Phase, Phases, PhasesHint};
use blobrepo::BlobRepo;
use context::CoreContext;
use errors::*;
use futures::{future, Future};
use futures_ext::{BoxFuture, FutureExt};
use memcache::{KeyGen, MemcacheClient};
use mercurial_types::RepositoryId;
use mononoke_types::ChangesetId;
use stats::Timeseries;
use std::sync::Arc;
use std::time::Duration;
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

pub fn get_cache_key(repo_id: &RepositoryId, cs_id: &ChangesetId) -> String {
    format!("{}.{}", repo_id.prefix(), cs_id)
}

pub struct CachingPhases {
    phases: Arc<Phases>, // Phases is the underlying storage we cache around
    memcache: MemcacheClient,
    keygen: KeyGen,
    phases_hint: PhasesHint,
}

impl CachingPhases {
    pub fn new(phases: Arc<Phases>) -> Self {
        let key_prefix = "scm.mononoke.phases";
        Self {
            phases,
            memcache: MemcacheClient::new(),
            keygen: KeyGen::new(key_prefix, MC_CODEVER, MC_SITEVER),
            phases_hint: PhasesHint::new(),
        }
    }
}

impl Phases for CachingPhases {
    /// Add a new entry to the phases.
    /// Memcache + underlying storage
    /// Returns true if a new changeset was added or the phase has been changed,
    /// returns false if the phase hasn't been changed for the changeset.
    fn add(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_id: ChangesetId,
        phase: Phase,
    ) -> BoxFuture<bool, Error> {
        cloned!(self.keygen, self.memcache, self.phases);
        let repo_id = repo.get_repoid();
        get_phase_from_memcache(&memcache, &keygen, &repo_id, &cs_id)
            .and_then(move |maybe_phase| {
                match maybe_phase {
                    // The phase is already the same in memcache, nothing is needed.
                    Some(ref current_phase) if current_phase == &phase => {
                        future::ok(false).left_future()
                    }
                    _ => {
                        // The phase is missing or different in memcache. Refresh memcache.
                        set_phase_to_memcache(&memcache, &keygen, &repo_id, &cs_id, &phase)
                        // Refresh the underlying persistent storage (currently for public commits only).
                        .and_then(move |_| {
                            if phase == Phase::Public {
                                phases.add(ctx, repo, cs_id, phase)
                            } else {
                                future::ok(true).boxify()
                            }
                        })
                        .right_future()
                    }
                }
            })
            .boxify()
    }

    /// Retrieve the phase specified by this commit, if available.
    /// If not available recalculate it if possible.
    fn get(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<Phase>, Error> {
        cloned!(self.keygen, self.memcache, self.phases, self.phases_hint);
        let repo_id = repo.get_repoid();
        // Look up in the memcache.
        get_phase_from_memcache(&memcache, &keygen, &repo_id, &cs_id)
            .and_then(move |maybe_phase| {
                // The phase is found in memcache, return it.
                if maybe_phase.is_some() {
                    return future::ok(maybe_phase).left_future();
                }
                // The phase is missing in memcache. Try to fetch from the underlying storage.
                phases
                    .get(ctx.clone(), repo.clone(), cs_id)
                    .and_then(move |maybe_phase| {
                        match maybe_phase {
                            // The phase is found. Refresh memcache and return the value.
                            Some(phase) => {
                                set_phase_to_memcache(&memcache, &keygen, &repo_id, &cs_id, &phase)
                                    .map(move |_| Some(phase))
                                    .left_future()
                            }
                            // The phase is not found. Try to calculate it.
                            // It will be error if calculation failed.
                            None => phases_hint
                                .get(ctx.clone(), repo.clone(), cs_id)
                                .and_then(move |phase| {
                                    // The phase is calculated. Refresh memcache.
                                    set_phase_to_memcache(
                                        &memcache,
                                        &keygen,
                                        &repo_id,
                                        &cs_id,
                                        &phase,
                                    ).and_then(move |_| {
                                        // Update the underlying storage (currently public commits only).
                                        // Return the phase.
                                        if phase == Phase::Public {
                                            phases
                                                .add(ctx, repo, cs_id, phase.clone())
                                                .map(|_| Some(phase))
                                                .left_future()
                                        } else {
                                            future::ok(Some(phase)).right_future()
                                        }
                                    })
                                })
                                .right_future(),
                        }
                    })
                    .right_future()
            })
            .boxify()
    }
}

// Memcache getter
fn get_phase_from_memcache(
    memcache: &MemcacheClient,
    keygen: &KeyGen,
    repo_id: &RepositoryId,
    cs_id: &ChangesetId,
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

// Memcache setter (with TTL)
fn set_phase_to_memcache(
    memcache: &MemcacheClient,
    keygen: &KeyGen,
    repo_id: &RepositoryId,
    cs_id: &ChangesetId,
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
    }.then(move |res| match res {
        Err(_) => {
            STATS::memcache_miss.add_value(1);
            Ok(())
        }
        Ok(_res) => Ok(()),
    })
}
