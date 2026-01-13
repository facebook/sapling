/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Error;
use anyhow::Result;
use anyhow::ensure;
use async_trait::async_trait;
use context::CoreContext;
use context::PerfCounterType;
use futures::future;
use futures_stats::TimedTryFutureExt;
use mercurial_types::HgChangesetId;
use mercurial_types::HgChangesetIdPrefix;
use mercurial_types::HgChangesetIdsResolvedFromPrefix;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use rendezvous::ConfigurableRendezVousController;
use rendezvous::RendezVous;
use rendezvous::RendezVousOptions;
use rendezvous::RendezVousStats;
use scuba_ext::FutureStatsScubaExt;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::Connection;
use sql_ext::ConsistentReadOptions;
use sql_ext::SqlConnections;
use sql_ext::SqlQueryTelemetry;
use sql_ext::consistent_read_options;
use sql_ext::mononoke_queries;
use stats::prelude::*;
use time_ext::DurationExt;

mod caching;
mod errors;
mod mem_writes_bonsai_hg_mapping;
use futures::FutureExt;

pub use crate::caching::CachingBonsaiHgMapping;
pub use crate::errors::ErrorKind;
pub use crate::mem_writes_bonsai_hg_mapping::MemWritesBonsaiHgMapping;

define_stats! {
    prefix = "mononoke.bonsai_hg_mapping";
    gets: timeseries(Rate, Sum),
    gets_master: timeseries(Rate, Sum),
    adds: timeseries(Rate, Sum),
    get_many_hg_by_prefix: timeseries(Rate, Sum),
    // Number of mappings that were not found in the replica
    left_to_fetch: timeseries(Sum, Average, Count),
    // Number of mappings that were fetched from the master
    fetched_from_master: timeseries(Sum, Average, Count),
    // Duration of fetches
    get_duration_us: timeseries(Average, Count),
    // Duration of fetches using consistent read queries
    cons_read_get_duration_us: timeseries(Average, Count),
}

#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct BonsaiHgMappingEntry {
    pub hg_cs_id: HgChangesetId,
    pub bcs_id: ChangesetId,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub enum BonsaiOrHgChangesetIds {
    Bonsai(Vec<ChangesetId>),
    Hg(Vec<HgChangesetId>),
}

impl BonsaiOrHgChangesetIds {
    pub fn is_empty(&self) -> bool {
        match self {
            BonsaiOrHgChangesetIds::Bonsai(v) => v.is_empty(),
            BonsaiOrHgChangesetIds::Hg(v) => v.is_empty(),
        }
    }

    pub fn count(&self) -> usize {
        match self {
            BonsaiOrHgChangesetIds::Bonsai(v) => v.len(),
            BonsaiOrHgChangesetIds::Hg(v) => v.len(),
        }
    }
}

impl From<ChangesetId> for BonsaiOrHgChangesetIds {
    fn from(cs_id: ChangesetId) -> Self {
        BonsaiOrHgChangesetIds::Bonsai(vec![cs_id])
    }
}

impl From<Vec<ChangesetId>> for BonsaiOrHgChangesetIds {
    fn from(cs_ids: Vec<ChangesetId>) -> Self {
        BonsaiOrHgChangesetIds::Bonsai(cs_ids)
    }
}

impl From<HgChangesetId> for BonsaiOrHgChangesetIds {
    fn from(cs_id: HgChangesetId) -> Self {
        BonsaiOrHgChangesetIds::Hg(vec![cs_id])
    }
}

impl From<Vec<HgChangesetId>> for BonsaiOrHgChangesetIds {
    fn from(cs_ids: Vec<HgChangesetId>) -> Self {
        BonsaiOrHgChangesetIds::Hg(cs_ids)
    }
}

#[facet::facet]
#[async_trait]
pub trait BonsaiHgMapping: Send + Sync {
    fn repo_id(&self) -> RepositoryId;

    async fn add(&self, ctx: &CoreContext, entry: BonsaiHgMappingEntry) -> Result<bool, Error>;

