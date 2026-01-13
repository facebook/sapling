/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::sync::Arc;

use ::sql_ext::Connection;
use ::sql_ext::Transaction;
use ::sql_ext::mononoke_queries;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use context::PerfCounterType;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::hash::GitSha1;
use rendezvous::ConfigurableRendezVousController;
use rendezvous::RendezVous;
use rendezvous::RendezVousOptions;
use rendezvous::RendezVousStats;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use sql_ext::SqlQueryTelemetry;
use stats::prelude::*;

use crate::BonsaiGitMapping;
use crate::BonsaiGitMappingEntry;
use crate::BonsaisOrGitShas;
use crate::errors::AddGitMappingErrorKind;

define_stats! {
    prefix = "mononoke.bonsai_git_mapping";
    gets: timeseries(Rate, Sum),
    gets_master: timeseries(Rate, Sum),
    adds: timeseries(Rate, Sum),
    // Number of mappings that were not found in the replica
    left_to_fetch: timeseries(Sum, Average, Count),
    // Number of mappings that were fetched from the master
    fetched_from_master: timeseries(Sum, Average, Count),
}

#[derive(Clone)]
struct RendezVousConnection {
    bonsai: RendezVous<ChangesetId, GitSha1>,
    git: RendezVous<GitSha1, ChangesetId>,
    conn: Connection,
}

impl RendezVousConnection {
    fn new(conn: Connection, name: &str, opts: RendezVousOptions) -> Self {
        Self {
            conn,
            bonsai: RendezVous::new(
                ConfigurableRendezVousController::new(opts),
                Arc::new(RendezVousStats::new(format!(
                    "bonsai_git_mapping.bonsai.{}",
                    name,
                ))),
            ),
            git: RendezVous::new(
                ConfigurableRendezVousController::new(opts),
                Arc::new(RendezVousStats::new(format!(
                    "bonsai_git_mapping.git.{}",
                    name,
                ))),
            ),
        }
    }
}

pub struct SqlBonsaiGitMapping {
    write_connection: Connection,
    read_connection: RendezVousConnection,
    read_master_connection: RendezVousConnection,
    repo_id: RepositoryId,
}

