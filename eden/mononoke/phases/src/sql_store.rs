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
use cloned::cloned;
use context::{CoreContext, PerfCounterType};
use futures::compat::Future01CompatExt;
use futures_ext::FutureExt as OldFutureExt;
use futures_old::Future as OldFuture;
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
    list_all: timeseries(Rate, Sum),
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
        ctx: &CoreContext,
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

        cloned!(ctx, self.read_connection);
        let get_from_db = {
            move |cs_ids: HashSet<ChangesetId>| {
                let cs_ids: Vec<_> = cs_ids.into_iter().collect();
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsReplica);
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

    pub async fn get_single_raw(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> Result<Option<Phase>, Error> {
        STATS::get_single.add_value(1);
        let csids = vec![cs_id];

        let cacher = self.get_cacher(ctx, repo_id);
        let cs_to_phase = cacher.run(csids.into_iter().collect()).compat().await?;

        Ok(cs_to_phase.into_iter().next().map(|(_, phase)| phase))
    }

    pub async fn get_public_raw(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        csids: &[ChangesetId],
    ) -> Result<HashSet<ChangesetId>, Error> {
        if csids.is_empty() {
            return Ok(Default::default());
        }

        STATS::get_many.add_value(1);
        let cacher = self.get_cacher(ctx, repo_id);
        let csids = csids.iter().cloned().collect();
        let cs_to_phase = cacher.run(csids).compat().await?;

        Ok(cs_to_phase
            .into_iter()
            .filter_map(|(key, value)| {
                if value == Phase::Public {
                    Some(key)
                } else {
                    None
                }
            })
            .collect())
    }

    pub async fn add_public_raw(
        &self,
        ctx: &CoreContext,
        repoid: RepositoryId,
        csids: Vec<ChangesetId>,
    ) -> Result<(), Error> {
        if csids.is_empty() {
            return Ok(());
        }
        STATS::add_many.add_value(1);
        let cacher = self.get_cacher(ctx, repoid);
        let phases: Vec<_> = csids
            .iter()
            .map(|csid| (&repoid, csid, &Phase::Public))
            .collect();

        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        InsertPhase::query(&self.write_connection, &phases)
            .compat()
            .await?;

        cacher.fill_caches(csids.iter().map(|csid| (*csid, Phase::Public)).collect());
        Ok(())
    }

    pub async fn list_all_public(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
    ) -> Result<Vec<ChangesetId>, Error> {
        STATS::list_all.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let ans = SelectAllPublic::query(&self.read_connection, &repo_id)
            .compat()
            .await?;
        Ok(ans.into_iter().map(|x| x.0).collect())
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