    async fn get(
        &self,
        ctx: &CoreContext,
        cs_id: BonsaiOrHgChangesetIds,
    ) -> Result<Vec<BonsaiHgMappingEntry>, Error>;

    async fn get_hg_from_bonsai(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<HgChangesetId>, Error> {
        let result = self.get(ctx, cs_id.into()).await?;
        let hg_cs_id = result.into_iter().next().map(|entry| entry.hg_cs_id);
        Ok(hg_cs_id)
    }

    async fn get_bonsai_from_hg(
        &self,
        ctx: &CoreContext,
        cs_id: HgChangesetId,
    ) -> Result<Option<ChangesetId>, Error> {
        let result = self.get(ctx, cs_id.into()).await?;
        let bcs_id = result.into_iter().next().map(|entry| entry.bcs_id);
        Ok(bcs_id)
    }

    async fn get_many_hg_by_prefix(
        &self,
        ctx: &CoreContext,
        cs_prefix: HgChangesetIdPrefix,
        limit: usize,
    ) -> Result<HgChangesetIdsResolvedFromPrefix, Error> {
        let mut fetched_cs = self
            .get_hg_in_range(ctx, cs_prefix.min_cs(), cs_prefix.max_cs(), limit + 1)
            .await?;
        let res = match fetched_cs.len() {
            0 => HgChangesetIdsResolvedFromPrefix::NoMatch,
            1 => HgChangesetIdsResolvedFromPrefix::Single(fetched_cs[0].clone()),
            l if l <= limit => HgChangesetIdsResolvedFromPrefix::Multiple(fetched_cs),
            _ => HgChangesetIdsResolvedFromPrefix::TooMany({
                fetched_cs.pop();
                fetched_cs
            }),
        };
        Ok(res)
    }

    async fn get_hg_in_range(
        &self,
        ctx: &CoreContext,
        low: HgChangesetId,
        high: HgChangesetId,
        limit: usize,
    ) -> Result<Vec<HgChangesetId>, Error>;

    /// Convert a set of hg changeset ids to bonsai changesets.  If a changeset doesn't exist, it is omitted from the result.
    /// Order of returned ids is random.
    async fn convert_available_hg_to_bonsai(
        &self,
        ctx: &CoreContext,
        hg_cs_ids: Vec<HgChangesetId>,
    ) -> Result<Vec<ChangesetId>, Error> {
        let mapping = self.get(ctx, hg_cs_ids.into()).await?;
        Ok(mapping.into_iter().map(|entry| entry.bcs_id).collect())
    }

    /// Convert a set of hg changeset ids to bonsai changesets.  If a changeset doesn't exist, this is an error.
    /// Order of returned ids is random.
    async fn convert_all_hg_to_bonsai(
        &self,
        ctx: &CoreContext,
        hg_cs_ids: Vec<HgChangesetId>,
    ) -> Result<Vec<ChangesetId>, Error> {
        let mapping = self.get(ctx, hg_cs_ids.clone().into()).await?;
        if mapping.len() != hg_cs_ids.len() {
            let mut result = Vec::with_capacity(mapping.len());
            let mut missing = hg_cs_ids.into_iter().collect::<HashSet<_>>();
            for entry in mapping {
                missing.remove(&entry.hg_cs_id);
                result.push(entry.bcs_id);
            }
            ensure!(
                missing.is_empty(),
                "Missing bonsai mapping for hg changesets: {:?}",
                missing,
            );
            Ok(result)
        } else {
            Ok(mapping.into_iter().map(|entry| entry.bcs_id).collect())
        }
    }

    /// Convert a set of bonsai changeset ids to hg changesets.  If a changeset doesn't exist, it is omitted from the result.
    async fn convert_available_bonsai_to_hg(
        &self,
        ctx: &CoreContext,
        bcs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<HgChangesetId>, Error> {
        let mapping = self.get(ctx, bcs_ids.into()).await?;
        Ok(mapping.into_iter().map(|entry| entry.hg_cs_id).collect())
    }

    /// Convert a set of bonsai changeset ids to hg changesets.  If a changeset doesn't exist, this is an error.
    async fn convert_all_bonsai_to_hg(
        &self,
        ctx: &CoreContext,
        bcs_ids: Vec<ChangesetId>,
    ) -> Result<Vec<HgChangesetId>, Error> {
        let mapping = self.get(ctx, bcs_ids.clone().into()).await?;
        if mapping.len() != bcs_ids.len() {
            let mut result = Vec::with_capacity(mapping.len());
            let mut missing = bcs_ids.into_iter().collect::<HashSet<_>>();
            for entry in mapping {
                missing.remove(&entry.bcs_id);
                result.push(entry.hg_cs_id);
            }
            ensure!(
                missing.is_empty(),
                "Missing hg mapping for bonsai changesets: {:?}",
                missing,
            );
            Ok(result)
        } else {
            Ok(mapping.into_iter().map(|entry| entry.hg_cs_id).collect())
        }
    }

    /// Get a hashmap that maps from given bonsai changesets to their hg equivalent.
    async fn get_bonsai_to_hg_map(
        &self,
        ctx: &CoreContext,
        bcs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, HgChangesetId>, Error> {
        let mapping = self.get(ctx, bcs_ids.into()).await?;
        Ok(mapping
            .into_iter()
            .map(|entry| (entry.bcs_id, entry.hg_cs_id))
            .collect())
    }
}

#[derive(Clone)]
struct RendezVousConnection {
    bonsai: RendezVous<ChangesetId, HgChangesetId>,
    hg: RendezVous<HgChangesetId, ChangesetId>,
    conn: Connection,
}

impl RendezVousConnection {
    fn new(conn: Connection, name: &str, opts: RendezVousOptions) -> Self {
        Self {
            conn,
            bonsai: RendezVous::new(
                ConfigurableRendezVousController::new(opts),
                Arc::new(RendezVousStats::new(format!(
                    "bonsai_hg_mapping.bonsai.{}",
                    name,
                ))),
            ),
            hg: RendezVous::new(
                ConfigurableRendezVousController::new(opts),
                Arc::new(RendezVousStats::new(format!(
                    "bonsai_hg_mapping.hg.{}",
                    name,
                ))),
            ),
        }
    }
}

pub struct SqlBonsaiHgMapping {
    write_connection: Connection,
    read_connection: RendezVousConnection,
    read_master_connection: RendezVousConnection,
    repo_id: RepositoryId,
    // Option that forces all `add()` method calls to overwrite values
    // that set in the database. This should be used only when we try to
    // fix broken entries in the db.
    overwrite: bool,
}

mononoke_queries! {
    // Sett almost identical ReplaceMapping below
    write InsertMapping(values: (
        repo_id: RepositoryId,
        hg_cs_id: HgChangesetId,
        bcs_id: ChangesetId,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bonsai_hg_mapping (repo_id, hg_cs_id, bcs_id) VALUES {values}"
    }

    // Sett almost identical InsertMapping above
    write ReplaceMapping(values: (
        repo_id: RepositoryId,
        hg_cs_id: HgChangesetId,
        bcs_id: ChangesetId,
    )) {
        none,
        "REPLACE INTO bonsai_hg_mapping (repo_id, hg_cs_id, bcs_id) VALUES {values}"
    }

    read SelectMappingByBonsai(
        repo_id: RepositoryId,
        >list bcs_id: ChangesetId
    ) -> (HgChangesetId, ChangesetId) {
        "SELECT hg_cs_id, bcs_id
         FROM bonsai_hg_mapping
         WHERE repo_id = {repo_id}
           AND bcs_id IN {bcs_id}"
    }

    read SelectMappingByHg(
        repo_id: RepositoryId,
        >list hg_cs_id: HgChangesetId
    ) -> (HgChangesetId, ChangesetId) {
        "SELECT hg_cs_id, bcs_id
         FROM bonsai_hg_mapping
         WHERE repo_id = {repo_id}
           AND hg_cs_id IN {hg_cs_id}"
    }

    read SelectHgChangesetsByRange(repo_id: RepositoryId, hg_cs_min: &[u8], hg_cs_max: &[u8], limit: usize) -> (HgChangesetId) {
        "SELECT hg_cs_id
         FROM bonsai_hg_mapping
         WHERE repo_id = {repo_id}
           AND hg_cs_id >= {hg_cs_min} AND hg_cs_id <= {hg_cs_max}
           LIMIT {limit}
        "
    }
}

#[derive(Clone)]
pub struct SqlBonsaiHgMappingBuilder {
    connections: SqlConnections,
    overwrite: bool,
}

impl SqlConstruct for SqlBonsaiHgMappingBuilder {
    const LABEL: &'static str = "bonsai_hg_mapping";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-bonsai-hg-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            connections,
            overwrite: false,
        }
    }
}

impl SqlBonsaiHgMappingBuilder {
    pub fn with_overwrite(mut self) -> Self {
        self.overwrite = true;
        self
    }

    pub fn build(self, repo_id: RepositoryId, opts: RendezVousOptions) -> SqlBonsaiHgMapping {
        let SqlBonsaiHgMappingBuilder {
            connections,
            overwrite,
        } = self;

        SqlBonsaiHgMapping {
            write_connection: connections.write_connection,
            read_connection: RendezVousConnection::new(connections.read_connection, "reader", opts),
            read_master_connection: RendezVousConnection::new(
                connections.read_master_connection,
                "read_master",
                opts,
            ),
            repo_id,
            overwrite,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlBonsaiHgMappingBuilder {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        Some(&remote.bonsai_hg_mapping)
    }
    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        Some(&remote.bonsai_hg_mapping)
    }
}

impl SqlBonsaiHgMapping {
    async fn verify_consistency(
        &self,
        ctx: &CoreContext,
        entry: BonsaiHgMappingEntry,
    ) -> Result<(), Error> {
        let BonsaiHgMappingEntry { hg_cs_id, bcs_id } = entry.clone();

        let hg_ids = &[hg_cs_id];
        let by_hg = SelectMappingByHg::query(
            &self.read_master_connection.conn,
            ctx.sql_query_telemetry(),
            &self.repo_id,
            hg_ids,
        );
        let bcs_ids = &[bcs_id];

        let by_bcs = SelectMappingByBonsai::query(
            &self.read_master_connection.conn,
            ctx.sql_query_telemetry(),
            &self.repo_id,
            bcs_ids,
        )
        .boxed();

        let (by_hg_rows, by_bcs_rows) = future::try_join(by_hg, by_bcs).await?;

        match by_hg_rows.into_iter().chain(by_bcs_rows.into_iter()).next() {
            Some(entry) if entry == (hg_cs_id, bcs_id) => Ok(()),
            Some((hg_cs_id, bcs_id)) => Err(ErrorKind::ConflictingEntries(
                BonsaiHgMappingEntry { hg_cs_id, bcs_id },
                entry,
            )
            .into()),
            None => Err(ErrorKind::RaceConditionWithDelete(entry).into()),
        }
    }
}

#[async_trait]
impl BonsaiHgMapping for SqlBonsaiHgMapping {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add(&self, ctx: &CoreContext, entry: BonsaiHgMappingEntry) -> Result<bool, Error> {
        STATS::adds.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        let BonsaiHgMappingEntry { hg_cs_id, bcs_id } = entry.clone();
        if self.overwrite {
            let result = ReplaceMapping::query(
                &self.write_connection,
                ctx.sql_query_telemetry(),
                &[(&self.repo_id, &hg_cs_id, &bcs_id)],
            )
            .await?;
            Ok(result.affected_rows() >= 1)
        } else {
            let result = InsertMapping::query(
                &self.write_connection,
                ctx.sql_query_telemetry(),
                &[(&self.repo_id, &hg_cs_id, &bcs_id)],
            )
            .await?;
            if result.affected_rows() == 1 {
                Ok(true)
            } else {
                self.verify_consistency(ctx, entry).await?;
                Ok(false)
            }
        }
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        ids: BonsaiOrHgChangesetIds,
    ) -> Result<Vec<BonsaiHgMappingEntry>> {
        let cons_read_opts =
            consistent_read_options(ctx.client_correlator(), Some("bonsai_hg_mapping"));

        let used_consistent_reads = cons_read_opts.is_some();
        let timed_res = async move {
            STATS::gets.add_value(1);
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsReplica);

            let (mut mappings, left_to_fetch) = select_mapping(
                ctx,
                &self.read_connection,
                &self.read_master_connection,
                self.repo_id,
                ids,
                cons_read_opts,
            )
            .await?;

            let left_to_fetch_count = left_to_fetch.count().try_into().map_err(Error::from)?;
            STATS::left_to_fetch.add_value(left_to_fetch_count);

            if left_to_fetch.is_empty() || used_consistent_reads {
                // If consistent reads were used, the replica that served the request
                // was up-to-date, so any mappings left to fetch don't exist.
                return anyhow::Ok::<Vec<BonsaiHgMappingEntry>>(mappings);
            }

            STATS::gets_master.add_value(1);
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            let (mut master_mappings, _) = select_mapping(
                ctx,
                &self.read_master_connection,
                &self.read_master_connection,
                self.repo_id,
                left_to_fetch,
                None,
            )
            .await?;

            let fetched_from_master_count =
                master_mappings.len().try_into().map_err(Error::from)?;
            STATS::fetched_from_master.add_value(fetched_from_master_count);

            mappings.append(&mut master_mappings);
            Ok(mappings)
        }
        .try_timed()
        .await?;

        if let Ok(completion_time_us) = timed_res.0.completion_time.as_micros_unchecked().try_into()
        {
            if used_consistent_reads {
                STATS::cons_read_get_duration_us.add_value(completion_time_us);
            } else {
                STATS::get_duration_us.add_value(completion_time_us);
            };
        };

        let res = timed_res.log_future_stats(ctx.scuba().clone(), "Get BonsaiHgMapping", None);

        Ok(res)
    }

    /// Return [`HgChangesetId`] entries in the inclusive range described by `low` and `high`.
    /// Maximum `limit` entries will be returned.
    async fn get_hg_in_range(
        &self,
        ctx: &CoreContext,
        low: HgChangesetId,
        high: HgChangesetId,
        limit: usize,
    ) -> Result<Vec<HgChangesetId>, Error> {
        if low > high {
            return Ok(Vec::new());
        }
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let rows = SelectHgChangesetsByRange::query(
            &self.read_connection.conn,
            ctx.sql_query_telemetry(),
            &self.repo_id,
            &low.as_bytes(),
            &high.as_bytes(),
            &limit,
        )
        .await?;
        let mut fetched: Vec<HgChangesetId> = rows.into_iter().map(|row| row.0).collect();
        if fetched.is_empty() {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            let rows = SelectHgChangesetsByRange::query(
                &self.read_master_connection.conn,
                ctx.sql_query_telemetry(),
                &self.repo_id,
                &low.as_bytes(),
                &high.as_bytes(),
                &limit,
            )
            .await?;
            fetched = rows.into_iter().map(|row| row.0).collect();
        }
        Ok(fetched)
    }
}

async fn select_mapping(
    ctx: &CoreContext,
    connection: &RendezVousConnection,
    read_master_connection: &RendezVousConnection,
    repo_id: RepositoryId,
    cs_ids: BonsaiOrHgChangesetIds,
    cons_read_opts: Option<ConsistentReadOptions>,
) -> Result<(Vec<BonsaiHgMappingEntry>, BonsaiOrHgChangesetIds), Error> {
    if cs_ids.is_empty() {
        return Ok((vec![], cs_ids));
    }
    let sql_query_tel: SqlQueryTelemetry = ctx.sql_query_telemetry();

    let num_ids_requested = cs_ids.count();

    let (found, missing): (Vec<_>, _) = match cs_ids {
        BonsaiOrHgChangesetIds::Bonsai(bcs_ids) => {
            let ret = connection
                .bonsai
                .dispatch(ctx.fb, bcs_ids.into_iter().collect(), || {
                    let conn = connection.conn.clone();
                    let read_master_conn = read_master_connection.conn.clone();

                    move |bcs_ids| async move {
                        let bcs_ids = bcs_ids.into_iter().collect::<Vec<_>>();

                        let res = if let Some(cons_read_opts) = cons_read_opts {
                            let sql_connections = SqlConnections {
                                // The write connections won't be used, but pass the read-only
                                // connection to ensure that it can't be used by accident in the future.
                                write_connection: conn.clone(),
                                read_connection: conn,
                                read_master_connection: read_master_conn,
                            };

                            let return_early_if: Arc<
                                Box<
                                    dyn for<'a> Fn(&'a Vec<(HgChangesetId, ChangesetId)>) -> bool
                                        + Send
                                        + Sync,
                                >,
                            > = Arc::new(Box::new(move |query_res| {
                                query_res.len() == num_ids_requested
                            }));

                            SelectMappingByBonsai::query_with_consistency(
                                &sql_connections,
                                sql_query_tel.clone(),
                                Some(Timestamp::now()),
                                Some(return_early_if),
                                cons_read_opts,
                                &repo_id,
                                &bcs_ids[..],
                            )
                            .await?
                        } else {
                            SelectMappingByBonsai::query(
                                &conn,
                                sql_query_tel.clone(),
                                &repo_id,
                                &bcs_ids[..],
                            )
                            .await?
                        };

                        Ok(res
                            .into_iter()
                            .map(|(hg_cs_id, bcs_id)| (bcs_id, hg_cs_id))
                            .collect())
                    }
                })
                .await?;

            let mut not_found = vec![];
            let found = ret
                .into_iter()
                .filter_map(|(bcs_id, hg_cs_id)| match hg_cs_id {
                    Some(hg_cs_id) => Some((hg_cs_id, bcs_id)),
                    None => {
                        not_found.push(bcs_id);
                        None
                    }
                })
                .collect();

            (found, BonsaiOrHgChangesetIds::Bonsai(not_found))
        }
        BonsaiOrHgChangesetIds::Hg(hg_cs_ids) => {
            let ret = connection
                .hg
                .dispatch(ctx.fb, hg_cs_ids.into_iter().collect(), || {
                    let conn = connection.conn.clone();
                    let read_master_conn = read_master_connection.conn.clone();
                    move |hg_cs_ids| async move {
                        let hg_cs_ids = hg_cs_ids.into_iter().collect::<Vec<_>>();

                        let res = if let Some(cons_read_opts) = cons_read_opts {
                            let sql_connections = SqlConnections {
                                // The write connections won't be used, but pass the read-only
                                // connection to ensure that it can't be used by accident in the future.
                                write_connection: conn.clone(),
                                read_connection: conn,
                                read_master_connection: read_master_conn,
                            };

                            let return_early_if: Arc<
                                Box<
                                    dyn for<'a> Fn(&'a Vec<(HgChangesetId, ChangesetId)>) -> bool
                                        + Send
                                        + Sync,
                                >,
                            > = Arc::new(Box::new(move |query_res| {
                                query_res.len() == num_ids_requested
                            }));

                            SelectMappingByHg::query_with_consistency(
                                &sql_connections,
                                sql_query_tel.clone(),
                                Some(Timestamp::now()),
                                Some(return_early_if),
                                cons_read_opts,
                                &repo_id,
                                &hg_cs_ids[..],
                            )
                            .await?
                        } else {
                            SelectMappingByHg::query(
                                &conn,
                                sql_query_tel.clone(),
                                &repo_id,
                                &hg_cs_ids[..],
                            )
                            .await?
                        };

                        Ok(res.into_iter().collect())
                    }
                })
                .await?;

            let mut not_found = vec![];
            let found = ret
                .into_iter()
                .filter_map(|(hg_cs_id, bcs_id)| match bcs_id {
                    Some(bcs_id) => Some((hg_cs_id, bcs_id)),
                    None => {
                        not_found.push(hg_cs_id);
                        None
                    }
                })
                .collect();

            (found, BonsaiOrHgChangesetIds::Hg(not_found))
        }
    };

    Ok((
        found
            .into_iter()
            .map(move |(hg_cs_id, bcs_id)| BonsaiHgMappingEntry { hg_cs_id, bcs_id })
            .collect(),
        missing,
    ))
}