mononoke_queries! {
    write InsertMapping(values: (
        repo_id: RepositoryId,
        git_sha1: GitSha1,
        bcs_id: ChangesetId,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bonsai_git_mapping (repo_id, git_sha1, bcs_id) VALUES {values}"
    }
    read SelectMappingByBonsai(
        repo_id: RepositoryId,
        >list bcs_id: ChangesetId
    ) -> (GitSha1, ChangesetId) {
        "SELECT git_sha1, bcs_id
         FROM bonsai_git_mapping
         WHERE repo_id = {repo_id}
           AND bcs_id IN {bcs_id}"
    }

    read SelectMappingByGitSha1(
        repo_id: RepositoryId,
        >list git_sha1: GitSha1
    ) -> (GitSha1, ChangesetId) {
        "SELECT git_sha1, bcs_id
         FROM bonsai_git_mapping
         WHERE repo_id = {repo_id}
           AND git_sha1 IN {git_sha1}"
    }

    read SelectGitSha1sByRange(
        repo_id: RepositoryId,
        git_sha1_min: GitSha1,
        git_sha1_max: GitSha1,
        limit: usize
    ) -> (GitSha1) {
        "SELECT git_sha1
         FROM bonsai_git_mapping
         WHERE repo_id = {repo_id}
            AND git_sha1 >= {git_sha1_min} AND git_sha1 <= {git_sha1_max}
            LIMIT {limit}
        "
    }
}

#[async_trait]
impl BonsaiGitMapping for SqlBonsaiGitMapping {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn bulk_add(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGitMappingEntry],
    ) -> Result<(), AddGitMappingErrorKind> {
        let txn = self
            .write_connection
            .start_transaction(ctx.sql_query_telemetry())
            .await?;
        let txn = self
            .bulk_add_git_mapping_in_transaction(ctx, entries, txn)
            .await?;
        txn.commit().await?;
        Ok(())
    }

    async fn bulk_add_git_mapping_in_transaction(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGitMappingEntry],
        transaction: Transaction,
    ) -> Result<Transaction, AddGitMappingErrorKind> {
        STATS::adds.add_value(entries.len().try_into().map_err(Error::from)?);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        let rows: Vec<_> = entries
            .iter()
            .map(|BonsaiGitMappingEntry { git_sha1, bcs_id }| (&self.repo_id, git_sha1, bcs_id))
            .collect();

        let (transaction, res) =
            InsertMapping::query_with_transaction(transaction, &rows[..]).await?;

        let transaction = if res.affected_rows() != rows.len() as u64 {
            // Let's see if there are any conflicting entries in DB.
            let git_shas = entries.iter().map(|x| x.git_sha1).collect::<Vec<_>>();
            let (transaction, git2bonsai_mapping_from_db) =
                SelectMappingByGitSha1::query_with_transaction(
                    transaction,
                    &self.repo_id,
                    &git_shas[..],
                )
                .await?;
            let git2bonsai_mapping_from_db: BTreeMap<_, _> =
                git2bonsai_mapping_from_db.into_iter().collect();

            let bcs_ids = entries.iter().map(|x| x.bcs_id).collect::<Vec<_>>();
            let (transaction, bonsai2git_mapping_from_db) =
                SelectMappingByBonsai::query_with_transaction(
                    transaction,
                    &self.repo_id,
                    &bcs_ids[..],
                )
                .await?;
            let bonsai2git_mapping_from_db: BTreeMap<_, _> = bonsai2git_mapping_from_db
                .into_iter()
                .map(|(a, b)| (b, a))
                .collect();

            for entry in entries.iter() {
                match (
                    git2bonsai_mapping_from_db.get(&entry.git_sha1),
                    bonsai2git_mapping_from_db.get(&entry.bcs_id),
                ) {
                    (Some(bcs_id), _) if bcs_id == &entry.bcs_id => {} // We've tried to insert a duplicate, proceed.
                    (Some(bcs_id), None) => {
                        // Conflict git_sha1 already mapped to a different bcs_id.
                        return Err(AddGitMappingErrorKind::Conflict(
                            Some(BonsaiGitMappingEntry {
                                git_sha1: entry.git_sha1,
                                bcs_id: *bcs_id,
                            }),
                            vec![entry.clone()],
                        ));
                    }
                    (None, Some(git_sha1)) => {
                        // Conflict bcs_id already mapped to a different git_sha1.
                        return Err(AddGitMappingErrorKind::Conflict(
                            Some(BonsaiGitMappingEntry {
                                git_sha1: *git_sha1,
                                bcs_id: entry.bcs_id,
                            }),
                            vec![entry.clone()],
                        ));
                    }
                    _ => {
                        return Err(AddGitMappingErrorKind::Conflict(None, vec![entry.clone()]));
                    }
                }
            }

            transaction
        } else {
            transaction
        };

        Ok(transaction)
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        objects: BonsaisOrGitShas,
    ) -> Result<Vec<BonsaiGitMappingEntry>> {
        STATS::gets.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let mut mappings =
            select_mapping(ctx, &self.read_connection, &self.repo_id, &objects).await?;
        let left_to_fetch = filter_fetched_ids(objects, &mappings);
        let left_to_fetch_count = left_to_fetch.count().try_into().map_err(Error::from)?;

        STATS::left_to_fetch.add_value(left_to_fetch_count);

        let client_correlator = ctx.client_correlator();

        // Callsites that don't require the most recent bookmark value should
        // read from a replica. More context on D81212709.
        let disable_primary_fallback = justknobs::eval(
            "scm/mononoke:disable_bonsai_mapping_read_fallback_to_primary",
            client_correlator,
            Some("git"),
        )
        .unwrap_or(false);

        if !left_to_fetch.is_empty() && !disable_primary_fallback {
            STATS::gets_master.add_value(1);
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            let mut master_mappings = select_mapping(
                ctx,
                &self.read_master_connection,
                &self.repo_id,
                &left_to_fetch,
            )
            .await?;
            let fetched_from_master_count =
                master_mappings.len().try_into().map_err(Error::from)?;
            STATS::fetched_from_master.add_value(fetched_from_master_count);
            mappings.append(&mut master_mappings);
        }
        Ok(mappings)
    }

    /// Return [`GitSha1`] entries in the inclusive range described by `low` and `high`.
    /// Maximum `limit` entries will be returned.
    async fn get_in_range(
        &self,
        ctx: &CoreContext,
        low: GitSha1,
        high: GitSha1,
        limit: usize,
    ) -> Result<Vec<GitSha1>, Error> {
        if low > high {
            return Ok(Vec::new());
        }
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let rows = SelectGitSha1sByRange::query(
            &self.read_connection.conn,
            ctx.sql_query_telemetry(),
            &self.repo_id,
            &low,
            &high,
            &limit,
        )
        .await?;
        let mut fetched: Vec<GitSha1> = rows.into_iter().map(|row| row.0).collect();
        if fetched.is_empty() {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            let rows = SelectGitSha1sByRange::query(
                &self.read_master_connection.conn,
                ctx.sql_query_telemetry(),
                &self.repo_id,
                &low,
                &high,
                &limit,
            )
            .await?;
            fetched = rows.into_iter().map(|row| row.0).collect();
        }
        Ok(fetched)
    }
}

