/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use context::PerfCounterType;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use pushrebase_hook::PushrebaseHook;
use sql::queries;
use sql::Connection;
use sql::Transaction;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use tunables::tunables;

use crate::save_mapping_pushrebase_hook::SaveMappingPushrebaseHook;
use crate::PushrebaseMutationMapping;
use crate::PushrebaseMutationMappingEntry;

queries! {
    read SelectPrepushrebaseIds(
        repo_id: RepositoryId,
        successor_bcs_id: ChangesetId,
    ) -> (ChangesetId,) {
        "SELECT predecessor_bcs_id
        FROM pushrebase_mutation_mapping
        WHERE repo_id = {repo_id} AND successor_bcs_id = {successor_bcs_id}"
    }

    write InsertMappingEntries(values:(
        repo_id: RepositoryId,
        predecessor_bcs_id: ChangesetId,
        successor_bcs_id: ChangesetId,
    )) {
        insert_or_ignore,
       "{insert_or_ignore}
       INTO pushrebase_mutation_mapping
       (repo_id, predecessor_bcs_id, successor_bcs_id)
       VALUES {values}"
    }
}

pub async fn add_pushrebase_mapping(
    transaction: Transaction,
    entries: &[PushrebaseMutationMappingEntry],
) -> Result<Transaction> {
    let entries: Vec<_> = entries
        .iter()
        .map(
            |PushrebaseMutationMappingEntry {
                 repo_id,
                 predecessor_bcs_id,
                 successor_bcs_id,
             }| (repo_id, predecessor_bcs_id, successor_bcs_id),
        )
        .collect();

    let (transaction, _) =
        InsertMappingEntries::query_with_transaction(transaction, &entries).await?;

    Ok(transaction)
}

pub async fn get_prepushrebase_ids(
    connection: &Connection,
    repo_id: RepositoryId,
    successor_bcs_id: ChangesetId,
) -> Result<Vec<ChangesetId>> {
    let rows = SelectPrepushrebaseIds::query(connection, &repo_id, &successor_bcs_id).await?;

    Ok(rows.into_iter().map(|r| r.0).collect())
}

pub struct SqlPushrebaseMutationMapping {
    repo_id: RepositoryId,
    sql_conn: SqlPushrebaseMutationMappingConnection,
}

impl SqlPushrebaseMutationMapping {
    pub fn new(repo_id: RepositoryId, sql_conn: SqlPushrebaseMutationMappingConnection) -> Self {
        Self { repo_id, sql_conn }
    }
}

#[derive(Clone)]
pub struct SqlPushrebaseMutationMappingConnection {
    #[allow(dead_code)]
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
}

impl SqlPushrebaseMutationMappingConnection {
    pub fn with_repo_id(self, repo_id: RepositoryId) -> SqlPushrebaseMutationMapping {
        SqlPushrebaseMutationMapping::new(repo_id, self)
    }

    async fn get_prepushrebase_ids(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        successor_bcs_id: ChangesetId,
    ) -> Result<Vec<ChangesetId>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let mut ids =
            get_prepushrebase_ids(&self.read_connection, repo_id, successor_bcs_id).await?;
        if ids.is_empty() {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            ids = get_prepushrebase_ids(&self.read_master_connection, repo_id, successor_bcs_id)
                .await?;
        }
        Ok(ids)
    }
}

impl SqlConstruct for SqlPushrebaseMutationMappingConnection {
    const LABEL: &'static str = "pushrebase_mutation_mapping";

    const CREATION_QUERY: &'static str =
        include_str!("../schemas/sqlite-pushrebase-mutation-mapping.sql");

    // We don't need the connections because we never use them.
    // But we need SqlConstruct to get our SQL tables created in tests.
    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
            read_master_connection: connections.read_master_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlPushrebaseMutationMappingConnection {}

#[async_trait]
impl PushrebaseMutationMapping for SqlPushrebaseMutationMapping {
    fn get_hook(&self) -> Option<Box<dyn PushrebaseHook>> {
        if tunables().get_disable_save_mapping_pushrebase_hook() {
            None
        } else {
            Some(SaveMappingPushrebaseHook::new(self.repo_id))
        }
    }

    async fn get_prepushrebase_ids(
        &self,
        ctx: &CoreContext,
        successor_bcs_id: ChangesetId,
    ) -> Result<Vec<ChangesetId>> {
        self.sql_conn
            .get_prepushrebase_ids(ctx, self.repo_id, successor_bcs_id)
            .await
    }
}
