/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::caching::{get_cache_key, phase_caching_determinator, Caches};
use anyhow::Error;
use bytes::Bytes;
use caching_ext::{GetOrFillMultipleFromCacheLayers, McErrorKind, McResult};
use context::CoreContext;
use futures_ext::FutureExt;
use futures_old::{future, Future};
use mononoke_types::{ChangesetId, RepositoryId};
use sql::{queries, Connection};
use stats::prelude::*;
use std::collections::HashSet;
use std::convert::TryInto;
use std::sync::Arc;

use crate::Phase;

define_stats! {
    prefix = "mononoke.phases";
    get_single: timeseries(Rate, Sum),
    get_many: timeseries(Rate, Sum),
    add_many: timeseries(Rate, Sum),
    memcache_hit: timeseries("memcache.hit"; Rate, Sum),
    memcache_miss: timeseries("memcache.miss"; Rate, Sum),
    memcache_internal_err: timeseries("memcache.internal_err"; Rate, Sum),
    memcache_deserialize_err: timeseries("memcache.deserialize_err"; Rate, Sum),
}

/// Object that reads/writes to phases db
#[derive(Clone)]
pub struct SqlPhasesStore {
    pub(crate) write_connection: Connection,
    pub(crate) read_connection: Connection,
    pub(crate) read_master_connection: Connection,
    pub(crate) caches: Arc<Caches>,
}

impl SqlPhasesStore {
    fn get_cacher(
        &self,
        repo_id: RepositoryId,
    ) -> GetOrFillMultipleFromCacheLayers<ChangesetId, Phase> {
        let report_mc_result = |res: McResult<()>| {
            match res {
                Ok(_) => STATS::memcache_hit.add_value(1),
                Err(McErrorKind::MemcacheInternal) => STATS::memcache_internal_err.add_value(1),
                Err(McErrorKind::Missing) => STATS::memcache_miss.add_value(1),
                Err(McErrorKind::Deserialization) => STATS::memcache_deserialize_err.add_value(1),
            };
        };

        let read_connection = self.read_connection.clone();
        let get_from_db = {
            move |cs_ids: HashSet<ChangesetId>| {
                let cs_ids: Vec<_> = cs_ids.into_iter().collect();
                SelectPhases::query(&read_connection, &repo_id, &cs_ids)
                    .map(move |public| public.into_iter().collect())
                    .boxify()
            }
        };

        let determinator = phase_caching_determinator;

        GetOrFillMultipleFromCacheLayers {
            repo_id,
            get_cache_key: Arc::new(get_cache_key),
            cachelib: self.caches.cache_pool.clone(),
            keygen: self.caches.keygen.clone(),
            memcache: self.caches.memcache.clone(),
            deserialize: Arc::new(|buf| buf.as_ref().try_into().map_err(|_| ())),
            serialize: Arc::new(|phase| Bytes::from(phase.to_string())),
            report_mc_result: Arc::new(report_mc_result),
            get_from_db: Arc::new(get_from_db),
            determinator,
        }
    }

    pub fn get_single_raw(
        &self,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> impl Future<Item = Option<Phase>, Error = Error> {
        STATS::get_single.add_value(1);
        let csids = vec![cs_id];

        let cacher = self.get_cacher(repo_id);
        cacher
            .run(csids.into_iter().collect())
            .map(|cs_to_phase| cs_to_phase.into_iter().next().map(|(_, phase)| phase))
    }

    pub fn get_public_raw(
        &self,
        repo_id: RepositoryId,
        csids: &[ChangesetId],
    ) -> impl Future<Item = HashSet<ChangesetId>, Error = Error> {
        if csids.is_empty() {
            return future::ok(Default::default()).left_future();
        }

        STATS::get_many.add_value(1);
        let cacher = self.get_cacher(repo_id);
        cacher
            .run(csids.iter().cloned().collect())
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
            .right_future()
    }

    pub fn add_public_raw(
        &self,
        _ctx: CoreContext,
        repoid: RepositoryId,
        csids: Vec<ChangesetId>,
    ) -> impl Future<Item = (), Error = Error> {
        if csids.is_empty() {
            return future::ok(()).left_future();
        }
        let phases: Vec<_> = csids
            .iter()
            .map(|csid| (&repoid, csid, &Phase::Public))
            .collect();
        STATS::add_many.add_value(1);
        let cacher = self.get_cacher(repoid);
        InsertPhase::query(&self.write_connection, &phases)
            .map(move |_| {
                cacher.fill_caches(csids.iter().map(|csid| (*csid, Phase::Public)).collect());
            })
            .right_future()
    }

    pub fn list_all_public(
        &self,
        _ctx: CoreContext,
        repo_id: RepositoryId,
    ) -> impl Future<Item = Vec<ChangesetId>, Error = Error> {
        SelectAllPublic::query(&self.read_connection, &repo_id)
            .map(|ans| ans.into_iter().map(|x| x.0).collect())
    }
}

queries! {
    write InsertPhase(values: (repo_id: RepositoryId, cs_id: ChangesetId, phase: Phase)) {
        none,
        mysql("INSERT INTO phases (repo_id, cs_id, phase) VALUES {values} ON DUPLICATE KEY UPDATE phase = VALUES(phase)")
        // sqlite query currently doesn't support changing the value
        // there is not usage for changing the phase at the moment
        // TODO (liubovd): improve sqlite query to make it semantically the same
        sqlite("INSERT OR IGNORE INTO phases (repo_id, cs_id, phase) VALUES {values}")
    }

    read SelectPhases(
        repo_id: RepositoryId,
        >list cs_ids: ChangesetId
    ) -> (ChangesetId, Phase) {
        "SELECT cs_id, phase
         FROM phases
         WHERE repo_id = {repo_id}
           AND cs_id IN {cs_ids}"
    }

    read SelectAllPublic(repo_id: RepositoryId) -> (ChangesetId, ) {
        "SELECT cs_id
         FROM phases
         WHERE repo_id = {repo_id}
           AND phase = 'Public'"
    }
}
