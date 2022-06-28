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
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use context::PerfCounterType;
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;
use stats::prelude::*;
use std::collections::BTreeMap;
use std::collections::HashSet;

use crate::errors::AddGitMappingErrorKind;
use crate::BonsaiGitMapping;
use crate::BonsaiGitMappingEntry;
use crate::BonsaisOrGitShas;

define_stats! {
    prefix = "mononoke.bonsai_git_mapping";
    gets: timeseries(Rate, Sum),
    gets_master: timeseries(Rate, Sum),
    adds: timeseries(Rate, Sum),
}

#[derive(Clone)]
pub struct SqlBonsaiGitMapping {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

queries! {
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
}

#[async_trait]
impl BonsaiGitMapping for SqlBonsaiGitMapping {
    async fn bulk_add(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGitMappingEntry],
    ) -> Result<(), AddGitMappingErrorKind> {
        let txn = self
            .connections
            .write_connection
            .start_transaction()
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
            select_mapping(&self.connections.read_connection, &self.repo_id, &objects).await?;
        let left_to_fetch = filter_fetched_ids(objects, &mappings[..]);

        if !left_to_fetch.is_empty() {
            STATS::gets_master.add_value(1);
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            let mut master_mappings = select_mapping(
                &self.connections.read_master_connection,
                &self.repo_id,
                &left_to_fetch,
            )
            .await?;
            mappings.append(&mut master_mappings);
        }
        Ok(mappings)
    }
}

pub struct SqlBonsaiGitMappingBuilder {
    connections: SqlConnections,
}

impl SqlBonsaiGitMappingBuilder {
    pub fn build(self, repo_id: RepositoryId) -> SqlBonsaiGitMapping {
        let SqlBonsaiGitMappingBuilder { connections } = self;
        SqlBonsaiGitMapping {
            connections,
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

impl SqlConstructFromMetadataDatabaseConfig for SqlBonsaiGitMappingBuilder {}

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
    connection: &Connection,
    repo_id: &RepositoryId,
    objects: &BonsaisOrGitShas,
) -> Result<Vec<BonsaiGitMappingEntry>> {
    if objects.is_empty() {
        return Ok(vec![]);
    }

    let rows = match objects {
        BonsaisOrGitShas::Bonsai(bcs_ids) => {
            SelectMappingByBonsai::query(connection, repo_id, &bcs_ids[..]).await?
        }
        BonsaisOrGitShas::GitSha1(git_sha1s) => {
            SelectMappingByGitSha1::query(connection, repo_id, &git_sha1s[..]).await?
        }
    };

    Ok(rows
        .into_iter()
        .map(move |(git_sha1, bcs_id)| BonsaiGitMappingEntry { bcs_id, git_sha1 })
        .collect())
}
