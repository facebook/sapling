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
use mononoke_types::hash::GitSha1;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;

use super::BonsaiTagMapping;
use super::BonsaiTagMappingEntry;

mononoke_queries! {
    write AddOrUpdateBonsaiTagMapping(values: (
        repo_id: RepositoryId,
        tag_name: String,
        changeset_id: ChangesetId,
        tag_hash: GitSha1,
    )) {
        none,
        "REPLACE INTO bonsai_tag_mapping (repo_id, tag_name, changeset_id, tag_hash) VALUES {values}"
    }

    read SelectMappingByChangeset(
        repo_id: RepositoryId,
        changeset_id: ChangesetId
    ) -> (String, ChangesetId, GitSha1) {
        "SELECT tag_name, changeset_id, tag_hash
         FROM bonsai_tag_mapping
         WHERE repo_id = {repo_id} AND changeset_id = {changeset_id}"
    }

    read SelectMappingByTagName(
        repo_id: RepositoryId,
        tag_name: String,
    ) -> (String, ChangesetId, GitSha1) {
        "SELECT tag_name, changeset_id, tag_hash
         FROM bonsai_tag_mapping
         WHERE repo_id = {repo_id} AND tag_name = {tag_name}"
    }
}

pub struct SqlBonsaiTagMapping {
    connections: SqlConnections,
    repo_id: RepositoryId,
}

#[derive(Clone)]
pub struct SqlBonsaiTagMappingBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlBonsaiTagMappingBuilder {
    const LABEL: &'static str = "bonsai_tag_mapping";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-bonsai-tag-mapping.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlBonsaiTagMappingBuilder {}

impl SqlBonsaiTagMappingBuilder {
    pub fn build(self, repo_id: RepositoryId) -> SqlBonsaiTagMapping {
        SqlBonsaiTagMapping {
            connections: self.connections,
            repo_id,
        }
    }
}

#[async_trait]
impl BonsaiTagMapping for SqlBonsaiTagMapping {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn get_entry_by_tag_name(
        &self,
        tag_name: String,
    ) -> Result<Option<BonsaiTagMappingEntry>> {
        let results = SelectMappingByTagName::query(
            &self.connections.read_connection,
            &self.repo_id,
            &tag_name,
        )
        .await
        .with_context(|| {
            format!(
                "Failure in fetching entry for tag {} in repo {}",
                tag_name, self.repo_id
            )
        })?;
        // This should not happen but since this is new code, extra checks dont hurt.
        if results.len() > 1 {
            anyhow::bail!(
                "Multiple entries returned for tag {} in repo {}",
                tag_name,
                self.repo_id
            )
        }
        Ok(results
            .into_iter()
            .next()
            .map(|(tag_name, changeset_id, tag_hash)| {
                BonsaiTagMappingEntry::new(changeset_id, tag_name, tag_hash)
            }))
    }

    async fn get_entries_by_changeset(
        &self,
        changeset_id: ChangesetId,
    ) -> Result<Option<Vec<BonsaiTagMappingEntry>>> {
        let results = SelectMappingByChangeset::query(
            &self.connections.read_connection,
            &self.repo_id,
            &changeset_id,
        )
        .await
        .with_context(|| {
            format!(
                "Failure in fetching entry for changeset {:?} in repo {}",
                changeset_id, self.repo_id
            )
        })?;

        let values = results
            .into_iter()
            .map(|(tag_name, changeset_id, tag_hash)| {
                BonsaiTagMappingEntry::new(changeset_id, tag_name, tag_hash)
            })
            .collect::<Vec<_>>();
        let output = (!values.is_empty()).then_some(values);
        return Ok(output);
    }

    async fn add_or_update_mappings(&self, entries: Vec<BonsaiTagMappingEntry>) -> Result<()> {
        let converted_entries: Vec<_> = entries
            .iter()
            .map(|entry| {
                (
                    &self.repo_id,
                    &entry.tag_name,
                    &entry.changeset_id,
                    &entry.tag_hash,
                )
            })
            .collect();
        AddOrUpdateBonsaiTagMapping::query(
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