pub struct SqlBonsaiGitMappingBuilder {
    connections: SqlConnections,
}

impl SqlBonsaiGitMappingBuilder {
    pub fn build(self, repo_id: RepositoryId, opts: RendezVousOptions) -> SqlBonsaiGitMapping {
        let SqlBonsaiGitMappingBuilder { connections } = self;
        SqlBonsaiGitMapping {
            write_connection: connections.write_connection,
            read_connection: RendezVousConnection::new(connections.read_connection, "reader", opts),
            read_master_connection: RendezVousConnection::new(
                connections.read_master_connection,
                "read_master",
                opts,
            ),
            repo_id,
        }
    }
}

impl SqlConstruct for SqlBonsaiGitMappingBuilder {
    const LABEL: &'static str = "bonsai_git_mapping";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-bonsai-git-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlBonsaiGitMappingBuilder {
    fn remote_database_config(
        remote: &RemoteMetadataDatabaseConfig,
    ) -> Option<&RemoteDatabaseConfig> {
        Some(&remote.bookmarks)
    }
    fn oss_remote_database_config(
        remote: &OssRemoteMetadataDatabaseConfig,
    ) -> Option<&OssRemoteDatabaseConfig> {
        Some(&remote.bookmarks)
    }
}

fn filter_fetched_ids(
    cs: BonsaisOrGitShas,
    mappings: &[BonsaiGitMappingEntry],
) -> BonsaisOrGitShas {
    match cs {
        BonsaisOrGitShas::Bonsai(cs_ids) => {
            let bcs_fetched: HashSet<_> = mappings.iter().map(|m| &m.bcs_id).collect();

            BonsaisOrGitShas::Bonsai(
                cs_ids
                    .iter()
                    .filter_map(|cs| {
                        if !bcs_fetched.contains(cs) {
                            Some(*cs)
                        } else {
                            None
                        }
                    })
                    .collect(),
            )
        }
        BonsaisOrGitShas::GitSha1(git_sha1s) => {
            let git_fetched: HashSet<_> = mappings.iter().map(|m| &m.git_sha1).collect();

            BonsaisOrGitShas::GitSha1(
                git_sha1s
                    .iter()
                    .filter_map(|cs| {
                        if !git_fetched.contains(cs) {
                            Some(*cs)
                        } else {
                            None
                        }
                    })
                    .collect(),
            )
        }
    }
}

