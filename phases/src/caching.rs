// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use blobrepo::BlobRepo;
use context::CoreContext;
use errors::*;
use futures::{future, stream, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use memcache::{KeyGen, MemcacheClient};
use mononoke_types::{ChangesetId, RepositoryId};
use reachabilityindex::SkiplistIndex;
use stats::Timeseries;
use std::collections::HashSet;
use std::iter::FromIterator;
use std::sync::Arc;
use std::time::Duration;
use try_from::TryInto;
use {Phase, Phases, PhasesMapping, PhasesReachabilityHint};

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

pub struct CachingHintPhases {
    phases_store: Arc<Phases>, // phases_store is the underlying persistent storage (db)
    phases_reachability_hint: PhasesReachabilityHint, // phases_reachability_hint for slow path calculation
    memcache: MemcacheClient,                         // Memcache Client for temporary caching
    keygen: KeyGen,
}

impl CachingHintPhases {
    pub fn new(phases_store: Arc<Phases>, skip_index: Arc<SkiplistIndex>) -> Self {
        let key_prefix = "scm.mononoke.phases";
        Self {
            phases_store,
            phases_reachability_hint: PhasesReachabilityHint::new(skip_index),
            memcache: MemcacheClient::new(),
            keygen: KeyGen::new(key_prefix, MC_CODEVER, MC_SITEVER),
        }
    }
}

impl Phases for CachingHintPhases {
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
        cloned!(self.keygen, self.memcache, self.phases_store);
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
                                phases_store.add(ctx, repo, cs_id, phase)
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

    /// Add several new entries of phases to the underlying storage if they are not already the same
    /// in memcache. Update memcache if changes.
    fn add_all(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        phases: Vec<(ChangesetId, Phase)>,
    ) -> BoxFuture<(), Error> {
        cloned!(self.keygen, self.memcache, self.phases_store);
        let repo_id = repo.get_repoid();
        get_phases_from_memcache(
            &memcache,
            &keygen,
            &repo_id,
            phases.iter().map(|(cs_id, _)| cs_id.clone()).collect(),
        )
        .and_then(move |phases_mapping| {
            // Calculate the difference.

            // Some phases are missing or different in the memcache. They should be updated in memcache.
            let add_to_memcache: Vec<(ChangesetId, Phase)> = phases
                .into_iter()
                .filter_map(|(cs_id, phase)| {
                    if let Some(current_phase) = phases_mapping.calculated.get(&cs_id) {
                        if current_phase == &phase {
                            return None;
                        }
                    }
                    Some((cs_id, phase))
                })
                .collect();

            // Refresh the underlying persistent storage.
            // Same set of phases but for public commits only.
            let add_to_db: Vec<(ChangesetId, Phase)> = add_to_memcache
                .iter()
                .filter_map(|(cs_id, phase)| {
                    if phase == &Phase::Public {
                        Some((cs_id.clone(), phase.clone()))
                    } else {
                        None
                    }
                })
                .collect();

            set_phases_to_memcache(&memcache, &keygen, &repo_id, add_to_memcache)
                .and_then(move |_| phases_store.add_all(ctx, repo, add_to_db))
        })
        .boxify()
    }

    /// Retrieve the phase specified by this commit, if available.
    /// If phases are not available for some of the commits, they will be recalculated.
    /// If recalculation failed error will be returned.
    fn get(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_id: ChangesetId,
    ) -> BoxFuture<Option<Phase>, Error> {
        self.get_all(ctx, repo, vec![cs_id])
            .map(move |mut phases_mapping| phases_mapping.calculated.remove(&cs_id))
            .boxify()
    }

    /// Retrieve the phase specified by this commit, if available.
    /// If phases are not available for some of the commits, they will be recalculated.
    /// If recalculation failed error will be returned.
    fn get_all(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_ids: Vec<ChangesetId>,
    ) -> BoxFuture<PhasesMapping, Error> {
        cloned!(
            self.keygen,
            self.memcache,
            self.phases_store,
            self.phases_reachability_hint
        );
        let repo_id = repo.get_repoid();
        // Look up in the memcache.
        get_phases_from_memcache(&memcache, &keygen, &repo_id, cs_ids)
            .and_then(move |phases_mapping| {
                let found_in_memcache = phases_mapping.calculated;
                let not_found_in_memcache = phases_mapping.unknown;
                // Some phases are missing in memcache. Try to fetch from the underlying storage.
                phases_store
                    .get_all(ctx.clone(), repo.clone(), not_found_in_memcache)
                    .and_then(move |phases_mapping| {
                        // Some phases are missing in the underlying storage. Try to calculate it using phases_reachability_hint.
                        // Bookmarks are required (only if not_found_in_db is not empty). Fetch them once.
                        let found_in_db = phases_mapping.calculated;
                        let not_found_in_db = phases_mapping.unknown;
                        let bookmarks_fut = if not_found_in_db.is_empty() {
                            future::ok(vec![]).left_future()
                        } else {
                            repo.get_bonsai_bookmarks(ctx.clone())
                                .map(|(_, cs_id)| cs_id)
                                .collect()
                                .right_future()
                        };

                        bookmarks_fut
                            .and_then({
                                let changeset_fetcher = repo.get_changeset_fetcher().clone();
                                let ctx = ctx.clone();
                                move |bookmarks| {
                                    phases_reachability_hint.get_all(
                                        ctx,
                                        changeset_fetcher,
                                        not_found_in_db,
                                        Arc::new(HashSet::from_iter(bookmarks.into_iter())),
                                    )
                                }
                            })
                            .and_then(move |calculated| {
                                // These phases are calculated. Refresh the underlying storage (for public commits only).
                                let add_to_db: Vec<(ChangesetId, Phase)> = calculated
                                    .iter()
                                    .filter_map(|(cs_id, phase)| {
                                        if phase == &Phase::Public {
                                            Some((cs_id.clone(), phase.clone()))
                                        } else {
                                            None
                                        }
                                    })
                                    .collect();

                                // Add calculated and found_in_db phases in memcache
                                let add_to_memcache: Vec<(ChangesetId, Phase)> =
                                    calculated.into_iter().chain(found_in_db).collect();

                                // Chain all calculated, found in memcache and found in the db phases to the returned result
                                // This the same as add_to_memcache + found_in_memcache
                                let mut calculated = found_in_memcache;
                                calculated.extend(
                                    add_to_memcache
                                        .iter()
                                        .map(|(cs_id, phase)| (cs_id.clone(), phase.clone())),
                                );

                                phases_store
                                    .add_all(ctx, repo, add_to_db)
                                    .and_then(move |_| {
                                        set_phases_to_memcache(
                                            &memcache,
                                            &keygen,
                                            &repo_id,
                                            add_to_memcache,
                                        )
                                    })
                                    .map(move |_| PhasesMapping {
                                        calculated,
                                        unknown: vec![],
                                    })
                            })
                    })
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

// Memcache getter
// Memcache client doesn't have bulk api.
fn get_phases_from_memcache(
    memcache: &MemcacheClient,
    keygen: &KeyGen,
    repo_id: &RepositoryId,
    cs_ids: Vec<ChangesetId>,
) -> impl Future<Item = PhasesMapping, Error = Error> {
    stream::futures_unordered(cs_ids.into_iter().map(move |cs_id| {
        cloned!(memcache, keygen, repo_id);
        get_phase_from_memcache(&memcache, &keygen, &repo_id, &cs_id)
            .map(move |maybe_phase| (cs_id, maybe_phase))
    }))
    .collect()
    .map(|vec| {
        // split to unknown and calculated
        let (unknown, calculated): (Vec<_>, Vec<_>) = vec
            .into_iter()
            .partition(|(_, maybephase)| maybephase.is_none());

        PhasesMapping {
            calculated: calculated
                .into_iter()
                .filter_map(|(cs_id, somephase)| somephase.map(|phase| (cs_id, phase)))
                .collect(),

            unknown: unknown.into_iter().map(|(cs_id, _)| cs_id).collect(),
        }
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
    repo_id: &RepositoryId,
    phases: Vec<(ChangesetId, Phase)>,
) -> impl Future<Item = (), Error = Error> {
    cloned!(memcache, keygen, repo_id);
    stream::futures_unordered(
        phases.iter().map(|(cs_id, phase)| {
            set_phase_to_memcache(&memcache, &keygen, &repo_id, &cs_id, &phase)
        }),
    )
    .collect()
    .map(|_| ())
}
