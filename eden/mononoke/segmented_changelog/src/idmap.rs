/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;

use anyhow::{format_err, Result};
use futures::{compat::Future01CompatExt, future, TryFutureExt};
use sql::{queries, Connection};
use sql_ext::SqlConnections;

use dag::Id as Vertex;

use mononoke_types::{ChangesetId, RepositoryId};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};

const INSERT_MAX: usize = 1_000;

#[derive(Clone)]
pub struct IdMap(SqlConnections);

queries! {
    write InsertIdMapEntry(values: (repo_id: RepositoryId, vertex: u64, cs_id: ChangesetId)) {
        insert_or_ignore,
        "
        {insert_or_ignore} INTO segmented_changelog_idmap (repo_id, vertex, cs_id)
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

    read SelectManyChangesetIds(repo_id: RepositoryId, >list vertex: u64) -> (u64, ChangesetId) {
        "
        SELECT idmap.vertex as vertex, idmap.cs_id as cs_id
        FROM segmented_changelog_idmap AS idmap
        WHERE idmap.repo_id = {repo_id} AND idmap.vertex IN {vertex}
        "
    }

    read SelectVertex(repo_id: RepositoryId, cs_id: ChangesetId) -> (u64) {
        "
        SELECT idmap.vertex as vertex
        FROM segmented_changelog_idmap AS idmap
        WHERE idmap.repo_id = {repo_id} AND idmap.cs_id = {cs_id}
        "
    }

    read SelectLastEntry(repo_id: RepositoryId) -> (u64, ChangesetId) {
        "
        SELECT idmap.vertex as vertex, idmap.cs_id as cs_id
        FROM segmented_changelog_idmap AS idmap
        WHERE idmap.repo_id = {repo_id} AND idmap.vertex = (
            SELECT MAX(inner.vertex)
            FROM segmented_changelog_idmap AS inner
            WHERE inner.repo_id = {repo_id}
        )
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
        self.insert_many(repo_id, vec![(vertex, cs_id)]).await
    }

    pub async fn insert_many(
        &self,
        repo_id: RepositoryId,
        mut mappings: Vec<(Vertex, ChangesetId)>,
    ) -> Result<()> {
        mappings.sort();
        for chunk in mappings.chunks(INSERT_MAX) {
            let mut to_insert = Vec::with_capacity(chunk.len());
            for (vertex, cs_id) in chunk {
                to_insert.push((&repo_id, &vertex.0, cs_id));
            }
            let mut transaction = self.0.write_connection.start_transaction().compat().await?;
            let query_result = InsertIdMapEntry::query_with_transaction(transaction, &to_insert)
                .compat()
                .await;
            match query_result {
                Err(err) => {
                    // transaction is "lost" to the query
                    return Err(err.context(format_err!(
                        "inserting many IdMap entries for repository {}",
                        repo_id
                    )));
                }
                Ok((t, insert_result)) => {
                    transaction = t;
                    if insert_result.affected_rows() != chunk.len() as u64 {
                        let to_select: Vec<_> = chunk.iter().map(|v| (v.0).0).collect();
                        let (t, rows) = SelectManyChangesetIds::query_with_transaction(
                            transaction,
                            &repo_id,
                            &to_select[..],
                        )
                        .compat()
                        .await?;
                        transaction = t;
                        let changeset_mappings = rows
                            .into_iter()
                            .map(|row| (Vertex(row.0), row.1))
                            .collect::<HashMap<_, _>>();
                        for (vertex, cs_id) in chunk {
                            match changeset_mappings.get(vertex) {
                                None => {
                                    transaction.rollback().compat().await?;
                                    return Err(format_err!(
                                        "Failed to insert entry ({} -> {}) in Idmap",
                                        vertex,
                                        cs_id
                                    ));
                                }
                                Some(store_cs_id) => {
                                    if store_cs_id != cs_id {
                                        transaction.rollback().compat().await?;
                                        return Err(format_err!(
                                            "Duplicate segmented changelog idmap entry {} \
                                                has different assignments: {} vs {}",
                                            vertex,
                                            cs_id,
                                            store_cs_id
                                        ));
                                    }
                                    // TODO(sfilip): log redundant insert call
                                }
                            };
                        }
                    }
                }
            }
            transaction.commit().compat().await?;
        }
        Ok(())
    }

    pub async fn find_changeset_id(
        &self,
        repo_id: RepositoryId,
        vertex: Vertex,
    ) -> Result<Option<ChangesetId>> {
        let result = self.find_many_changeset_ids(repo_id, vec![vertex]).await?;
        Ok(result.get(&vertex).copied())
    }

    pub async fn find_many_changeset_ids(
        &self,
        repo_id: RepositoryId,
        vertexes: Vec<Vertex>,
    ) -> Result<HashMap<Vertex, ChangesetId>> {
        let select_vertexes = |connection: &Connection, vertexes: &[u64]| {
            SelectManyChangesetIds::query(connection, &repo_id, vertexes)
                .compat()
                .and_then(|rows| {
                    future::ok(
                        rows.into_iter()
                            .map(|row| (Vertex(row.0), row.1))
                            .collect::<HashMap<_, _>>(),
                    )
                })
        };
        let to_query: Vec<_> = vertexes.iter().map(|v| v.0).collect();
        let mut cs_ids = select_vertexes(&self.0.read_connection, &to_query).await?;
        let not_found_in_replica: Vec<_> = vertexes
            .iter()
            .filter(|x| cs_ids.contains_key(x))
            .map(|v| v.0)
            .collect();
        if !not_found_in_replica.is_empty() {
            let from_master =
                select_vertexes(&self.0.read_master_connection, &not_found_in_replica).await?;
            for (k, v) in from_master {
                cs_ids.insert(k, v);
            }
        }
        Ok(cs_ids)
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

    pub async fn get_last_entry(
        &self,
        repo_id: RepositoryId,
    ) -> Result<Option<(Vertex, ChangesetId)>> {
        let rows = SelectLastEntry::query(&self.0.read_connection, &repo_id)
            .compat()
            .await?;
        Ok(rows.into_iter().next().map(|r| (Vertex(r.0), r.1)))
    }
}

pub struct MemIdMap {
    vertex2cs: HashMap<Vertex, ChangesetId>,
    cs2vertex: HashMap<ChangesetId, Vertex>,
}

impl MemIdMap {
    pub fn new() -> Self {
        Self {
            vertex2cs: HashMap::new(),
            cs2vertex: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.vertex2cs.len()
    }

    pub fn iter(&self) -> impl Iterator<Item = (Vertex, ChangesetId)> + '_ {
        self.vertex2cs
            .iter()
            .map(|(&vertex, &cs_id)| (vertex, cs_id))
    }

    pub fn insert(&mut self, vertex: Vertex, cs_id: ChangesetId) {
        self.vertex2cs.insert(vertex, cs_id);
        self.cs2vertex.insert(cs_id, vertex);
    }

    pub fn find_changeset_id(&self, vertex: Vertex) -> Option<ChangesetId> {
        self.vertex2cs.get(&vertex).copied()
    }

    pub fn get_changeset_id(&self, vertex: Vertex) -> Result<ChangesetId> {
        self.find_changeset_id(vertex)
            .ok_or_else(|| format_err!("Failed to find segmented changelog id {} in IdMap", vertex))
    }

    pub fn find_vertex(&self, cs_id: ChangesetId) -> Option<Vertex> {
        self.cs2vertex.get(&cs_id).copied()
    }

    pub fn get_vertex(&self, cs_id: ChangesetId) -> Result<Vertex> {
        self.find_vertex(cs_id)
            .ok_or_else(|| format_err!("Failed to find find changeset id {} in IdMap", cs_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use maplit::hashmap;

    use fbinit::FacebookInit;

    use mononoke_types_mocks::changesetid::{
        FIVES_CSID, FOURS_CSID, ONES_CSID, THREES_CSID, TWOS_CSID,
    };

    #[fbinit::compat_test]
    async fn test_get_last_entry(_fb: FacebookInit) -> Result<()> {
        let repo_id = RepositoryId::new(0);
        let idmap = IdMap::with_sqlite_in_memory()?;

        assert_eq!(idmap.get_last_entry(repo_id).await?, None);

        idmap.insert(repo_id, Vertex(1), ONES_CSID).await?;
        idmap.insert(repo_id, Vertex(2), TWOS_CSID).await?;
        idmap.insert(repo_id, Vertex(3), THREES_CSID).await?;

        assert_eq!(
            idmap.get_last_entry(repo_id).await?,
            Some((Vertex(3), THREES_CSID))
        );

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_insert_many(_fb: FacebookInit) -> Result<()> {
        let repo_id = RepositoryId::new(0);
        let idmap = IdMap::with_sqlite_in_memory()?;

        assert_eq!(idmap.get_last_entry(repo_id).await?, None);

        idmap.insert_many(repo_id, vec![]).await?;
        idmap
            .insert_many(
                repo_id,
                vec![
                    (Vertex(1), ONES_CSID),
                    (Vertex(2), TWOS_CSID),
                    (Vertex(3), THREES_CSID),
                ],
            )
            .await?;

        assert_eq!(idmap.get_changeset_id(repo_id, Vertex(1)).await?, ONES_CSID);
        assert_eq!(
            idmap.get_changeset_id(repo_id, Vertex(3)).await?,
            THREES_CSID
        );

        idmap
            .insert_many(
                repo_id,
                vec![
                    (Vertex(1), ONES_CSID),
                    (Vertex(2), TWOS_CSID),
                    (Vertex(3), THREES_CSID),
                ],
            )
            .await?;
        assert_eq!(idmap.get_changeset_id(repo_id, Vertex(2)).await?, TWOS_CSID);

        idmap
            .insert_many(
                repo_id,
                vec![(Vertex(1), ONES_CSID), (Vertex(4), FOURS_CSID)],
            )
            .await?;
        assert_eq!(
            idmap.get_changeset_id(repo_id, Vertex(4)).await?,
            FOURS_CSID
        );

        assert!(idmap
            .insert_many(repo_id, vec![(Vertex(1), FIVES_CSID)])
            .await
            .is_err());

        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_find_many_changeset_ids(_fb: FacebookInit) -> Result<()> {
        let repo_id = RepositoryId::new(0);
        let idmap = IdMap::with_sqlite_in_memory()?;

        let response = idmap
            .find_many_changeset_ids(repo_id, vec![Vertex(1), Vertex(2), Vertex(3), Vertex(6)])
            .await?;
        assert!(response.is_empty());

        idmap
            .insert_many(
                repo_id,
                vec![
                    (Vertex(1), ONES_CSID),
                    (Vertex(2), TWOS_CSID),
                    (Vertex(3), THREES_CSID),
                    (Vertex(4), FOURS_CSID),
                    (Vertex(5), FIVES_CSID),
                ],
            )
            .await?;

        let response = idmap
            .find_many_changeset_ids(repo_id, vec![Vertex(1), Vertex(2), Vertex(3), Vertex(6)])
            .await?;
        assert_eq!(
            response,
            hashmap![Vertex(1) => ONES_CSID, Vertex(2) => TWOS_CSID, Vertex(3) => THREES_CSID]
        );

        let response = idmap
            .find_many_changeset_ids(repo_id, vec![Vertex(4), Vertex(5)])
            .await?;
        assert_eq!(
            response,
            hashmap![Vertex(4) => FOURS_CSID, Vertex(5) => FIVES_CSID]
        );

        let response = idmap
            .find_many_changeset_ids(repo_id, vec![Vertex(6)])
            .await?;
        assert!(response.is_empty());

        Ok(())
    }
}
