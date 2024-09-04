/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Error;
use async_trait::async_trait;
use context::CoreContext;
use context::PerfCounterType;
use dashmap::DashMap;
use itertools::Itertools;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use rendezvous::ConfigurableRendezVousController;
use rendezvous::RendezVous;
use rendezvous::RendezVousOptions;
use rendezvous::RendezVousStats;
use sql::Connection;
use sql::Transaction;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::mononoke_queries;
use sql_ext::SqlConnections;
use stats::prelude::*;

use crate::EquivalentWorkingCopyEntry;
use crate::ErrorKind;
use crate::FetchedMappingEntry;
use crate::SyncedCommitMapping;
use crate::SyncedCommitMappingEntry;
use crate::SyncedCommitSourceRepo;
use crate::WorkingCopyEquivalence;

define_stats! {
    prefix = "mononoke.synced_commit_mapping";
    gets: timeseries(Rate, Sum),
    gets_master: timeseries(Rate, Sum),
    adds: timeseries(Rate, Sum),
    add_many_in_txn: timeseries(Rate, Sum),
    add_bulks: timeseries(Rate, Sum),
    insert_working_copy_eqivalence: timeseries(Rate, Sum),
    get_equivalent_working_copy: timeseries(Rate, Sum),
}

#[derive(Clone)]
pub struct SqlSyncedCommitMappingBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlSyncedCommitMappingBuilder {
    const LABEL: &'static str = "synced_commit_mapping";

