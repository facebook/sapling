// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use crate::errors::*;
use crate::{fill_unkown_phases, Phase, Phases, PhasesMapping};
use blobrepo::BlobRepo;
use cloned::cloned;
use context::CoreContext;
use futures::{future, stream, Future, Stream};
use futures_ext::{BoxFuture, FutureExt};
use memcache::{KeyGen, MemcacheClient};
use mononoke_types::{ChangesetId, RepositoryId};
use stats::{define_stats, Timeseries};
use std::collections::{HashMap, HashSet};
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

pub fn get_cache_key(repo_id: RepositoryId, cs_id: ChangesetId) -> String {
    format!("{}.{}", repo_id.prefix(), cs_id)
}

pub struct CachingHintPhases {
    phases_store: Arc<dyn Phases>, // phases_store is the underlying persistent storage (db)
    memcache: MemcacheClient,      // Memcache Client for temporary caching
    keygen: KeyGen,
}

impl CachingHintPhases {
    pub fn new(phases_store: Arc<dyn Phases>) -> Self {
        let key_prefix = "scm.mononoke.phases";
        Self {
            phases_store,
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
        get_phase_from_memcache(&memcache, &keygen, repo_id, cs_id)
            .and_then(move |maybe_phase| {
                match maybe_phase {
                    // The phase is already the same in memcache, nothing is needed.
                    Some(ref current_phase) if current_phase == &phase => {
                        future::ok(false).left_future()
                    }
                    _ => {
                        // The phase is missing or different in memcache. Refresh memcache.
                        set_phase_to_memcache(&memcache, &keygen, repo_id, cs_id, &phase)
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
            repo_id,
            phases.iter().map(|(cs_id, _)| cs_id.clone()).collect(),
        )
        .and_then(move |phases_mapping| {
            // Calculate the difference.

            // Some phases are missing or different in the memcache. They should be updated in memcache.
            let add_to_memcache: HashMap<_, _> = phases
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
            let add_to_db = add_to_memcache
                .iter()
                .filter_map(|(cs_id, phase)| {
                    if phase == &Phase::Public {
                        Some((cs_id.clone(), phase.clone()))
                    } else {
                        None
                    }
                })
                .collect();

            set_phases_to_memcache(&memcache, &keygen, repo_id, &add_to_memcache)
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
        self.get_all_with_bookmarks(ctx, repo, cs_ids, None)
    }

    /// Get phases for the list of commits.
    /// Accept optional bookmarks heads. Use this API if bookmarks are known.
    /// If phases are not available for some of the commits, they will be recalculated.
    /// If recalculation failed an error will be returned.
    /// Returns:
    /// phases_mapping::calculated           - phases hash map
    /// phases_mapping::unknown              - always empty
    /// phases_mapping::maybe_public_heads   - if bookmarks heads were fetched during calculation
    ///                                         or passed to this function they will be filled in.
    fn get_all_with_bookmarks(
        &self,
        ctx: CoreContext,
        repo: BlobRepo,
        cs_ids: Vec<ChangesetId>,
        maybe_public_heads: Option<Arc<HashSet<ChangesetId>>>,
    ) -> BoxFuture<PhasesMapping, Error> {
        cloned!(self.keygen, self.memcache, self.phases_store);
        let repo_id = repo.get_repoid();
        get_phases_from_memcache(&memcache, &keygen, repo_id, cs_ids)
            .and_then(move |phases_memcache| {
                fill_unkown_phases(
                    ctx,
                    repo,
                    phases_store,
                    maybe_public_heads,
                    phases_memcache.clone(),
                )
                .and_then(move |phases| {
                    let calculated = phases_memcache
                        .unknown
                        .into_iter()
                        .flat_map(|k| phases.calculated.get(&k).map(move |v| (k, *v)))
                        .collect();
                    set_phases_to_memcache(&memcache, &keygen, repo_id, &calculated)
                        .map(move |_| phases)
                })
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
) -> impl Future<Item = PhasesMapping, Error = Error> {
    stream::futures_unordered(cs_ids.into_iter().map(move |cs_id| {
        cloned!(memcache, keygen, repo_id);
        get_phase_from_memcache(&memcache, &keygen, repo_id, cs_id)
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

            ..Default::default()
        }
    })
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
    phases: &HashMap<ChangesetId, Phase>,
) -> impl Future<Item = (), Error = Error> {
    cloned!(memcache, keygen, repo_id);
    stream::futures_unordered(
        phases.iter().map(|(cs_id, phase)| {
            set_phase_to_memcache(&memcache, &keygen, repo_id, *cs_id, &phase)
        }),
    )
    .collect()
    .map(|_| ())
}
