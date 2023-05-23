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
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::SqlConnections;

use super::BonsaiTagMapping;
use super::BonsaiTagMappingEntry;

mononoke_queries! {
    write AddBonsaiTagMapping(values: (
        repo_id: RepositoryId,
        tag_name: String,
        changeset_id: ChangesetId,
    )) {
        insert_or_ignore,
        "{insert_or_ignore} INTO bonsai_tag_mapping (repo_id, tag_name, changeset_id) VALUES {values}"
    }

    read SelectMappingByChangeset(
        repo_id: RepositoryId,
        changeset_id: ChangesetId
    ) -> (String, ChangesetId) {
        "SELECT tag_name, changeset_id
         FROM bonsai_tag_mapping
         WHERE repo_id = {repo_id} AND changeset_id = {changeset_id}"
    }

    read SelectMappingByTagName(
        repo_id: RepositoryId,
        tag_name: String,
    ) -> (String, ChangesetId) {
        "SELECT tag_name, changeset_id
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

    async fn get_changeset_by_tag_name(&self, tag_name: String) -> Result<Option<ChangesetId>> {
        let results = SelectMappingByTagName::query(
            &self.connections.read_connection,
            &self.repo_id,
            &tag_name,
        )
        .await
        .with_context(|| {
            format!(
                "Failure in fetching changeset for tag {} in repo {}",
                tag_name, self.repo_id
            )
        })?;
        // This should not happen but since this is new code, extra checks dont hurt.
        if results.len() > 1 {
            anyhow::bail!(
                "Multiple changesets returned for tag {} in repo {}",
                tag_name,
                self.repo_id
            )
        }
        Ok(results
            .into_iter()
            .next()
            .map(|(_, changeset_id)| changeset_id))
    }

    async fn get_tag_names_by_changeset(
        &self,
        changeset_id: ChangesetId,
    ) -> Result<Option<Vec<String>>> {
        let results = SelectMappingByChangeset::query(
            &self.connections.read_connection,
            &self.repo_id,
            &changeset_id,
        )
        .await
        .with_context(|| {
            format!(
                "Failure in fetching tag for changeset {:?} in repo {}",
                changeset_id, self.repo_id
            )
        })?;

        let values = results
            .into_iter()
            .map(|(tag_name, _)| tag_name)
            .collect::<Vec<_>>();
        let output = (!values.is_empty()).then_some(values);
        return Ok(output);
    }

    async fn add_mappings(&self, entries: Vec<BonsaiTagMappingEntry>) -> Result<()> {
        let entries: Vec<_> = entries
            .iter()
            .map(|entry| (&self.repo_id, &entry.tag_name, &entry.changeset_id))
            .collect();
        AddBonsaiTagMapping::query(&self.connections.write_connection, entries.as_slice())
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
