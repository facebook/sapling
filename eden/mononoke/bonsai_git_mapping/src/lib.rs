/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow;
use ascii::AsciiStr;
use async_trait::async_trait;
use context::{CoreContext, PerfCounterType};
use mononoke_types::{hash::GitSha1, BonsaiChangeset, ChangesetId, RepositoryId};
use slog::warn;
use sql::queries;
use sql::{Connection, Transaction};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::SqlConnections;
use stats::prelude::*;
use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

mod errors;
pub use crate::errors::AddGitMappingErrorKind;

define_stats! {
    prefix = "mononoke.bonsai_git_mapping";
    gets: timeseries(Rate, Sum),
    gets_master: timeseries(Rate, Sum),
    adds: timeseries(Rate, Sum),
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BonsaiGitMappingEntry {
    pub git_sha1: GitSha1,
    pub bcs_id: ChangesetId,
}

impl BonsaiGitMappingEntry {
    pub fn new(git_sha1: GitSha1, bcs_id: ChangesetId) -> Self {
        BonsaiGitMappingEntry { git_sha1, bcs_id }
    }
}

pub enum BonsaisOrGitShas {
    Bonsai(Vec<ChangesetId>),
    GitSha1(Vec<GitSha1>),
}

impl BonsaisOrGitShas {
    pub fn is_empty(&self) -> bool {
        match self {
            BonsaisOrGitShas::Bonsai(v) => v.is_empty(),
            BonsaisOrGitShas::GitSha1(v) => v.is_empty(),
        }
    }
}

impl From<ChangesetId> for BonsaisOrGitShas {
    fn from(cs_id: ChangesetId) -> Self {
        BonsaisOrGitShas::Bonsai(vec![cs_id])
    }
}

impl From<Vec<ChangesetId>> for BonsaisOrGitShas {
    fn from(cs_ids: Vec<ChangesetId>) -> Self {
        BonsaisOrGitShas::Bonsai(cs_ids)
    }
}

impl From<GitSha1> for BonsaisOrGitShas {
    fn from(git_sha1: GitSha1) -> Self {
        BonsaisOrGitShas::GitSha1(vec![git_sha1])
    }
}

impl From<Vec<GitSha1>> for BonsaisOrGitShas {
    fn from(revs: Vec<GitSha1>) -> Self {
        BonsaisOrGitShas::GitSha1(revs)
    }
}

#[facet::facet]
#[async_trait]
pub trait BonsaiGitMapping: Send + Sync {
    async fn bulk_add(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGitMappingEntry],
    ) -> Result<(), AddGitMappingErrorKind>;

    async fn bulk_add_git_mapping_in_transaction(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGitMappingEntry],
        transaction: Transaction,
    ) -> Result<Transaction, AddGitMappingErrorKind>;

    async fn get(
        &self,
        ctx: &CoreContext,
        field: BonsaisOrGitShas,
    ) -> anyhow::Result<Vec<BonsaiGitMappingEntry>>;

    async fn get_git_sha1_from_bonsai(
        &self,
        ctx: &CoreContext,
        bcs_id: ChangesetId,
    ) -> anyhow::Result<Option<GitSha1>> {
        let result = self
            .get(ctx, BonsaisOrGitShas::Bonsai(vec![bcs_id]))
            .await?;
        Ok(result.into_iter().next().map(|entry| entry.git_sha1))
    }

    async fn get_bonsai_from_git_sha1(
        &self,
        ctx: &CoreContext,
        git_sha1: GitSha1,
    ) -> anyhow::Result<Option<ChangesetId>> {
        let result = self
            .get(ctx, BonsaisOrGitShas::GitSha1(vec![git_sha1]))
            .await?;
        Ok(result.into_iter().next().map(|entry| entry.bcs_id))
    }

    async fn bulk_import_from_bonsai(
        &self,
        ctx: &CoreContext,
        changesets: &[BonsaiChangeset],
    ) -> anyhow::Result<()> {
        let mut entries = vec![];
        for bcs in changesets.into_iter() {
            match extract_git_sha1_from_bonsai_extra(bcs.extra()) {
                Ok(Some(git_sha1)) => {
                    let entry = BonsaiGitMappingEntry::new(git_sha1, bcs.get_changeset_id());
                    entries.push(entry);
                }
                Ok(None) => {
                    warn!(
                        ctx.logger(),
                        "The git mapping is missing in bonsai commit extras: {:?}",
                        bcs.get_changeset_id()
                    );
                }
                Err(e) => {
                    warn!(ctx.logger(), "Couldn't fetch git mapping: {:?}", e);
                }
            }
        }
        self.bulk_add(ctx, &entries).await?;
        Ok(())
    }
}

