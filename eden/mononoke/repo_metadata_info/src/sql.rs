/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::Timestamp;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::mononoke_queries;
use sql_ext::SqlConnections;

use super::RepoMetadataInfo;
use super::RepoMetadataInfoEntry;

mononoke_queries! {
    write AddOrUpdateRepoMetadataInfo(values: (
        repo_id: RepositoryId,
        bookmark_name: String,
        changeset_id: ChangesetId,
        last_updated_timestamp: Timestamp,
    )) {
        none,
        "REPLACE INTO repo_metadata_info (repo_id, bookmark_name, changeset_id, last_updated_timestamp) VALUES {values}"
    }

    read SelectAllEntries(
        repo_id: RepositoryId,
    ) -> (String, ChangesetId, Timestamp) {
        "SELECT bookmark_name, changeset_id, last_updated_timestamp
         FROM repo_metadata_info
         WHERE repo_id = {repo_id}"
    }

    read SelectEntryByBookmark(
        repo_id: RepositoryId,
        bookmark_name: String,
    ) -> (String, ChangesetId, Timestamp) {
        "SELECT bookmark_name, changeset_id, last_updated_timestamp
         FROM repo_metadata_info
         WHERE repo_id = {repo_id} AND bookmark_name = {bookmark_name}"
    }
}

pub struct SqlRepoMetadataInfo {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

#[derive(Clone)]
pub struct SqlRepoMetadataInfoBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlRepoMetadataInfoBuilder {
    const LABEL: &'static str = "repo_metadata_info";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-repo-metadata-info.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlRepoMetadataInfoBuilder {}

impl SqlRepoMetadataInfoBuilder {
    pub fn build(self, repo_id: RepositoryId) -> SqlRepoMetadataInfo {
        SqlRepoMetadataInfo {
            connections: self.connections,
            repo_id,
        }
    }
}

#[async_trait]
impl RepoMetadataInfo for SqlRepoMetadataInfo {
    /// The repository for which this entry has been created
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    /// Fetch all the metadata info entries for the given repo
    async fn get_all_entries(&self) -> Result<Vec<RepoMetadataInfoEntry>> {
        let results = SelectAllEntries::query(&self.connections.read_connection, &self.repo_id)
            .await
            .with_context(|| {
                format!("Failure in fetching all entries for repo {}", self.repo_id)
            })?;

        let values = results
            .into_iter()
            .map(|(bookmark_name, changeset_id, last_updated_timestamp)| {
                RepoMetadataInfoEntry::new(changeset_id, bookmark_name, last_updated_timestamp)
            })
            .collect::<Vec<_>>();
        return Ok(values);
    }

    /// Fetch the repo metadata entries corresponding to the input bookmark name
    /// for the given repo
    async fn get_entry(&self, bookmark_name: String) -> Result<Option<RepoMetadataInfoEntry>> {
        let results = SelectEntryByBookmark::query(
            &self.connections.read_connection,
            &self.repo_id,
            &bookmark_name,
        )
        .await
        .with_context(|| {
            format!(
                "Failure in fetching entry for repo {} and bookmark {}",
                self.repo_id, bookmark_name
            )
        })?;
        // This should not happen but since this is new code, extra checks dont hurt.
        if results.len() > 1 {
            anyhow::bail!(
                "Multiple entries returned for bookmark {} in repo {}",
                bookmark_name,
                self.repo_id
            )
        }
        Ok(results.into_iter().next().map(
            |(bookmark_name, changeset_id, last_updated_timestamp)| {
                RepoMetadataInfoEntry::new(changeset_id, bookmark_name, last_updated_timestamp)
            },
        ))
    }

    /// Add new or update existing repo metadata entries for the given repo
    async fn add_or_update_entries(&self, entries: Vec<RepoMetadataInfoEntry>) -> Result<()> {
        let converted_entries: Vec<_> = entries
            .iter()
            .map(|entry| {
                (
                    &self.repo_id,
                    &entry.bookmark_name,
                    &entry.changeset_id,
                    &entry.last_updated_timestamp,
                )
            })
            .collect();
        AddOrUpdateRepoMetadataInfo::query(
            &self.connections.write_connection,
            converted_entries.as_slice(),
        )
        .await
        .with_context(|| {
            format!(
                "Failed to add mappings in repo {} for entries {:?}",
                self.repo_id, entries,
            )
        })?;
        Ok(())
    }
}
