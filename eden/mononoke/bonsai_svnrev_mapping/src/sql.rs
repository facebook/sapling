/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use ::sql::{queries, Connection};
use anyhow::Error;
use async_trait::async_trait;
use context::{CoreContext, PerfCounterType};
use mononoke_types::{BonsaiChangeset, ChangesetId, RepositoryId, Svnrev};
use slog::warn;
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::SqlConnections;
use std::collections::HashSet;
use thiserror::Error;

use super::{BonsaiSvnrevMapping, BonsaiSvnrevMappingEntry, BonsaisOrSvnrevs};

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
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
}

impl SqlConstruct for SqlBonsaiSvnrevMapping {
    const LABEL: &'static str = "bonsai_svnrev_mapping";

    const CREATION_QUERY: &'static str =
        include_str!("../schemas/sqlite-bonsai-svnrev-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
            read_master_connection: connections.read_master_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlBonsaiSvnrevMapping {}

#[async_trait]
impl BonsaiSvnrevMapping for SqlBonsaiSvnrevMapping {
    async fn bulk_import(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiSvnrevMappingEntry],
    ) -> Result<(), Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        let entries: Vec<_> = entries
            .iter()
            .map(
                |
                    BonsaiSvnrevMappingEntry {
                        repo_id,
                        bcs_id,
                        svnrev,
                    },
                | (repo_id, bcs_id, svnrev),
            )
            .collect();

        DangerouslyAddSvnrevs::query(&self.write_connection, &entries[..]).await?;

        Ok(())
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        repo_id: RepositoryId,
        objects: BonsaisOrSvnrevs,
    ) -> Result<Vec<BonsaiSvnrevMappingEntry>, Error> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let mut mappings = select_mapping(&self.read_connection, repo_id, &objects).await?;

        let left_to_fetch = filter_fetched_objects(objects, &mappings[..]);

        if left_to_fetch.is_empty() {
            return Ok(mappings);
        }

        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsMaster);

        let mut master_mappings =
            select_mapping(&self.read_master_connection, repo_id, &left_to_fetch).await?;
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
            SelectMappingByBonsai::query(&connection, &repo_id, &bcs_ids[..]).await?
        }
        BonsaisOrSvnrevs::Svnrev(svnrevs) => {
            SelectMappingBySvnrev::query(&connection, &repo_id, &svnrevs[..]).await?
        }
    };


    Ok(rows
        .into_iter()
        .map(move |(bcs_id, svnrev)| BonsaiSvnrevMappingEntry {
            repo_id,
            bcs_id,
            svnrev,
        })
        .collect())
}

/// This method is for importing Svnrevs in bulk from a set of BonsaiChangesets where you know
/// they are correct.
pub async fn bulk_import_svnrevs<'a>(
    ctx: &'a CoreContext,
    repo_id: RepositoryId,
    svnrevs_store: &'a impl BonsaiSvnrevMapping,
    changesets: impl IntoIterator<Item = &'a BonsaiChangeset>,
) -> Result<(), Error> {
    let mut entries = vec![];
    for bcs in changesets.into_iter() {
        match Svnrev::from_bcs(bcs) {
            Ok(svnrev) => {
                let entry = BonsaiSvnrevMappingEntry::new(repo_id, bcs.get_changeset_id(), svnrev);
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
