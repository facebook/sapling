/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use anyhow;
use ascii::AsciiStr;
use async_trait::async_trait;
use context::CoreContext;
use futures_preview::compat::Future01CompatExt;
use mononoke_types::{hash::GitSha1, BonsaiChangeset, ChangesetId, RepositoryId};
use slog::warn;
use sql::queries;
use sql::{Connection, Transaction};
use sql_ext::SqlConstructors;
use stats::prelude::*;
use std::collections::HashSet;
use std::convert::AsRef;
use std::convert::TryInto;
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
    pub repo_id: RepositoryId,
    pub git_sha1: GitSha1,
    pub bcs_id: ChangesetId,
}

impl BonsaiGitMappingEntry {
    pub fn new(repo_id: RepositoryId, git_sha1: GitSha1, bcs_id: ChangesetId) -> Self {
        BonsaiGitMappingEntry {
            repo_id,
            git_sha1,
            bcs_id,
        }
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

#[async_trait]
pub trait BonsaiGitMapping: Send + Sync {
    async fn bulk_add(
        &self,
        entries: &[BonsaiGitMappingEntry],
    ) -> Result<(), AddGitMappingErrorKind>;

    async fn get(
        &self,
        repo_id: RepositoryId,
        field: BonsaisOrGitShas,
    ) -> anyhow::Result<Vec<BonsaiGitMappingEntry>>;

    async fn get_git_sha1_from_bonsai(
        &self,
        repo_id: RepositoryId,
        bcs_id: ChangesetId,
    ) -> anyhow::Result<Option<GitSha1>> {
        let result = self
            .get(repo_id, BonsaisOrGitShas::Bonsai(vec![bcs_id]))
            .await?;
        Ok(result.into_iter().next().map(|entry| entry.git_sha1))
    }

    async fn get_bonsai_from_git_sha1(
        &self,
        repo_id: RepositoryId,
        git_sha1: GitSha1,
    ) -> anyhow::Result<Option<ChangesetId>> {
        let result = self
            .get(repo_id, BonsaisOrGitShas::GitSha1(vec![git_sha1]))
            .await?;
        Ok(result.into_iter().next().map(|entry| entry.bcs_id))
    }

    async fn bulk_import_from_bonsai(
        &self,
        ctx: CoreContext,
        repo_id: RepositoryId,
        changesets: &[BonsaiChangeset],
    ) -> anyhow::Result<()> {
        let mut entries = vec![];
        for bcs in changesets.into_iter() {
            match extract_git_sha1_from_bonsai_extra(bcs.extra()) {
                Ok(Some(git_sha1)) => {
                    let entry =
                        BonsaiGitMappingEntry::new(repo_id, git_sha1, bcs.get_changeset_id());
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
        self.bulk_add(&entries).await?;
        Ok(())
    }
}

#[async_trait]
impl BonsaiGitMapping for Arc<dyn BonsaiGitMapping> {
    async fn bulk_add(
        &self,
        entries: &[BonsaiGitMappingEntry],
    ) -> Result<(), AddGitMappingErrorKind> {
        (**self).bulk_add(entries).await
    }

    async fn get(
        &self,
        repo_id: RepositoryId,
        field: BonsaisOrGitShas,
    ) -> anyhow::Result<Vec<BonsaiGitMappingEntry>> {
        (**self).get(repo_id, field).await
    }
}

#[derive(Clone)]
pub struct SqlBonsaiGitMapping {
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

impl SqlConstructors for SqlBonsaiGitMapping {
    const LABEL: &'static str = "bonsai_git_mapping";

    fn from_connections(
        write_connection: Connection,
        read_connection: Connection,
        read_master_connection: Connection,
    ) -> Self {
        Self {
            write_connection,
            read_connection,
            read_master_connection,
        }
    }

    fn get_up_query() -> &'static str {
        include_str!("../schemas/sqlite-bonsai-git-mapping.sql")
    }
}

#[async_trait]
impl BonsaiGitMapping for SqlBonsaiGitMapping {
    async fn bulk_add(
        &self,
        entries: &[BonsaiGitMappingEntry],
    ) -> Result<(), AddGitMappingErrorKind> {
        STATS::adds.add_value(entries.len().try_into().map_err(anyhow::Error::from)?);
        let rows: Vec<_> = entries
            .into_iter()
            .map(
                |BonsaiGitMappingEntry {
                     repo_id,
                     git_sha1,
                     bcs_id,
                 }| (repo_id, git_sha1, bcs_id),
            )
            .collect();

        let res = InsertMapping::query(&self.write_connection, &rows[..])
            .compat()
            .await?;

        if res.affected_rows() != rows.len() as u64 {
            Err(AddGitMappingErrorKind::Conflict(entries.into()))?;
        }
        return Ok(());
    }

    async fn get(
        &self,
        repo_id: RepositoryId,
        objects: BonsaisOrGitShas,
    ) -> anyhow::Result<Vec<BonsaiGitMappingEntry>> {
        STATS::gets.add_value(1);
        let mut mappings = select_mapping(&self.read_connection, repo_id, &objects).await?;
        let left_to_fetch = filter_fetched_ids(objects, &mappings[..]);

        if !left_to_fetch.is_empty() {
            STATS::gets_master.add_value(1);
            let mut master_mappings =
                select_mapping(&self.read_master_connection, repo_id, &left_to_fetch).await?;
            mappings.append(&mut master_mappings);
        }
        Ok(mappings)
    }
}

/// An in-transaction version of bulk_add. Instead of using multiple connections
/// it reuses existing `Transaction`. Useful for updating the mapping atomically
/// with other changes.
///
/// It would be nice if we could use the usual SqlBonsaiGitMapping for this
/// purpose as well but a self-contained object owning connection is very
/// convenient to use and making every method take connection or transaction a
/// parameter would complicate it greatly.
pub async fn bulk_add_git_mapping_in_transaction(
    transaction: Transaction,
    entries: &[BonsaiGitMappingEntry],
) -> Result<Transaction, AddGitMappingErrorKind> {
    let rows: Vec<_> = entries
        .into_iter()
        .map(
            |BonsaiGitMappingEntry {
                 repo_id,
                 git_sha1,
                 bcs_id,
             }| (repo_id, git_sha1, bcs_id),
        )
        .collect();

    let (transaction, res) = InsertMapping::query_with_transaction(transaction, &rows[..])
        .compat()
        .await?;

    if res.affected_rows() != rows.len() as u64 {
        Err(AddGitMappingErrorKind::Conflict(entries.into()))?;
    }
    Ok(transaction)
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
    connection: &Connection,
    repo_id: RepositoryId,
    objects: &BonsaisOrGitShas,
) -> anyhow::Result<Vec<BonsaiGitMappingEntry>> {
    if objects.is_empty() {
        return Ok(vec![]);
    }

    let rows = match objects {
        BonsaisOrGitShas::Bonsai(bcs_ids) => {
            SelectMappingByBonsai::query(&connection, &repo_id, &bcs_ids[..])
                .compat()
                .await?
        }
        BonsaisOrGitShas::GitSha1(git_sha1s) => {
            SelectMappingByGitSha1::query(&connection, &repo_id, &git_sha1s[..])
                .compat()
                .await?
        }
    };

    Ok(rows
        .into_iter()
        .map(move |(git_sha1, bcs_id)| BonsaiGitMappingEntry {
            repo_id,
            bcs_id,
            git_sha1,
        })
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
