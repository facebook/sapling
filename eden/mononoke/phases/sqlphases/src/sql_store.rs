/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context as _;
use anyhow::Error;
use async_trait::async_trait;
use bytes::Bytes;
use caching_ext::fill_cache;
use caching_ext::get_or_fill;
use caching_ext::CacheDisposition;
use caching_ext::CacheTtl;
use caching_ext::CachelibHandler;
use caching_ext::EntityStore;
use caching_ext::KeyedEntityStore;
use caching_ext::MemcacheEntity;
use caching_ext::MemcacheHandler;
use context::CoreContext;
use context::PerfCounterType;
use maplit::hashset;
use memcache::KeyGen;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use phases::Phase;
use sql::queries;
use sql::Connection;
use stats::prelude::*;
use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use crate::sql_phases::SqlPhase;

// 6 hours in sec
pub const TTL_DRAFT_SEC: u64 = 21600;

define_stats! {
    prefix = "mononoke.phases";
    get_single: timeseries(Rate, Sum),
    get_many: timeseries(Rate, Sum),
    add_many: timeseries(Rate, Sum),
    list_all: timeseries(Rate, Sum),
}

pub struct Caches {
    pub memcache: MemcacheHandler, // Memcache Client for temporary caching
    pub cache_pool: CachelibHandler<SqlPhase>,
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

/// Object that reads/writes to phases db
#[derive(Clone)]
pub struct SqlPhasesStore {
    pub(crate) write_connection: Connection,
    pub(crate) read_connection: Connection,
    #[allow(dead_code)]
    pub(crate) read_master_connection: Connection,
    pub(crate) caches: Arc<Caches>,
}

impl SqlPhasesStore {
    pub async fn get_single_raw(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> Result<Option<Phase>, Error> {
        STATS::get_single.add_value(1);

        let ctx = (ctx, repo_id, self);

        let res = get_or_fill(ctx, hashset! { cs_id })
            .await
            .with_context(|| "Error fetching phases via cache")?
            .into_iter()
            .map(|(_, val)| val.into())
            .next();

        Ok(res)
    }

    pub async fn get_public_raw(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        csids: impl IntoIterator<Item = &ChangesetId>,
    ) -> Result<HashSet<ChangesetId>, Error> {
        let ids = csids.into_iter().cloned().collect::<HashSet<ChangesetId>>();
        if ids.is_empty() {
            return Ok(Default::default());
        }
        STATS::get_many.add_value(1);
        let ctx = (ctx, repo_id, self);
        let cs_to_phase = get_or_fill(ctx, ids)
            .await
            .with_context(|| "Error fetching phases via cache")?;

        Ok(cs_to_phase
            .into_iter()
            .filter_map(|(key, value)| {
                if value == SqlPhase(Phase::Public) {
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
        let phases: Vec<_> = csids
            .iter()
            .map(|csid| (&repoid, csid, &SqlPhase(Phase::Public)))
            .collect();

        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        InsertPhase::query(&self.write_connection, &phases).await?;

        {
            let ctx = (ctx, repoid, self);
            let phases = csids
                .iter()
                .map(|csid| (csid, &SqlPhase(Phase::Public)))
                .collect::<Vec<_>>();
            fill_cache(ctx, phases).await;
        }

        Ok(())
    }

    pub async fn list_all_public(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
    ) -> Result<Vec<ChangesetId>, Error> {
        STATS::list_all.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let ans = SelectAllPublic::query(&self.read_connection, &repo_id).await?;
        Ok(ans.into_iter().map(|x| x.0).collect())
    }
}

impl MemcacheEntity for SqlPhase {
    fn serialize(&self) -> Bytes {
        Bytes::from(self.to_string())
    }

    fn deserialize(bytes: Bytes) -> Result<Self, ()> {
        bytes.as_ref().try_into().map_err(|_| ())
    }
}

type CacheRequest<'a> = (&'a CoreContext, RepositoryId, &'a SqlPhasesStore);

impl EntityStore<SqlPhase> for CacheRequest<'_> {
    fn cachelib(&self) -> &CachelibHandler<SqlPhase> {
        let (_, _, phases) = self;
        &phases.caches.cache_pool
    }

    fn keygen(&self) -> &KeyGen {
        let (_, _, phases) = self;
        &phases.caches.keygen
    }

    fn memcache(&self) -> &MemcacheHandler {
        let (_, _, phases) = self;
        &phases.caches.memcache
    }

    fn cache_determinator(&self, phase: &SqlPhase) -> CacheDisposition {
        let ttl = if phase == &SqlPhase(Phase::Public) {
            CacheTtl::NoTtl
        } else {
            CacheTtl::Ttl(Duration::from_secs(TTL_DRAFT_SEC))
        };

        CacheDisposition::Cache(ttl)
    }

    caching_ext::impl_singleton_stats!("phases");
}

#[async_trait]
impl KeyedEntityStore<ChangesetId, SqlPhase> for CacheRequest<'_> {
    fn get_cache_key(&self, cs_id: &ChangesetId) -> String {
        let (_, repo_id, _) = self;
        get_cache_key(*repo_id, cs_id)
    }

    async fn get_from_db(
        &self,
        cs_ids: HashSet<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, SqlPhase>, Error> {
        let (ctx, repo_id, mapping) = self;

        let cs_ids: Vec<_> = cs_ids.into_iter().collect();
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        // NOTE: We only track public phases in the DB.
        let public = SelectPhases::query(&mapping.read_connection, repo_id, &cs_ids).await?;

        Result::<_, Error>::Ok(public.into_iter().collect())
    }
}

pub fn get_cache_key(repo_id: RepositoryId, cs_id: &ChangesetId) -> String {
    format!("{}.{}", repo_id.prefix(), cs_id)
}

queries! {
    write InsertPhase(values: (repo_id: RepositoryId, cs_id: ChangesetId, phase: SqlPhase)) {
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
    ) -> (ChangesetId, SqlPhase) {
        mysql(
            "SELECT cs_id, phase
            FROM phases FORCE INDEX(PRIMARY)
            WHERE repo_id = {repo_id}
              AND cs_id IN {cs_ids}"
        )
        sqlite(
            "SELECT cs_id, phase
            FROM phases
            WHERE repo_id = {repo_id}
              AND cs_id IN {cs_ids}"
        )
    }

    read SelectAllPublic(repo_id: RepositoryId) -> (ChangesetId, ) {
        "SELECT cs_id
         FROM phases
         WHERE repo_id = {repo_id}
           AND phase = 'Public'"
    }
}
