/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Result};
use futures::compat::Future01CompatExt;
use maplit::hashset;
use sql::queries;
use sql_ext::SqlConnections;

use dag::Id as Vertex;

use blobrepo::BlobRepo;
use context::CoreContext;
use mononoke_types::ChangesetId;
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};

use crate::parents::Parents;

#[derive(Clone)]
pub struct IdMap(SqlConnections);

queries! {
    // TODO(sfilip): upsert is a hack around a limited build_up implementation, we want insert_or_ignore.
    write InsertIdMapEntry(values: (vertex: u64, cs_id: ChangesetId)) {
        none,
        "
        INSERT OR REPLACE INTO segmented_changelog_idmap (vertex, cs_id)
        VALUES {values}
        "
    }

    read SelectChangesetId(vertex: u64) -> (ChangesetId) {
        "
        SELECT idmap.cs_id as cs_id
        FROM segmented_changelog_idmap AS idmap
        WHERE idmap.vertex = {vertex}
        "
    }

    read SelectVertex(cs_id: ChangesetId) -> (u64) {
        "
        SELECT idmap.vertex as vertex
        FROM segmented_changelog_idmap AS idmap
        WHERE idmap.cs_id = {cs_id}
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
    pub async fn insert(&self, vertex: Vertex, cs_id: ChangesetId) -> Result<()> {
        // TODO(sfilip): add tests
        let result = InsertIdMapEntry::query(&self.0.write_connection, &[(&vertex.0, &cs_id)])
            .compat()
            .await?;
        if result.affected_rows() != 1 {
            let stored = SelectChangesetId::query(&self.0.read_master_connection, &vertex.0)
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

    pub async fn find_changeset_id(&self, vertex: Vertex) -> Result<Option<ChangesetId>> {
        let select = |connection| async move {
            let rows = SelectChangesetId::query(connection, &vertex.0)
                .compat()
                .await?;
            Ok(rows.into_iter().next().map(|r| r.0))
        };
        match select(&self.0.read_connection).await? {
            None => select(&self.0.read_master_connection).await,
            Some(cs_id) => Ok(Some(cs_id)),
        }
    }

    pub async fn get_changeset_id(&self, vertex: Vertex) -> Result<ChangesetId> {
        self.find_changeset_id(vertex)
            .await?
            .ok_or_else(|| format_err!("Failed to find segmented changelog id {} in IdMap", vertex))
    }

    pub async fn find_vertex(&self, cs_id: ChangesetId) -> Result<Option<Vertex>> {
        let select = |connection| async move {
            let rows = SelectVertex::query(connection, &cs_id).compat().await?;
            Ok(rows.into_iter().next().map(|r| Vertex(r.0)))
        };
        match select(&self.0.read_connection).await? {
            None => select(&self.0.read_master_connection).await,
            Some(v) => Ok(Some(v)),
        }
    }

    pub async fn get_vertex(&self, cs_id: ChangesetId) -> Result<Vertex> {
        self.find_vertex(cs_id)
            .await?
            .ok_or_else(|| format_err!("Failed to find find changeset id {} in IdMap", cs_id))
    }

    pub async fn build_up(
        &self,
        ctx: &CoreContext,
        blob_repo: &BlobRepo,
        head: ChangesetId,
    ) -> Result<Vertex> {
        enum Todo {
            Visit(ChangesetId),
            Assign(ChangesetId),
        }
        let mut next_vertex = dag::Group::MASTER.min_id().0;
        let parents = Parents::new(ctx, blob_repo);
        let mut todo_stack = vec![Todo::Visit(head)];
        let mut seen = hashset![head];
        while let Some(todo) = todo_stack.pop() {
            match todo {
                Todo::Visit(cs_id) => {
                    todo_stack.push(Todo::Assign(cs_id));
                    let parents = parents.get(cs_id).await?;
                    for parent in parents.into_iter().rev() {
                        // Note: iterating parents in reverse is a small optimization because
                        // in our setup p1 is master.
                        if !seen.contains(&parent) {
                            seen.insert(parent);
                            todo_stack.push(Todo::Visit(parent));
                        }
                    }
                }
                Todo::Assign(cs_id) => {
                    self.insert(Vertex(next_vertex), cs_id).await?;
                    next_vertex += 1;
                }
            }
        }
        match self.find_vertex(head).await? {
            None => Err(format_err!(
                "Error building IdMap. Failed to assign head {}",
                head
            )),
            Some(vertex) => Ok(vertex),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use fbinit::FacebookInit;
    use fixtures::{linear, merge_even, merge_uneven};
    use futures::compat::{Future01CompatExt, Stream01CompatExt};
    use futures::StreamExt;
    use revset::AncestorsNodeStream;
    use tests_utils::resolve_cs_id;

    async fn validate_build_up(ctx: CoreContext, repo: BlobRepo, head: &'static str) -> Result<()> {
        let idmap = IdMap::with_sqlite_in_memory()?;
        let head = resolve_cs_id(&ctx, &repo, head).await?;
        idmap.build_up(&ctx, &repo, head).await?;

        let mut ancestors =
            AncestorsNodeStream::new(ctx.clone(), &repo.get_changeset_fetcher(), head).compat();
        while let Some(cs_id) = ancestors.next().await {
            let cs_id = cs_id?;
            let parents = repo
                .get_changeset_parents_by_bonsai(ctx.clone(), cs_id)
                .compat()
                .await?;
            for parent in parents {
                let parent_vertex = idmap.get_vertex(parent).await?;
                let vertex = idmap.get_vertex(cs_id).await?;
                assert!(parent_vertex < vertex);
            }
        }
        Ok(())
    }

    #[fbinit::compat_test]
    async fn test_build_up_idmap(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        validate_build_up(
            ctx.clone(),
            linear::getrepo(fb).await,
            "79a13814c5ce7330173ec04d279bf95ab3f652fb",
        )
        .await?;
        validate_build_up(
            ctx.clone(),
            merge_even::getrepo(fb).await,
            "4dcf230cd2f20577cb3e88ba52b73b376a2b3f69",
        )
        .await?;
        validate_build_up(
            ctx.clone(),
            merge_uneven::getrepo(fb).await,
            "7221fa26c85f147db37c2b5f4dbcd5fe52e7645b",
        )
        .await?;
        Ok(())
    }
}
