/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use ::sql_ext::Connection;
use ::sql_ext::mononoke_queries;
use anyhow::Error;
use async_trait::async_trait;
use context::CoreContext;
use context::PerfCounterType;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::ChangesetId;
use mononoke_types::P4ChangelistId;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;

use super::BonsaiP4Mapping;
use super::BonsaiP4MappingEntry;
use super::BonsaisOrP4ChangelistIds;

mononoke_queries! {
    write DangerouslyAddP4ChangelistIds(values: (
        repo_id: RepositoryId,
        bcs_id: ChangesetId,
        p4_changelist_id: P4ChangelistId,
    )) {
        none,
        "INSERT INTO bonsai_p4_mapping (repo_id, bcs_id, p4_changelist_id) VALUES {values}"
    }

    read SelectMappingByBonsai(
        repo_id: RepositoryId,
        >list bcs_id: ChangesetId
    ) -> (ChangesetId, P4ChangelistId) {
        "SELECT bcs_id, p4_changelist_id
         FROM bonsai_p4_mapping
         WHERE repo_id = {repo_id} AND bcs_id in {bcs_id}"
    }

    read SelectMappingByP4ChangelistId(
        repo_id: RepositoryId,
        >list p4_changelist_id: P4ChangelistId
    ) -> (ChangesetId, P4ChangelistId) {
        "SELECT bcs_id, p4_changelist_id
         FROM bonsai_p4_mapping
         WHERE repo_id = {repo_id} AND p4_changelist_id in {p4_changelist_id}"
    }
}

pub struct SqlBonsaiP4Mapping {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

#[derive(Clone)]
pub struct SqlBonsaiP4MappingBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlBonsaiP4MappingBuilder {
    const LABEL: &'static str = "bonsai_p4_mapping";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-bonsai-p4-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlBonsaiP4MappingBuilder {
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

impl SqlBonsaiP4MappingBuilder {
    pub fn build(self, repo_id: RepositoryId) -> SqlBonsaiP4Mapping {
        SqlBonsaiP4Mapping {
            connections: self.connections,
            repo_id,
        }
    }
}

#[async_trait]
impl BonsaiP4Mapping for SqlBonsaiP4Mapping {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn bulk_import(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiP4MappingEntry],
    ) -> Result<(), Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        let entries: Vec<_> = entries
            .iter()
            .map(|entry| (&self.repo_id, &entry.bcs_id, &entry.p4_changelist_id))
            .collect();

        DangerouslyAddP4ChangelistIds::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &entries[..],
        )
        .await?;

        Ok(())
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        objects: BonsaisOrP4ChangelistIds,
    ) -> Result<Vec<BonsaiP4MappingEntry>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let mut mappings = select_mapping(
            ctx,
            &self.connections.read_connection,
            self.repo_id,
            &objects,
        )
        .await?;

        let left_to_fetch = filter_fetched_objects(objects, &mappings[..]);

        if left_to_fetch.is_empty() {
            return Ok(mappings);
        }

        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);

        let mut master_mappings = select_mapping(
            ctx,
            &self.connections.read_master_connection,
            self.repo_id,
            &left_to_fetch,
        )
        .await?;
        mappings.append(&mut master_mappings);
        Ok(mappings)
    }
}

fn filter_fetched_objects(
    objects: BonsaisOrP4ChangelistIds,
    mappings: &[BonsaiP4MappingEntry],
) -> BonsaisOrP4ChangelistIds {
    match objects {
        BonsaisOrP4ChangelistIds::Bonsai(cs_ids) => {
            let bcs_fetched: HashSet<_> = mappings.iter().map(|m| &m.bcs_id).collect();

            BonsaisOrP4ChangelistIds::Bonsai(
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
        BonsaisOrP4ChangelistIds::P4ChangelistId(cl_ids) => {
            let cl_ids_fetched: HashSet<_> = mappings.iter().map(|m| &m.p4_changelist_id).collect();

            BonsaisOrP4ChangelistIds::P4ChangelistId(
                cl_ids
                    .iter()
                    .filter_map(|cl_id| {
                        if !cl_ids_fetched.contains(cl_id) {
                            Some(*cl_id)
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
    connection: &Connection,
    repo_id: RepositoryId,
    objects: &BonsaisOrP4ChangelistIds,
) -> Result<Vec<BonsaiP4MappingEntry>, Error> {
    if objects.is_empty() {
        return Ok(vec![]);
    }

    let rows = match objects {
        BonsaisOrP4ChangelistIds::Bonsai(bcs_ids) => {
            SelectMappingByBonsai::query(
                connection,
                ctx.sql_query_telemetry(),
                &repo_id,
                &bcs_ids[..],
            )
            .await?
        }
        BonsaisOrP4ChangelistIds::P4ChangelistId(cl_ids) => {
            SelectMappingByP4ChangelistId::query(
                connection,
                ctx.sql_query_telemetry(),
                &repo_id,
                &cl_ids[..],
            )
            .await?
        }
    };

    Ok(rows
        .into_iter()
        .map(move |(bcs_id, p4_changelist_id)| BonsaiP4MappingEntry {
            bcs_id,
            p4_changelist_id,
        })
        .collect())
}