    const CREATION_QUERY: &'static str =
        include_str!("../schemas/sqlite-synced-commit-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlSyncedCommitMappingBuilder {}

impl SqlSyncedCommitMappingBuilder {
    pub fn build(self, opts: RendezVousOptions) -> SqlSyncedCommitMapping {
        SqlSyncedCommitMapping {
            write_connection: self.connections.write_connection,
            read_connection: RendezVousConnection::new(
                self.connections.read_connection,
                "read",
                opts,
            ),
            read_master_connection: RendezVousConnection::new(
                self.connections.read_master_connection,
                "read_master",
                opts,
            ),
        }
    }
}

#[derive(Clone)]
struct RendezVousConnection {
    // For fetching synced commit mappings, we create a separate rendezvous instace per (source_repo_id, target_repo_id) pair
    fetch_synced_commit_mappings:
        DashMap<(RepositoryId, RepositoryId), RendezVous<ChangesetId, Vec<FetchedMappingEntry>>>,
    opts: RendezVousOptions,
    stats: Arc<RendezVousStats>,
    conn: Connection,
}

impl RendezVousConnection {
    fn new(conn: Connection, name: &str, opts: RendezVousOptions) -> Self {
        Self {
            fetch_synced_commit_mappings: Default::default(),
            opts,
            stats: Arc::new(RendezVousStats::new(format!(
                "synced_commit_mapping.fetch_synced_commit_mappings.{}",
                name,
            ))),
            conn,
        }
    }

    fn per_repo_pair_rendezvous(
        &self,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
    ) -> RendezVous<ChangesetId, Vec<FetchedMappingEntry>> {
        self.fetch_synced_commit_mappings
            .entry((source_repo_id, target_repo_id))
            .or_insert_with(|| {
                RendezVous::new(
                    ConfigurableRendezVousController::new(self.opts),
                    self.stats.clone(),
                )
            })
            .clone()
    }
}

#[derive(Clone)]
pub struct SqlSyncedCommitMapping {
    write_connection: Connection,
    read_connection: RendezVousConnection,
    read_master_connection: RendezVousConnection,
}

mononoke_queries! {
    write InsertMapping(values: (
        large_repo_id: RepositoryId,
        large_bcs_id: ChangesetId,
        small_repo_id: RepositoryId,
        small_bcs_id: ChangesetId,
        sync_map_version_name: Option<CommitSyncConfigVersion>,
        source_repo: Option<SyncedCommitSourceRepo>,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO synced_commit_mapping (large_repo_id, large_bcs_id, small_repo_id, small_bcs_id, sync_map_version_name, source_repo) VALUES {values}"
    }

    read SelectManyMappings(
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
        >list bcs_ids: ChangesetId
    ) -> (ChangesetId, ChangesetId, Option<CommitSyncConfigVersion>, Option<SyncedCommitSourceRepo>) {
        "SELECT large_bcs_id as source_bcs_id, small_bcs_id as target_bcs_id, sync_map_version_name, source_repo
          FROM synced_commit_mapping
          WHERE large_repo_id = {source_repo_id} AND large_bcs_id IN {bcs_ids} AND small_repo_id = {target_repo_id}

        UNION

        SELECT small_bcs_id as source_bcs_id, large_bcs_id as target_bcs_id, sync_map_version_name, source_repo
          FROM synced_commit_mapping
          WHERE small_repo_id = {source_repo_id} AND small_bcs_id IN {bcs_ids} AND large_repo_id = {target_repo_id}"
    }

    write InsertWorkingCopyEquivalence(values: (
        large_repo_id: RepositoryId,
        large_bcs_id: ChangesetId,
        small_repo_id: RepositoryId,
        small_bcs_id: Option<ChangesetId>,
        sync_map_version_name: Option<CommitSyncConfigVersion>,
    )) {
        insert_or_ignore,
        "{insert_or_ignore}
         INTO synced_working_copy_equivalence
         (large_repo_id, large_bcs_id, small_repo_id, small_bcs_id, sync_map_version_name)
         VALUES {values}"
    }

    write ReplaceWorkingCopyEquivalence(values: (
        large_repo_id: RepositoryId,
        large_bcs_id: ChangesetId,
        small_repo_id: RepositoryId,
        small_bcs_id: Option<ChangesetId>,
        sync_map_version_name: Option<CommitSyncConfigVersion>,
    )) {
        none,
        "REPLACE
         INTO synced_working_copy_equivalence
         (large_repo_id, large_bcs_id, small_repo_id, small_bcs_id, sync_map_version_name)
         VALUES {values}"
    }

    read SelectWorkingCopyEquivalence(
        source_repo_id: RepositoryId,
        bcs_id: ChangesetId,
        target_repo_id: RepositoryId,
    ) -> (RepositoryId, ChangesetId, RepositoryId, Option<ChangesetId>, Option<CommitSyncConfigVersion>) {
        "SELECT large_repo_id, large_bcs_id, small_repo_id, small_bcs_id, sync_map_version_name
          FROM synced_working_copy_equivalence
          WHERE (large_repo_id = {source_repo_id} AND small_repo_id = {target_repo_id} AND large_bcs_id = {bcs_id})
          OR (large_repo_id = {target_repo_id} AND small_repo_id = {source_repo_id} AND small_bcs_id = {bcs_id})
          ORDER BY mapping_id ASC
          LIMIT 1
          "
    }

    write InsertVersionForLargeRepoCommit(values: (
        large_repo_id: RepositoryId,
        large_bcs_id: ChangesetId,
        sync_map_version_name: CommitSyncConfigVersion,
    )) {
        insert_or_ignore,
        "{insert_or_ignore}
        INTO version_for_large_repo_commit
        (large_repo_id, large_bcs_id, sync_map_version_name)
        VALUES {values}"
    }

    write ReplaceVersionForLargeRepoCommit(values: (
        large_repo_id: RepositoryId,
        large_bcs_id: ChangesetId,
        sync_map_version_name: CommitSyncConfigVersion,
    )) {
        none,
        "REPLACE
        INTO version_for_large_repo_commit
        (large_repo_id, large_bcs_id, sync_map_version_name)
        VALUES {values}"
    }

    read SelectVersionForLargeRepoCommit(
        large_repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> (CommitSyncConfigVersion,) {
        "SELECT sync_map_version_name
          FROM version_for_large_repo_commit
          WHERE large_repo_id = {large_repo_id} AND large_bcs_id = {cs_id}"
    }
}

impl SqlSyncedCommitMapping {
    async fn add_many(
        &self,
        ctx: &CoreContext,
        entries: Vec<SyncedCommitMappingEntry>,
    ) -> Result<u64, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        let txn = self.write_connection.start_transaction().await?;
        let (txn, affected_rows) = add_many_in_txn(ctx, txn, entries).await?;
        txn.commit().await?;
        Ok(affected_rows)
    }

    async fn get_many_impl(
        &self,
        ctx: &CoreContext,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
        bcs_ids: &[ChangesetId],
        rendezvous: &RendezVousConnection,
    ) -> Result<HashMap<ChangesetId, Vec<FetchedMappingEntry>>, Error> {
        if bcs_ids.is_empty() {
            // SQL doesn't support querying empty lists.
            return Ok(HashMap::new());
        }

        let map = rendezvous
            .per_repo_pair_rendezvous(source_repo_id, target_repo_id)
            .dispatch(ctx.fb, bcs_ids.iter().copied().collect(), || {
                let conn = rendezvous.conn.clone();
                let cri = ctx.client_request_info().cloned();

                move |bcs_ids| async move {
                    let bcs_ids = bcs_ids.into_iter().collect::<Vec<_>>();
                    let rows = SelectManyMappings::maybe_traced_query(
                        &conn,
                        cri.as_ref(),
                        &source_repo_id,
                        &target_repo_id,
                        &bcs_ids,
                    )
                    .await?;
                    Ok(rows
                        .into_iter()
                        .map(|row| {
                            let (
                                source_bcs_id,
                                target_bcs_id,
                                maybe_version_name,
                                maybe_source_repo,
                            ) = row;
                            (
                                source_bcs_id,
                                FetchedMappingEntry {
                                    target_bcs_id,
                                    maybe_version_name,
                                    maybe_source_repo,
                                },
                            )
                        })
                        .into_group_map())
                }
            })
            .await?;

        Ok(map
            .into_iter()
            .map(|(bcs_id, entries)| (bcs_id, entries.unwrap_or_default()))
            .collect())
    }

    async fn insert_or_overwrite_equivalent_working_copy(
        &self,
        ctx: &CoreContext,
        entry: EquivalentWorkingCopyEntry,
        should_overwrite: bool,
    ) -> Result<bool, Error> {
        STATS::insert_working_copy_eqivalence.add_value(1);

        let EquivalentWorkingCopyEntry {
            large_repo_id,
            large_bcs_id,
            small_repo_id,
            small_bcs_id,
            version_name,
        } = entry;

        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        if let Some(ref version_name) = version_name {
            // TODO(stash): make version non-optional
            self.insert_version_for_large_repo_commit(
                ctx,
                &self.write_connection,
                large_repo_id,
                large_bcs_id,
                version_name,
                should_overwrite,
            )
            .await?;
        }
        let result = if should_overwrite {
            ReplaceWorkingCopyEquivalence::maybe_traced_query(
                &self.write_connection,
                ctx.client_request_info(),
                &[(
                    &large_repo_id,
                    &large_bcs_id,
                    &small_repo_id,
                    &small_bcs_id,
                    &version_name,
                )],
            )
            .await?
        } else {
            InsertWorkingCopyEquivalence::maybe_traced_query(
                &self.write_connection,
                ctx.client_request_info(),
                &[(
                    &large_repo_id,
                    &large_bcs_id,
                    &small_repo_id,
                    &small_bcs_id,
                    &version_name,
                )],
            )
            .await?
        };

        if result.affected_rows() >= 1 {
            Ok(true)
        } else {
            if !should_overwrite {
                // Check that db stores consistent entry
                let maybe_equivalent_wc = self
                    .get_equivalent_working_copy(ctx, large_repo_id, large_bcs_id, small_repo_id)
                    .await?;

                if let Some(equivalent_wc) = maybe_equivalent_wc {
                    use WorkingCopyEquivalence::*;
                    let (expected_bcs_id, expected_version) = match equivalent_wc {
                        WorkingCopy(wc, mapping) => (Some(wc), mapping),
                        NoWorkingCopy(mapping) => (None, mapping),
                    };
                    let expected_version = Some(expected_version);
                    if (expected_bcs_id != small_bcs_id) || (expected_version != version_name) {
                        let err = ErrorKind::InconsistentWorkingCopyEntry {
                            expected_bcs_id,
                            expected_config_version: expected_version,
                            actual_bcs_id: small_bcs_id,
                            actual_config_version: version_name,
                        };
                        return Err(err.into());
                    }
                }
            }
            Ok(false)
        }
    }

    async fn insert_version_for_large_repo_commit(
        &self,
        ctx: &CoreContext,
        write_connection: &Connection,
        large_repo_id: RepositoryId,
        large_cs_id: ChangesetId,
        version_name: &CommitSyncConfigVersion,
        should_overwrite: bool,
    ) -> Result<bool, Error> {
        let result = if should_overwrite {
            ReplaceVersionForLargeRepoCommit::maybe_traced_query(
                write_connection,
                ctx.client_request_info(),
                &[(&large_repo_id, &large_cs_id, version_name)],
            )
            .await?
        } else {
            InsertVersionForLargeRepoCommit::maybe_traced_query(
                write_connection,
                ctx.client_request_info(),
                &[(&large_repo_id, &large_cs_id, version_name)],
            )
            .await?
        };

        if result.affected_rows() >= 1 {
            Ok(true)
        } else {
            if !should_overwrite {
                // Check that db stores consistent entry
                let maybe_large_repo_version = self
                    .get_large_repo_commit_version(ctx, large_repo_id, large_cs_id)
                    .await?;

                if let Some(actual_version_name) = maybe_large_repo_version {
                    if &actual_version_name != version_name {
                        let err = ErrorKind::InconsistentLargeRepoCommitVersion {
                            large_repo_id,
                            large_cs_id,
                            expected_version_name: version_name.clone(),
                            actual_version_name,
                        };
                        return Err(err.into());
                    }
                }
            }
            Ok(false)
        }
    }
}

#[async_trait]
impl SyncedCommitMapping for SqlSyncedCommitMapping {
    async fn add(&self, ctx: &CoreContext, entry: SyncedCommitMappingEntry) -> Result<bool, Error> {
        STATS::adds.add_value(1);

        self.add_many(ctx, vec![entry])
            .await
            .map(|count| count == 1)
    }

    async fn add_bulk(
        &self,
        ctx: &CoreContext,
        entries: Vec<SyncedCommitMappingEntry>,
    ) -> Result<u64, Error> {
        STATS::add_bulks.add_value(1);

        self.add_many(ctx, entries).await
    }

    async fn get_many(
        &self,
        ctx: &CoreContext,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
        bcs_ids: &[ChangesetId],
    ) -> Result<HashMap<ChangesetId, Vec<FetchedMappingEntry>>, Error> {
        STATS::gets.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let mut map = self
            .get_many_impl(
                ctx,
                source_repo_id,
                target_repo_id,
                bcs_ids,
                &self.read_connection,
            )
            .await?;

        let missing_bcs_ids = bcs_ids
            .iter()
            .filter(|bcs_id| !map.contains_key(bcs_id))
            .copied()
            .collect::<Vec<_>>();

        if !missing_bcs_ids.is_empty() {
            STATS::gets_master.add_value(1);
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);

            let mut fetched_from_master_map = self
                .get_many_impl(
                    ctx,
                    source_repo_id,
                    target_repo_id,
                    &missing_bcs_ids,
                    &self.read_master_connection,
                )
                .await?;

            for bcs_id in missing_bcs_ids {
                map.entry(bcs_id)
                    .or_default()
                    .extend(fetched_from_master_map.remove(&bcs_id).unwrap_or_default());
            }
        }

        Ok(map)
    }

    async fn get_many_maybe_stale(
        &self,
        ctx: &CoreContext,
        source_repo_id: RepositoryId,
        target_repo_id: RepositoryId,
        bcs_ids: &[ChangesetId],
    ) -> Result<HashMap<ChangesetId, Vec<FetchedMappingEntry>>, Error> {
        STATS::gets.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        self.get_many_impl(
            ctx,
            source_repo_id,
            target_repo_id,
            bcs_ids,
            &self.read_connection,
        )
        .await
    }

    async fn insert_equivalent_working_copy(
        &self,
        ctx: &CoreContext,
        entry: EquivalentWorkingCopyEntry,
    ) -> Result<bool, Error> {
        self.insert_or_overwrite_equivalent_working_copy(
            ctx, entry, false, /* should overwrite */
        )
        .await
    }

    async fn overwrite_equivalent_working_copy(
        &self,
        ctx: &CoreContext,
        entry: EquivalentWorkingCopyEntry,
    ) -> Result<bool, Error> {
        self.insert_or_overwrite_equivalent_working_copy(
            ctx, entry, true, /* should overwrite */
        )
        .await
    }

    async fn get_equivalent_working_copy(
        &self,
        ctx: &CoreContext,
        source_repo_id: RepositoryId,
        source_bcs_id: ChangesetId,
        target_repo_id: RepositoryId,
    ) -> Result<Option<WorkingCopyEquivalence>, Error> {
        STATS::get_equivalent_working_copy.add_value(1);

        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let rows = SelectWorkingCopyEquivalence::maybe_traced_query(
            &self.read_connection.conn,
            ctx.client_request_info(),
            &source_repo_id,
            &source_bcs_id,
            &target_repo_id,
        )
        .await?;
        let maybe_row = if !rows.is_empty() {
            rows.first().cloned()
        } else {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            SelectWorkingCopyEquivalence::maybe_traced_query(
                &self.read_master_connection.conn,
                ctx.client_request_info(),
                &source_repo_id,
                &source_bcs_id,
                &target_repo_id,
            )
            .await
            .map(|rows| rows.first().cloned())?
        };

        Ok(match maybe_row {
            Some(row) => {
                let (
                    large_repo_id,
                    large_bcs_id,
                    _small_repo_id,
                    maybe_small_bcs_id,
                    maybe_mapping,
                ) = row;

                let mapping = maybe_mapping.ok_or_else(|| {
                    anyhow!(
                        "unexpected empty mapping for {}, {}->{}",
                        source_bcs_id,
                        source_repo_id,
                        target_repo_id
                    )
                })?;
                if target_repo_id == large_repo_id {
                    Some(WorkingCopyEquivalence::WorkingCopy(large_bcs_id, mapping))
                } else {
                    match maybe_small_bcs_id {
                        Some(small_bcs_id) => {
                            Some(WorkingCopyEquivalence::WorkingCopy(small_bcs_id, mapping))
                        }
                        None => Some(WorkingCopyEquivalence::NoWorkingCopy(mapping)),
                    }
                }
            }
            None => None,
        })
    }

    async fn insert_large_repo_commit_version(
        &self,
        ctx: &CoreContext,
        large_repo_id: RepositoryId,
        large_repo_cs_id: ChangesetId,
        version_name: &CommitSyncConfigVersion,
    ) -> Result<bool, Error> {
        self.insert_version_for_large_repo_commit(
            ctx,
            &self.write_connection,
            large_repo_id,
            large_repo_cs_id,
            version_name,
            false, /* should overwrite */
        )
        .await
    }

    async fn overwrite_large_repo_commit_version(
        &self,
        ctx: &CoreContext,
        large_repo_id: RepositoryId,
        large_repo_cs_id: ChangesetId,
        version_name: &CommitSyncConfigVersion,
    ) -> Result<bool, Error> {
        self.insert_version_for_large_repo_commit(
            ctx,
            &self.write_connection,
            large_repo_id,
            large_repo_cs_id,
            version_name,
            true, /* should overwrite */
        )
        .await
    }

    async fn get_large_repo_commit_version(
        &self,
        ctx: &CoreContext,
        large_repo_id: RepositoryId,
        large_repo_cs_id: ChangesetId,
    ) -> Result<Option<CommitSyncConfigVersion>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let maybe_version = SelectVersionForLargeRepoCommit::maybe_traced_query(
            &self.read_connection.conn,
            ctx.client_request_info(),
            &large_repo_id,
            &large_repo_cs_id,
        )
        .await?
        .pop()
        .map(|x| x.0);

        if let Some(version) = maybe_version {
            return Ok(Some(version));
        }

        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);
        Ok(SelectVersionForLargeRepoCommit::maybe_traced_query(
            &self.read_master_connection.conn,
            ctx.client_request_info(),
            &large_repo_id,
            &large_repo_cs_id,
        )
        .await?
        .pop()
        .map(|x| x.0))
    }
}