#[async_trait]
impl BonsaiGitMapping for Arc<dyn BonsaiGitMapping> {
    async fn bulk_add(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGitMappingEntry],
    ) -> Result<(), AddGitMappingErrorKind> {
        (**self).bulk_add(ctx, entries).await
    }

    async fn bulk_add_git_mapping_in_transaction(
        &self,
        ctx: &CoreContext,
        entries: &[BonsaiGitMappingEntry],
        transaction: Transaction,
    ) -> Result<Transaction, AddGitMappingErrorKind> {
        (**self)
            .bulk_add_git_mapping_in_transaction(ctx, entries, transaction)
            .await
    }

    async fn get(
        &self,
        ctx: &CoreContext,
        field: BonsaisOrGitShas,
    ) -> anyhow::Result<Vec<BonsaiGitMappingEntry>> {
        (**self).get(ctx, field).await
    }
}

#[derive(Clone)]
pub struct SqlBonsaiGitMapping {
    repo_id: RepositoryId,
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
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
        let txn = self.write_connection.start_transaction().await?;
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
        STATS::adds.add_value(entries.len().try_into().map_err(anyhow::Error::from)?);
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
    ) -> anyhow::Result<Vec<BonsaiGitMappingEntry>> {
        STATS::gets.add_value(1);
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);

        let mut mappings = select_mapping(&self.read_connection, &self.repo_id, &objects).await?;
        let left_to_fetch = filter_fetched_ids(objects, &mappings[..]);

        if !left_to_fetch.is_empty() {
            STATS::gets_master.add_value(1);
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            let mut master_mappings =
                select_mapping(&self.read_master_connection, &self.repo_id, &left_to_fetch).await?;
            mappings.append(&mut master_mappings);
        }
        Ok(mappings)
    }
}

pub struct SqlBonsaiGitMappingConnection {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
}

impl SqlBonsaiGitMappingConnection {
    pub fn with_repo_id(self, repo_id: RepositoryId) -> SqlBonsaiGitMapping {
        let SqlBonsaiGitMappingConnection {
            write_connection,
            read_connection,
            read_master_connection,
        } = self;
        SqlBonsaiGitMapping {
            repo_id,
            write_connection,
            read_connection,
            read_master_connection,
        }
    }
}

impl SqlConstruct for SqlBonsaiGitMappingConnection {
    const LABEL: &'static str = "bonsai_git_mapping";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-bonsai-git-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
            read_master_connection: connections.read_master_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlBonsaiGitMappingConnection {}

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
) -> anyhow::Result<Vec<BonsaiGitMappingEntry>> {
    if objects.is_empty() {
        return Ok(vec![]);
    }

    let rows = match objects {
        BonsaisOrGitShas::Bonsai(bcs_ids) => {
            SelectMappingByBonsai::query(&connection, repo_id, &bcs_ids[..]).await?
        }
        BonsaisOrGitShas::GitSha1(git_sha1s) => {
            SelectMappingByGitSha1::query(&connection, repo_id, &git_sha1s[..]).await?
        }
    };

    Ok(rows
        .into_iter()
        .map(move |(git_sha1, bcs_id)| BonsaiGitMappingEntry { bcs_id, git_sha1 })
        .collect())
}

pub const HGGIT_SOURCE_EXTRA: &str = "hg-git-rename-source";
pub const CONVERT_REVISION_EXTRA: &str = "convert_revision";
pub fn extract_git_sha1_from_bonsai_extra<'a, 'b, T>(extra: T) -> anyhow::Result<Option<GitSha1>>
where
    T: Iterator<Item = (&'a str, &'b [u8])>,
{
    let (mut hggit_source_extra, mut convert_revision_extra) = (None, None);
    for (key, value) in extra {
        if key == HGGIT_SOURCE_EXTRA {
            hggit_source_extra = Some(value);
        }
        if key == CONVERT_REVISION_EXTRA {
            convert_revision_extra = Some(value);
        }
    }

    if hggit_source_extra == Some(b"git") {
        if let Some(convert_revision_extra) = convert_revision_extra {
            let git_sha1 = AsciiStr::from_ascii(convert_revision_extra)?;
            let git_sha1 = GitSha1::from_ascii_str(git_sha1)?;
            return Ok(Some(git_sha1));
        }
    }
    Ok(None)
}
