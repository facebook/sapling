/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ::sql_ext::mononoke_queries;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use context::CoreContext;
use metaconfig_types::OssRemoteDatabaseConfig;
use metaconfig_types::OssRemoteMetadataDatabaseConfig;
use metaconfig_types::RemoteDatabaseConfig;
use metaconfig_types::RemoteMetadataDatabaseConfig;
use mononoke_types::RepositoryId;
use mononoke_types::hash::GitSha1;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;

use super::GitRefContentMapping;
use super::GitRefContentMappingEntry;

mononoke_queries! {
    write AddOrUpdateGitRefContentMapping(values: (
        repo_id: RepositoryId,
        ref_name: String,
        git_hash: GitSha1,
        is_tree: bool,
    )) {
        none,
        "REPLACE INTO git_ref_content_mapping (repo_id, ref_name, git_hash, is_tree) VALUES {values}"
    }

    write DeleteGitRefContentMappingsByName(repo_id: RepositoryId,
        >list ref_names: String) {
        none,
        "DELETE FROM git_ref_content_mapping WHERE repo_id = {repo_id} AND ref_name IN {ref_names}"
    }

    read SelectAllMappings(
        repo_id: RepositoryId,
    ) -> (String, GitSha1, bool) {
        "SELECT ref_name, git_hash, is_tree
          FROM git_ref_content_mapping
          WHERE repo_id = {repo_id}"
    }

    read SelectMappingByRefName(
        repo_id: RepositoryId,
        ref_name: String,
    ) -> (String, GitSha1, bool) {
        "SELECT ref_name, git_hash, is_tree
          FROM git_ref_content_mapping
          WHERE repo_id = {repo_id} AND ref_name = {ref_name}"
    }
}

pub struct SqlGitRefContentMapping {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

#[derive(Clone)]
pub struct SqlGitRefContentMappingBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlGitRefContentMappingBuilder {
    const LABEL: &'static str = "git_ref_content_mapping";

    const CREATION_QUERY: &'static str =
        include_str!("../schemas/sqlite-git-ref-content-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlGitRefContentMappingBuilder {
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

impl SqlGitRefContentMappingBuilder {
    pub fn build(self, repo_id: RepositoryId) -> SqlGitRefContentMapping {
        SqlGitRefContentMapping {
            connections: self.connections,
            repo_id,
        }
    }
}

#[async_trait]
impl GitRefContentMapping for SqlGitRefContentMapping {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn get_all_entries(&self, ctx: &CoreContext) -> Result<Vec<GitRefContentMappingEntry>> {
        let results = SelectAllMappings::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            &self.repo_id,
        )
        .await
        .with_context(|| format!("Failure in fetching all entries for repo {}", self.repo_id))?;

        let values = results
            .into_iter()
            .map(|(ref_name, git_hash, is_tree)| {
                GitRefContentMappingEntry::new(ref_name, git_hash, is_tree)
            })
            .collect::<Vec<_>>();
        return Ok(values);
    }

    async fn get_entry_by_ref_name(
        &self,
        ctx: &CoreContext,
        ref_name: String,
    ) -> Result<Option<GitRefContentMappingEntry>> {
        let results = SelectMappingByRefName::query(
            &self.connections.read_connection,
            ctx.sql_query_telemetry(),
            &self.repo_id,
            &ref_name,
        )
        .await
        .with_context(|| {
            format!(
                "Failure in fetching entry for ref {} in repo {}",
                ref_name, self.repo_id
            )
        })?;
        // This should not happen because we're selecting by primary key.
        if results.len() > 1 {
            anyhow::bail!(
                "Multiple entries returned for ref {} in repo {}",
                ref_name,
                self.repo_id
            )
        }
        Ok(results
            .into_iter()
            .next()
            .map(|(ref_name, git_hash, is_tree)| {
                GitRefContentMappingEntry::new(ref_name, git_hash, is_tree)
            }))
    }

    async fn add_or_update_mappings(
        &self,
        ctx: &CoreContext,
        entries: Vec<GitRefContentMappingEntry>,
    ) -> Result<()> {
        let converted_entries: Vec<_> = entries
            .iter()
            .map(|entry| {
                (
                    &self.repo_id,
                    &entry.ref_name,
                    &entry.git_hash,
                    &entry.is_tree,
                )
            })
            .collect();
        AddOrUpdateGitRefContentMapping::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
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

    async fn delete_mappings_by_name(
        &self,
        ctx: &CoreContext,
        ref_names: Vec<String>,
    ) -> Result<()> {
        DeleteGitRefContentMappingsByName::query(
            &self.connections.write_connection,
            ctx.sql_query_telemetry(),
            &self.repo_id,
            ref_names.as_slice(),
        )
        .await
        .with_context(|| {
            format!(
                "Failed to delete mappings in repo {} for ref names {:?}",
                self.repo_id, ref_names,
            )
        })?;
        Ok(())
    }
}
