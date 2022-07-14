/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql::queries;
use ::sql::Connection;
use anyhow::Error;
use async_trait::async_trait;
use context::CoreContext;
use context::PerfCounterType;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::Svnrev;
use slog::warn;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use std::collections::HashSet;
use thiserror::Error;

use super::BonsaiSvnrevMapping;
use super::BonsaiSvnrevMappingEntry;
use super::BonsaisOrSvnrevs;

queries! {
    write DangerouslyAddSvnrevs(values: (
        repo_id: RepositoryId,
        bcs_id: ChangesetId,
        svnrev: Svnrev,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bonsai_svnrev_mapping (repo_id, bcs_id, svnrev) VALUES {values}"
    }

    read SelectMappingByBonsai(
        repo_id: RepositoryId,
        >list bcs_id: ChangesetId
    ) -> (ChangesetId, Svnrev) {
        "SELECT bcs_id, svnrev
         FROM bonsai_svnrev_mapping
         WHERE repo_id = {repo_id} AND bcs_id in {bcs_id}"
    }

    read SelectMappingBySvnrev(
        repo_id: RepositoryId,
        >list svnrev: Svnrev
    ) -> (ChangesetId, Svnrev) {
        "SELECT bcs_id, svnrev
         FROM bonsai_svnrev_mapping
         WHERE repo_id = {repo_id} AND svnrev in {svnrev}"
    }
}

#[derive(Clone)]
pub struct SqlBonsaiSvnrevMapping {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

#[derive(Clone)]
pub struct SqlBonsaiSvnrevMappingBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlBonsaiSvnrevMappingBuilder {
    const LABEL: &'static str = "bonsai_svnrev_mapping";

    const CREATION_QUERY: &'static str =
        include_str!("../schemas/sqlite-bonsai-svnrev-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlBonsaiSvnrevMappingBuilder {}

impl SqlBonsaiSvnrevMappingBuilder {
    pub fn build(self, repo_id: RepositoryId) -> SqlBonsaiSvnrevMapping {
        SqlBonsaiSvnrevMapping {
            connections: self.connections,
            repo_id,
        }
    }
}

#[async_trait]
impl BonsaiSvnrevMapping for SqlBonsaiSvnrevMapping {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn bulk_import(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiSvnrevMappingEntry],
    ) -> Result<(), Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        let entries: Vec<_> = entries
            .iter()
            .map(|entry| (&self.repo_id, &entry.bcs_id, &entry.svnrev))
            .collect();

        DangerouslyAddSvnrevs::query(&self.connections.write_connection, &entries[..]).await?;

        Ok(())
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        objects: BonsaisOrSvnrevs,
    ) -> Result<Vec<BonsaiSvnrevMappingEntry>, Error> {
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
}

fn filter_fetched_objects(
    objects: BonsaisOrSvnrevs,
    mappings: &[BonsaiSvnrevMappingEntry],
) -> BonsaisOrSvnrevs {
    match objects {
        BonsaisOrSvnrevs::Bonsai(cs_ids) => {
            let bcs_fetched: HashSet<_> = mappings.iter().map(|m| &m.bcs_id).collect();

            BonsaisOrSvnrevs::Bonsai(
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
        BonsaisOrSvnrevs::Svnrev(svnrevs) => {
            let svnrevs_fetched: HashSet<_> = mappings.iter().map(|m| &m.svnrev).collect();

            BonsaisOrSvnrevs::Svnrev(
                svnrevs
                    .iter()
                    .filter_map(|svnrev| {
                        if !svnrevs_fetched.contains(svnrev) {
                            Some(*svnrev)
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
    objects: &BonsaisOrSvnrevs,
) -> Result<Vec<BonsaiSvnrevMappingEntry>, Error> {
    if objects.is_empty() {
        return Ok(vec![]);
    }

    let rows = match objects {
        BonsaisOrSvnrevs::Bonsai(bcs_ids) => {
            SelectMappingByBonsai::query(connection, &repo_id, &bcs_ids[..]).await?
        }
        BonsaisOrSvnrevs::Svnrev(svnrevs) => {
            SelectMappingBySvnrev::query(connection, &repo_id, &svnrevs[..]).await?
        }
    };

    Ok(rows
        .into_iter()
        .map(move |(bcs_id, svnrev)| BonsaiSvnrevMappingEntry { bcs_id, svnrev })
        .collect())
}

/// This method is for importing Svnrevs in bulk from a set of BonsaiChangesets where you know
/// they are correct.
pub async fn bulk_import_svnrevs<'a>(
    ctx: &'a CoreContext,
    svnrevs_store: &'a impl BonsaiSvnrevMapping,
    changesets: impl IntoIterator<Item = &'a BonsaiChangeset>,
) -> Result<(), Error> {
    let mut entries = vec![];
    for bcs in changesets.into_iter() {
        match Svnrev::from_bcs(bcs) {
            Ok(svnrev) => {
                let entry = BonsaiSvnrevMappingEntry::new(bcs.get_changeset_id(), svnrev);
                entries.push(entry);
            }
            Err(e) => {
                warn!(ctx.logger(), "Couldn't fetch svnrev from commit: {:?}", e);
            }
        }
    }

    svnrevs_store.bulk_import(ctx, &entries).await?;

    Ok(())
}

#[derive(Debug, Error)]
pub enum AddSvnrevsErrorKind {
    #[error("Conflict detected while inserting Svnrevs")]
    Conflict,

    #[error("Internal error occurred while inserting Svnrevs")]
    InternalError(#[from] Error),
}
