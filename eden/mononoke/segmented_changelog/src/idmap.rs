/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Result};
use futures::compat::Future01CompatExt;
use sql::queries;
use sql_ext::SqlConnections;

use dag::Id as Vertex;

use mononoke_types::{ChangesetId, RepositoryId};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};

#[derive(Clone)]
pub struct IdMap(SqlConnections);

queries! {
    // TODO(sfilip): upsert is a hack around a limited build_up implementation, we want insert_or_ignore.
    write InsertIdMapEntry(values: (repo_id: RepositoryId, vertex: u64, cs_id: ChangesetId)) {
        none,
        "
        INSERT OR REPLACE INTO segmented_changelog_idmap (repo_id, vertex, cs_id)
        VALUES {values}
        "
    }

    read SelectChangesetId(repo_id: RepositoryId, vertex: u64) -> (ChangesetId) {
        "
        SELECT idmap.cs_id as cs_id
        FROM segmented_changelog_idmap AS idmap
        WHERE idmap.repo_id = {repo_id} AND idmap.vertex = {vertex}
        "
    }

    read SelectVertex(repo_id: RepositoryId, cs_id: ChangesetId) -> (u64) {
        "
        SELECT idmap.vertex as vertex
        FROM segmented_changelog_idmap AS idmap
        WHERE idmap.repo_id = {repo_id} AND idmap.cs_id = {cs_id}
        "
    }

}

impl SqlConstruct for IdMap {
    const LABEL: &'static str = "segmented_changelog_idmap";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-segmented-changelog.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self(connections)
    }
}

impl SqlConstructFromMetadataDatabaseConfig for IdMap {}

impl IdMap {
    pub async fn insert(
        &self,
        repo_id: RepositoryId,
        vertex: Vertex,
        cs_id: ChangesetId,
    ) -> Result<()> {
        // TODO(sfilip): add tests
        let result =
            InsertIdMapEntry::query(&self.0.write_connection, &[(&repo_id, &vertex.0, &cs_id)])
                .compat()
                .await?;
        if result.affected_rows() != 1 {
            let stored =
                SelectChangesetId::query(&self.0.read_master_connection, &repo_id, &vertex.0)
                    .compat()
                    .await?;
            match stored.as_slice() {
                &[] => {
                    return Err(format_err!(
                        "Failed to insert entry ({} -> {}) in Idmap",
                        vertex,
                        cs_id
                    ))
                }
                &[(store_cs_id,)] => {
                    if store_cs_id != cs_id {
                        return Err(format_err!(
                            "Duplicate segmented changelog idmap entry {} \
                                has different assignments: {} vs {}",
                            vertex,
                            cs_id,
                            store_cs_id
                        ));
                    }
                }
                _ => {
                    return Err(format_err!(
                        "Duplicate segmented changelog idmap entries: {:?}",
                        stored
                    ))
                }
            };
        }
        Ok(())
    }

    pub async fn find_changeset_id(
        &self,
        repo_id: RepositoryId,
        vertex: Vertex,
    ) -> Result<Option<ChangesetId>> {
        let select = |connection| async move {
            let rows = SelectChangesetId::query(connection, &repo_id, &vertex.0)
                .compat()
                .await?;
            Ok(rows.into_iter().next().map(|r| r.0))
        };
        match select(&self.0.read_connection).await? {
            None => select(&self.0.read_master_connection).await,
            Some(cs_id) => Ok(Some(cs_id)),
        }
    }

    pub async fn get_changeset_id(
        &self,
        repo_id: RepositoryId,
        vertex: Vertex,
    ) -> Result<ChangesetId> {
        self.find_changeset_id(repo_id, vertex)
            .await?
            .ok_or_else(|| format_err!("Failed to find segmented changelog id {} in IdMap", vertex))
    }

    pub async fn find_vertex(
        &self,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> Result<Option<Vertex>> {
        let select = |connection| async move {
            let rows = SelectVertex::query(connection, &repo_id, &cs_id)
                .compat()
                .await?;
            Ok(rows.into_iter().next().map(|r| Vertex(r.0)))
        };
        match select(&self.0.read_connection).await? {
            None => select(&self.0.read_master_connection).await,
            Some(v) => Ok(Some(v)),
        }
    }

    pub async fn get_vertex(&self, repo_id: RepositoryId, cs_id: ChangesetId) -> Result<Vertex> {
        self.find_vertex(repo_id, cs_id)
            .await?
            .ok_or_else(|| format_err!("Failed to find find changeset id {} in IdMap", cs_id))
    }
}