pub async fn add_many_in_txn(
    ctx: &CoreContext,
    txn: Transaction,
    entries: Vec<SyncedCommitMappingEntry>,
) -> Result<(Transaction, u64), Error> {
    STATS::add_many_in_txn.add_value(1);

    let insert_entries: Vec<_> = entries
        .iter()
        .map(|entry| {
            (
                &entry.large_repo_id,
                &entry.large_bcs_id,
                &entry.small_repo_id,
                &entry.small_bcs_id,
                &entry.version_name,
                &entry.source_repo,
            )
        })
        .collect();

    let (txn, _result) = InsertMapping::maybe_traced_query_with_transaction(
        txn,
        ctx.client_request_info(),
        &insert_entries,
    )
    .await?;
    let owned_entries: Vec<_> = entries
        .into_iter()
        .map(|entry| entry.into_equivalent_working_copy_entry())
        .collect();

    let mut large_repo_commit_versions = vec![];
    for entry in &owned_entries {
        if let Some(version_name) = &entry.version_name {
            large_repo_commit_versions.push((
                &entry.large_repo_id,
                &entry.large_bcs_id,
                version_name,
            ));
        }
    }
    let (txn, _result) = InsertVersionForLargeRepoCommit::maybe_traced_query_with_transaction(
        txn,
        ctx.client_request_info(),
        &large_repo_commit_versions,
    )
    .await?;

    let ref_entries: Vec<_> = owned_entries
        .iter()
        .map(|entry| {
            (
                &entry.large_repo_id,
                &entry.large_bcs_id,
                &entry.small_repo_id,
                &entry.small_bcs_id,
                &entry.version_name,
            )
        })
        .collect();

    let (txn, result) = InsertWorkingCopyEquivalence::maybe_traced_query_with_transaction(
        txn,
        ctx.client_request_info(),
        &ref_entries,
    )
    .await?;
    Ok((txn, result.affected_rows()))
}

pub async fn add_many_large_repo_commit_versions_in_txn(
    ctx: &CoreContext,
    txn: Transaction,
    large_repo_commit_versions: &[(RepositoryId, ChangesetId, CommitSyncConfigVersion)],
) -> Result<(Transaction, u64), Error> {
    let (txn, result) = InsertVersionForLargeRepoCommit::maybe_traced_query_with_transaction(
        txn,
        ctx.client_request_info(),
        &large_repo_commit_versions
            .iter()
            .map(|(repo_id, cs_id, version_name)| (repo_id, cs_id, version_name))
            .collect::<Vec<_>>(),
    )
    .await?;
    Ok((txn, result.affected_rows()))
}
