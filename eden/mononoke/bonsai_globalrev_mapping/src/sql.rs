/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql::queries;
use ::sql::Connection;
use ::sql::Transaction;
use anyhow::Error;
use async_trait::async_trait;
use context::CoreContext;
use context::PerfCounterType;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::Globalrev;
use mononoke_types::RepositoryId;
use slog::warn;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use std::collections::HashSet;
use thiserror::Error;

use super::BonsaiGlobalrevMapping;
use super::BonsaiGlobalrevMappingEntry;
use super::BonsaisOrGlobalrevs;

queries! {
    write DangerouslyAddGlobalrevs(values: (
        repo_id: RepositoryId,
        bcs_id: ChangesetId,
        globalrev: Globalrev,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bonsai_globalrev_mapping (repo_id, bcs_id, globalrev) VALUES {values}"
    }

    read SelectMappingByBonsai(
        repo_id: RepositoryId,
        >list bcs_id: ChangesetId
    ) -> (ChangesetId, Globalrev) {
        "SELECT bcs_id, globalrev
         FROM bonsai_globalrev_mapping
         WHERE repo_id = {repo_id} AND bcs_id in {bcs_id}"
    }

    read SelectMappingByGlobalrev(
        repo_id: RepositoryId,
        >list globalrev: Globalrev
    ) -> (ChangesetId, Globalrev) {
        "SELECT bcs_id, globalrev
         FROM bonsai_globalrev_mapping
         WHERE repo_id = {repo_id} AND globalrev in {globalrev}"
    }

    read SelectMaxEntry(repo_id: RepositoryId) -> (Globalrev,) {
        "
        SELECT globalrev
        FROM bonsai_globalrev_mapping
        WHERE repo_id = {}
        ORDER BY globalrev DESC
        LIMIT 1
        "
    }

    read SelectClosestGlobalrev(repo_id: RepositoryId, rev: Globalrev) -> (Globalrev,) {
        "
        SELECT globalrev
        FROM bonsai_globalrev_mapping
        WHERE repo_id = {repo_id} AND globalrev <= {rev}
        ORDER BY globalrev DESC
        LIMIT 1
        "
    }
}

#[derive(Clone)]
pub struct SqlBonsaiGlobalrevMapping {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

#[derive(Clone)]
pub struct SqlBonsaiGlobalrevMappingBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlBonsaiGlobalrevMappingBuilder {
    const LABEL: &'static str = "bonsai_globalrev_mapping";

    const CREATION_QUERY: &'static str =
        include_str!("../schemas/sqlite-bonsai-globalrev-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlBonsaiGlobalrevMappingBuilder {}

impl SqlBonsaiGlobalrevMappingBuilder {
    pub fn build(self, repo_id: RepositoryId) -> SqlBonsaiGlobalrevMapping {
        SqlBonsaiGlobalrevMapping {
            connections: self.connections,
            repo_id,
        }
    }
}

#[async_trait]
impl BonsaiGlobalrevMapping for SqlBonsaiGlobalrevMapping {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn bulk_import(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGlobalrevMappingEntry],
    ) -> Result<(), Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);
        let repo_id = self.repo_id;

        let entries: Vec<_> = entries
            .iter()
            .map(|entry| (&repo_id, &entry.bcs_id, &entry.globalrev))
            .collect();

        DangerouslyAddGlobalrevs::query(&self.connections.write_connection, &entries[..]).await?;

        Ok(())
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        objects: BonsaisOrGlobalrevs,
    ) -> Result<Vec<BonsaiGlobalrevMappingEntry>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let mut mappings =
            select_mapping(&self.connections.read_connection, self.repo_id, &objects).await?;

        let left_to_fetch = filter_fetched_objects(objects, &mappings[..]);

        if left_to_fetch.is_empty() {
            return Ok(mappings);
        }

        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);

        let mut master_mappings = select_mapping(
            &self.connections.read_master_connection,
            self.repo_id,
            &left_to_fetch,
        )
        .await?;
        mappings.append(&mut master_mappings);
        Ok(mappings)
    }

    async fn get_closest_globalrev(
        &self,
        ctx: &CoreContext,
        globalrev: Globalrev,
    ) -> Result<Option<Globalrev>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let row = SelectClosestGlobalrev::query(
            &self.connections.read_connection,
            &self.repo_id,
            &globalrev,
        )
        .await?
        .into_iter()
        .next();

        Ok(row.map(|r| r.0))
    }

    async fn get_max(&self, ctx: &CoreContext) -> Result<Option<Globalrev>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);

        let row = SelectMaxEntry::query(&self.connections.read_master_connection, &self.repo_id)
            .await?
            .into_iter()
            .next();

        Ok(row.map(|r| r.0))
    }
}

