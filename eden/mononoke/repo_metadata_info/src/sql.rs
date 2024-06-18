/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use async_trait::async_trait;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;

use super::RepoMetadataInfo;
use super::RepoMetadataInfoEntry;

pub struct SqlRepoMetadataInfo {
    _connections: SqlConnections,
    repo_id: RepositoryId,
}

#[derive(Clone)]
pub struct SqlRepoMetadataInfoBuilder {
    _connections: SqlConnections,
}

impl SqlConstruct for SqlRepoMetadataInfoBuilder {
    const LABEL: &'static str = "repo_metadata_info";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-repo-metadata-info.sql");

    fn from_sql_connections(_connections: SqlConnections) -> Self {
        Self { _connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlRepoMetadataInfoBuilder {}

impl SqlRepoMetadataInfoBuilder {
    pub fn build(self, repo_id: RepositoryId) -> SqlRepoMetadataInfo {
        SqlRepoMetadataInfo {
            _connections: self._connections,
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
        Ok(vec![])
    }

    /// Fetch the repo metadata entries corresponding to the input bookmark name
    /// for the given repo
    async fn get_entry(&self, _bookmark_name: String) -> Result<Option<RepoMetadataInfoEntry>> {
        Ok(None)
    }

    /// Add new or update existing repo metadata entries for the given repo
    async fn add_or_update_entries(&self, _entries: Vec<RepoMetadataInfoEntry>) -> Result<()> {
        Ok(())
    }
}