async fn select_mapping(
    ctx: &CoreContext,
    connection: &RendezVousConnection,
    repo_id: &RepositoryId,
    objects: &BonsaisOrGitShas,
) -> Result<Vec<BonsaiGitMappingEntry>> {
    if objects.is_empty() {
        return Ok(vec![]);
    }

    let use_rendezvous = justknobs::eval(
        "scm/mononoke:rendezvous_bonsai_git_mapping",
        ctx.client_correlator(),
        None,
    )?;

    if use_rendezvous {
        select_mapping_rendezvous(ctx, connection, repo_id, objects).await
    } else {
        select_mapping_non_rendezvous(ctx, &connection.conn, repo_id, objects).await
    }
}

async fn select_mapping_rendezvous(
    ctx: &CoreContext,
    connection: &RendezVousConnection,
    repo_id: &RepositoryId,
    objects: &BonsaisOrGitShas,
) -> Result<Vec<BonsaiGitMappingEntry>> {
    let found = match objects {
        BonsaisOrGitShas::Bonsai(bcs_ids) => {
            let ret = connection
                .bonsai
                .dispatch(ctx.fb, bcs_ids.iter().copied().collect(), || {
                    let repo_id = *repo_id;
                    let conn = connection.conn.clone();
                    let telemetry = ctx.sql_query_telemetry().clone();
                    move |bcs_ids: HashSet<ChangesetId>| async move {
                        let bcs_ids = bcs_ids.into_iter().collect::<Vec<_>>();
                        let res =
                            SelectMappingByBonsai::query(&conn, telemetry, &repo_id, &bcs_ids[..])
                                .await?;
                        Ok(res
                            .into_iter()
                            .map(|(git_sha1, bcs_id)| (bcs_id, git_sha1))
                            .collect())
                    }
                })
                .await?;

            ret.into_iter()
                .filter_map(|(bcs_id, git_sha1)| git_sha1.map(|git_sha1| (git_sha1, bcs_id)))
                .collect()
        }
        BonsaisOrGitShas::GitSha1(git_sha1s) => {
            let ret = connection
                .git
                .dispatch(ctx.fb, git_sha1s.iter().copied().collect(), || {
                    let repo_id = *repo_id;
                    let conn = connection.conn.clone();
                    let telemetry = ctx.sql_query_telemetry().clone();
                    move |git_shas: HashSet<GitSha1>| async move {
                        let git_shas = git_shas.into_iter().collect::<Vec<_>>();
                        let res = SelectMappingByGitSha1::query(
                            &conn,
                            telemetry,
                            &repo_id,
                            &git_shas[..],
                        )
                        .await?;
                        Ok(res.into_iter().collect())
                    }
                })
                .await?;

            let found: Vec<(GitSha1, ChangesetId)> = ret
                .into_iter()
                .filter_map(|(git_sha1, bcs_id)| bcs_id.map(|bcs_id| (git_sha1, bcs_id)))
                .collect();
            found
        }
    };

    Ok(found
        .into_iter()
        .map(move |(git_sha1, bcs_id)| BonsaiGitMappingEntry { git_sha1, bcs_id })
        .collect())
}

async fn select_mapping_non_rendezvous(
    ctx: &CoreContext,
    connection: &Connection,
    repo_id: &RepositoryId,
    objects: &BonsaisOrGitShas,
) -> Result<Vec<BonsaiGitMappingEntry>> {
    if objects.is_empty() {
        return Ok(vec![]);
    }

    let rows = match objects {
        BonsaisOrGitShas::Bonsai(bcs_ids) => {
            SelectMappingByBonsai::query(
                connection,
                ctx.sql_query_telemetry(),
                repo_id,
                &bcs_ids[..],
            )
            .await?
        }
        BonsaisOrGitShas::GitSha1(git_sha1s) => {
            SelectMappingByGitSha1::query(
                connection,
                ctx.sql_query_telemetry(),
                repo_id,
                &git_sha1s[..],
            )
            .await?
        }
    };

    Ok(rows
        .into_iter()
        .map(move |(git_sha1, bcs_id)| BonsaiGitMappingEntry { bcs_id, git_sha1 })
        .collect())
}