fn filter_fetched_objects(
    objects: BonsaisOrGlobalrevs,
    mappings: &[BonsaiGlobalrevMappingEntry],
) -> BonsaisOrGlobalrevs {
    match objects {
        BonsaisOrGlobalrevs::Bonsai(cs_ids) => {
            let bcs_fetched: HashSet<_> = mappings.iter().map(|m| &m.bcs_id).collect();

            BonsaisOrGlobalrevs::Bonsai(
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
        BonsaisOrGlobalrevs::Globalrev(globalrevs) => {
            let globalrevs_fetched: HashSet<_> = mappings.iter().map(|m| &m.globalrev).collect();

            BonsaisOrGlobalrevs::Globalrev(
                globalrevs
                    .iter()
                    .filter_map(|globalrev| {
                        if !globalrevs_fetched.contains(globalrev) {
                            Some(*globalrev)
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
    connection: &Connection,
    repo_id: RepositoryId,
    objects: &BonsaisOrGlobalrevs,
) -> Result<Vec<BonsaiGlobalrevMappingEntry>, Error> {
    if objects.is_empty() {
        return Ok(vec![]);
    }

    let rows = match objects {
        BonsaisOrGlobalrevs::Bonsai(bcs_ids) => {
            SelectMappingByBonsai::query(connection, &repo_id, &bcs_ids[..]).await?
        }
        BonsaisOrGlobalrevs::Globalrev(globalrevs) => {
            SelectMappingByGlobalrev::query(connection, &repo_id, &globalrevs[..]).await?
        }
    };

    Ok(rows
        .into_iter()
        .map(move |(bcs_id, globalrev)| BonsaiGlobalrevMappingEntry { bcs_id, globalrev })
        .collect())
}

/// This method is for importing Globalrevs in bulk from a set of BonsaiChangesets where you know
/// they are correct. Don't use this to assign new Globalrevs.
pub async fn bulk_import_globalrevs<'a>(
    ctx: &'a CoreContext,
    globalrevs_store: &'a impl BonsaiGlobalrevMapping,
    changesets: impl IntoIterator<Item = &'a BonsaiChangeset>,
) -> Result<(), Error> {
    let mut entries = vec![];
    for bcs in changesets.into_iter() {
        match Globalrev::from_bcs(bcs) {
            Ok(globalrev) => {
                let entry = BonsaiGlobalrevMappingEntry::new(bcs.get_changeset_id(), globalrev);
                entries.push(entry);
            }
            Err(e) => {
                warn!(
                    ctx.logger(),
                    "Couldn't fetch globalrev from commit: {:?}", e
                );
            }
        }
    }

    globalrevs_store.bulk_import(ctx, &entries).await?;

    Ok(())
}

#[derive(Debug, Error)]
pub enum AddGlobalrevsErrorKind {
    #[error("Conflict detected while inserting Globalrevs")]
    Conflict,

    #[error("Internal error occurred while inserting Globalrevs")]
    InternalError(#[from] Error),
}

// NOTE: For now, this is a top-level function since it doesn't use the connections in the
// SqlBonsaiGlobalrevMapping, but if we were to add more implementations of the
// BonsaiGlobalrevMapping trait, we should probably rethink the design of it, and not actually have
// it contain any connections (instead, they should be passed on by callers).
pub async fn add_globalrevs(
    transaction: Transaction,
    repo_id: RepositoryId,
    entries: impl IntoIterator<Item = &BonsaiGlobalrevMappingEntry>,
) -> Result<Transaction, AddGlobalrevsErrorKind> {
    let rows: Vec<_> = entries
        .into_iter()
        .map(|BonsaiGlobalrevMappingEntry { bcs_id, globalrev }| (&repo_id, bcs_id, globalrev))
        .collect();

    // It'd be really nice if we could rely on the error from an index conflict here, but our SQL
    // crate doesn't allow us to reach into this yet, so for now we check the number of affected
    // rows.

    let (transaction, res) =
        DangerouslyAddGlobalrevs::query_with_transaction(transaction, &rows[..]).await?;

    if res.affected_rows() != rows.len() as u64 {
        return Err(AddGlobalrevsErrorKind::Conflict);
    }

    Ok(transaction)
}
