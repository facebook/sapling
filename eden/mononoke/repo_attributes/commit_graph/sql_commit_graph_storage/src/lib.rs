/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! SQL Commit Graph Storage
//!
//! Database-backed implementation of the commit graph storage.
#![allow(unused)]

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Result;
use async_trait::async_trait;
use commit_graph::edges::ChangesetEdges;
use commit_graph::edges::ChangesetNode;
use commit_graph::edges::ChangesetNodeParents;
use commit_graph::storage::CommitGraphStorage;
use commit_graph::ChangesetParents;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;
use rendezvous::RendezVous;
use rendezvous::RendezVousOptions;
use rendezvous::RendezVousStats;
use rendezvous::TunablesRendezVousController;
use sql::queries;
use sql::Connection;
use sql::SqlConnections;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;

#[cfg(test)]
mod tests;

pub struct SqlCommitGraphStorageBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlCommitGraphStorageBuilder {
    const LABEL: &'static str = "commit_graph";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-commit-graph.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self { connections }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlCommitGraphStorageBuilder {}

impl SqlCommitGraphStorageBuilder {
    pub fn build(self, opts: RendezVousOptions, repo_id: RepositoryId) -> SqlCommitGraphStorage {
        SqlCommitGraphStorage {
            repo_id,
            read_connection: RendezVousConnection::new(
                self.connections.read_connection,
                "read",
                opts,
            ),
            read_master_connection: RendezVousConnection::new(
                self.connections.read_master_connection,
                "read_master",
                opts,
            ),
            write_connection: self.connections.write_connection,
        }
    }
}

#[derive(Clone)]
struct RendezVousConnection {
    fetch_single: RendezVous<ChangesetId, ChangesetEdges>,
    conn: Connection,
}

impl RendezVousConnection {
    fn new(conn: Connection, name: &str, opts: RendezVousOptions) -> Self {
        Self {
            conn,
            fetch_single: RendezVous::new(
                TunablesRendezVousController::new(opts),
                Arc::new(RendezVousStats::new(format!(
                    "commit_graph.fetch_single.{}",
                    name
                ))),
            ),
        }
    }
}

pub struct SqlCommitGraphStorage {
    repo_id: RepositoryId,
    write_connection: Connection,
    read_connection: RendezVousConnection,
    read_master_connection: RendezVousConnection,
}

queries! {
    write InsertChangeset(
        repo_id: RepositoryId,
        cs_id: ChangesetId,
        gen: u64,
        skip_tree_depth: u64,
        p1_linear_depth: u64,
        parent_count: usize,
        p1_parent: Option<ChangesetId>,
        merge_ancestor: Option<ChangesetId>,
        skip_tree_parent: Option<ChangesetId>,
        skip_tree_skew_ancestor: Option<ChangesetId>,
        p1_linear_skew_ancestor: Option<ChangesetId>
    ) {
        insert_or_ignore,
        "
        {insert_or_ignore} INTO commit_graph_edges (
            repo_id,
            cs_id,
            gen,
            skip_tree_depth,
            p1_linear_depth,
            parent_count,
            p1_parent,
            merge_ancestor,
            skip_tree_parent,
            skip_tree_skew_ancestor,
            p1_linear_skew_ancestor
        ) VALUES (
            {repo_id},
            {cs_id},
            {gen},
            {skip_tree_depth},
            {p1_linear_depth},
            {parent_count},
            (SELECT cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id = {p1_parent}),
            (SELECT cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id = {merge_ancestor}),
            (SELECT cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id = {skip_tree_parent}),
            (SELECT cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id = {skip_tree_skew_ancestor}),
            (SELECT cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id = {p1_linear_skew_ancestor})
        )
        "
    }

    read SelectManyIds(repo_id: RepositoryId, >list cs_ids: ChangesetId) -> (ChangesetId, u64) {
        "SELECT cs.cs_id, cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id IN {cs_ids}"
    }

    write InsertMergeParents(values: (id: u64, parent_num: usize, parent: u64)) {
        insert_or_ignore,
        "{insert_or_ignore} INTO commit_graph_merge_parents (id, parent_num, parent) VALUES {values}"
    }

    read SelectManyChangesets(repo_id: RepositoryId, >list cs_ids: ChangesetId) -> (
        ChangesetId, // cs_id
        Option<u64>, // gen
        Option<u64>, // skip_tree_depth
        Option<u64>, // p1_linear_depth
        Option<usize>, // parent_count
        Option<ChangesetId>, // merge_ancestor
        Option<u64>, // merge_ancestor_gen
        Option<u64>, // merge_ancestor_skip_tree_depth
        Option<u64>, // merge_ancestor_p1_linear_depth
        Option<ChangesetId>, // skip_tree_parent
        Option<u64>, // skip_tree_parent_gen
        Option<u64>, // skip_tree_parent_skip_tree_depth
        Option<u64>, // skip_tree_parent_p1_linear_depth
        Option<ChangesetId>, // skip_tree_skew_ancestor
        Option<u64>, // skip_tree_skew_ancestor_gen
        Option<u64>, // skip_tree_skew_ancestor_skip_tree_depth
        Option<u64>, // skip_tree_skew_ancestor_p1_linear_depth
        Option<ChangesetId>, // p1_linear_skew_ancestor
        Option<u64>, // p1_linear_skew_ancestor_gen
        Option<u64>, // p1_linear_skew_ancestor_skip_tree_depth
        Option<u64>, // p1_linear_skew_ancestor_p1_linear_depth
        usize, // parent_num
        Option<ChangesetId>, // parent
        Option<u64>, // parent_gen
        Option<u64>, // parent_skip_tree_depth
        Option<u64>, // parent_p1_linear_depth
    ) {
        "
        SELECT
            cs0.cs_id AS cs_id,
            NULL AS gen,
            NULL AS skip_tree_depth,
            NULL AS p1_linear_depth,
            NULL AS parent_count,
            NULL AS merge_ancestor,
            NULL AS merge_ancestor_gen,
            NULL AS merge_ancestor_skip_tree_depth,
            NULL AS merge_ancestor_p1_linear_depth,
            NULL AS skip_tree_parent,
            NULL AS skip_tree_parent_gen,
            NULL AS skip_tree_parent_skip_tree_depth,
            NULL AS skip_tree_parent_p1_linear_depth,
            NULL AS skip_tree_skew_ancestor,
            NULL AS skip_tree_skew_ancestor_gen,
            NULL AS skip_tree_skew_ancestor_skip_tree_depth,
            NULL AS skip_tree_skew_ancestor_p1_linear_depth,
            NULL AS p1_linear_skew_ancestor,
            NULL AS p1_linear_skew_ancestor_gen,
            NULL AS p1_linear_skew_ancestor_skip_tree_depth,
            NULL AS p1_linear_skew_ancestor_p1_linear_depth,
            commit_graph_merge_parents.parent_num AS parent_num,
            cs1.cs_id AS parent,
            cs1.gen AS parent_gen,
            cs1.skip_tree_depth AS parent_skip_tree_depth,
            cs1.p1_linear_depth AS parent_p1_linear_depth
        FROM commit_graph_merge_parents
        INNER JOIN commit_graph_edges cs0 ON cs0.id = commit_graph_merge_parents.id
        INNER JOIN commit_graph_edges cs1 ON cs1.id = commit_graph_merge_parents.parent
        WHERE cs0.repo_id = {repo_id} AND cs0.cs_id IN {cs_ids} AND cs1.repo_id = {repo_id} AND cs0.parent_count >= 2

        UNION

        SELECT
            cs0.cs_id AS cs_id,
            cs0.gen AS gen,
            cs0.skip_tree_depth AS skip_tree_depth,
            cs0.p1_linear_depth AS p1_linear_depth,
            cs0.parent_count AS parent_count,
            cs_merge_ancestor.cs_id AS merge_ancestor,
            cs_merge_ancestor.gen AS merge_ancestor_gen,
            cs_merge_ancestor.skip_tree_depth AS merge_ancestor_skip_tree_depth,
            cs_merge_ancestor.p1_linear_depth AS merge_ancestor_p1_linear_depth,
            cs_skip_tree_parent.cs_id AS skip_tree_parent,
            cs_skip_tree_parent.gen AS skip_tree_parent_gen,
            cs_skip_tree_parent.skip_tree_depth AS skip_tree_parent_skip_tree_depth,
            cs_skip_tree_parent.p1_linear_depth AS skip_tree_parent_p1_linear_depth,
            cs_skip_tree_skew_ancestor.cs_id AS skip_tree_skew_ancestor,
            cs_skip_tree_skew_ancestor.gen AS skip_tree_skew_ancestor_gen,
            cs_skip_tree_skew_ancestor.skip_tree_depth AS skip_tree_skew_ancestor_skip_tree_depth,
            cs_skip_tree_skew_ancestor.p1_linear_depth AS skip_tree_skew_ancestor_p1_linear_depth,
            cs_p1_linear_skew_ancestor.cs_id AS p1_linear_skew_ancestor,
            cs_p1_linear_skew_ancestor.gen AS p1_linear_skew_ancestor_gen,
            cs_p1_linear_skew_ancestor.skip_tree_depth AS p1_linear_skew_ancestor_skip_tree_depth,
            cs_p1_linear_skew_ancestor.p1_linear_depth AS p1_linear_skew_ancestor_p1_linear_depth,
            0 AS parent_num,
            cs_p1_parent.cs_id AS parent,
            cs_p1_parent.gen AS parent_gen,
            cs_p1_parent.skip_tree_depth AS parent_skip_tree_depth,
            cs_p1_parent.p1_linear_depth AS parent_p1_linear_depth
        FROM commit_graph_edges cs0
        LEFT JOIN commit_graph_edges cs_p1_parent ON cs_p1_parent.id = cs0.p1_parent
        LEFT JOIN commit_graph_edges cs_merge_ancestor ON cs_merge_ancestor.id = cs0.merge_ancestor
        LEFT JOIN commit_graph_edges cs_skip_tree_parent ON cs_skip_tree_parent.id = cs0.skip_tree_parent
        LEFT JOIN commit_graph_edges cs_skip_tree_skew_ancestor ON cs_skip_tree_skew_ancestor.id = cs0.skip_tree_skew_ancestor
        LEFT JOIN commit_graph_edges cs_p1_linear_skew_ancestor ON cs_p1_linear_skew_ancestor.id = cs0.p1_linear_skew_ancestor
        WHERE cs0.repo_id = {repo_id} and cs0.cs_id IN {cs_ids}

        ORDER BY parent_num ASC
        "
    }

    read SelectChangesetsInRange(repo_id: RepositoryId, min_id: ChangesetId, max_id: ChangesetId, limit: usize) -> (ChangesetId) {
        "
        SELECT cs_id
        FROM commit_graph_edges
        WHERE repo_id = {repo_id} AND {min_id} <= cs_id AND cs_id <= {max_id}
        ORDER BY cs_id ASC
        LIMIT {limit}
        "
    }
}

impl SqlCommitGraphStorage {
    async fn fetch_many_edges_impl(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        _prefetch_hint: Option<Generation>,
        rendezvous: &RendezVousConnection,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        if cs_ids.is_empty() {
            // This is actually NECESSARY, because SQL doesn't deal well with
            // querying empty arrays
            return Ok(HashMap::new());
        }
        let option_fields_to_option_node =
            |cs_id, generation, skip_tree_depth, p1_linear_depth| match (
                cs_id,
                generation,
                skip_tree_depth,
                p1_linear_depth,
            ) {
                (Some(cs_id), Some(generation), Some(skip_tree_depth), Some(p1_linear_depth)) => {
                    Some(ChangesetNode {
                        cs_id,
                        generation: Generation::new(generation),
                        skip_tree_depth,
                        p1_linear_depth,
                    })
                }
                _ => None,
            };

        let ret = rendezvous
            .fetch_single
            .dispatch(ctx.fb.clone(), cs_ids.iter().copied().collect(), || {
                let conn = rendezvous.conn.clone();
                let repo_id = self.repo_id.clone();

                move |cs_ids| async move {
                    let cs_ids = cs_ids.into_iter().collect::<Vec<_>>();

                    let fetched_edges =
                        SelectManyChangesets::query(&conn, &repo_id, cs_ids.as_slice()).await?;

                    let mut cs_id_to_cs_edges = HashMap::new();

                    for row in fetched_edges.iter() {
                        match *row {
                            (
                                cs_id,
                                Some(gen),
                                Some(skip_tree_depth),
                                Some(p1_linear_depth),
                                Some(parent_count),
                                merge_ancestor,
                                merge_ancestor_gen,
                                merge_ancestor_skip_tree_depth,
                                merge_ancestor_p1_linear_depth,
                                skip_tree_parent,
                                skip_tree_parent_gen,
                                skip_tree_parent_skip_tree_depth,
                                skip_tree_parent_p1_linear_depth,
                                skip_tree_skew_ancestor,
                                skip_tree_skew_ancestor_gen,
                                skip_tree_skew_ancestor_skip_tree_depth,
                                skip_tree_skew_ancestor_p1_linear_depth,
                                p1_linear_skew_ancestor,
                                p1_linear_skew_ancestor_gen,
                                p1_linear_skew_ancestor_skip_tree_depth,
                                p1_linear_skew_ancestor_p1_linear_depth,
                                ..,
                            ) => {
                                cs_id_to_cs_edges.insert(
                                    cs_id,
                                    ChangesetEdges {
                                        node: ChangesetNode {
                                            cs_id,
                                            generation: Generation::new(gen),
                                            skip_tree_depth,
                                            p1_linear_depth,
                                        },
                                        parents: ChangesetNodeParents::new(),
                                        merge_ancestor: option_fields_to_option_node(
                                            merge_ancestor,
                                            merge_ancestor_gen,
                                            merge_ancestor_skip_tree_depth,
                                            merge_ancestor_p1_linear_depth,
                                        ),
                                        skip_tree_parent: option_fields_to_option_node(
                                            skip_tree_parent,
                                            skip_tree_parent_gen,
                                            skip_tree_parent_skip_tree_depth,
                                            skip_tree_parent_p1_linear_depth,
                                        ),
                                        skip_tree_skew_ancestor: option_fields_to_option_node(
                                            skip_tree_skew_ancestor,
                                            skip_tree_skew_ancestor_gen,
                                            skip_tree_skew_ancestor_skip_tree_depth,
                                            skip_tree_skew_ancestor_p1_linear_depth,
                                        ),
                                        p1_linear_skew_ancestor: option_fields_to_option_node(
                                            p1_linear_skew_ancestor,
                                            p1_linear_skew_ancestor_gen,
                                            p1_linear_skew_ancestor_skip_tree_depth,
                                            p1_linear_skew_ancestor_p1_linear_depth,
                                        ),
                                    },
                                );
                            }
                            _ => continue,
                        }
                    }

                    for row in fetched_edges {
                        match row {
                            (
                                cs_id,
                                ..,
                                Some(parent),
                                Some(parent_gen),
                                Some(parent_skip_tree_depth),
                                Some(parent_p1_linear_depth),
                            ) => {
                                if let Some(edge) = cs_id_to_cs_edges.get_mut(&cs_id) {
                                    edge.parents.push(ChangesetNode {
                                        cs_id: parent,
                                        generation: Generation::new(parent_gen),
                                        skip_tree_depth: parent_skip_tree_depth,
                                        p1_linear_depth: parent_p1_linear_depth,
                                    })
                                }
                            }
                            _ => continue,
                        }
                    }

                    Ok(cs_id_to_cs_edges)
                }
            })
            .await?;

        Ok(ret
            .into_iter()
            .filter_map(|(cs_id, cs_edge)| cs_edge.map(|cs_edge| (cs_id, cs_edge)))
            .collect())
    }
}

#[async_trait]
impl CommitGraphStorage for SqlCommitGraphStorage {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add(&self, _ctx: &CoreContext, edges: ChangesetEdges) -> Result<bool> {
        let merge_parent_cs_id_to_id: HashMap<ChangesetId, u64> = if edges.parents.len() >= 2 {
            SelectManyIds::query(
                &self.read_connection.conn,
                &self.repo_id,
                &edges
                    .parents
                    .iter()
                    .map(|node| node.cs_id)
                    .collect::<Vec<_>>(),
            )
            .await?
            .into_iter()
            .collect()
        } else {
            Default::default()
        };

        let transaction = self.write_connection.start_transaction().await?;

        let (transaction, result) = InsertChangeset::query_with_transaction(
            transaction,
            &self.repo_id,
            &edges.node.cs_id,
            &edges.node.generation.value(),
            &edges.node.skip_tree_depth,
            &edges.node.p1_linear_depth,
            &edges.parents.len(),
            &edges.parents.get(0).map(|node| node.cs_id),
            &edges.merge_ancestor.map(|node| node.cs_id),
            &edges.skip_tree_parent.map(|node| node.cs_id),
            &edges.skip_tree_skew_ancestor.map(|node| node.cs_id),
            &edges.p1_linear_skew_ancestor.map(|node| node.cs_id),
        )
        .await?;

        match result.last_insert_id() {
            Some(last_insert_id) if result.affected_rows() == 1 => {
                let merge_parent_rows = edges
                    .parents
                    .iter()
                    .enumerate()
                    .skip(1)
                    .map(|(parent_num, node)| {
                        Ok((
                            last_insert_id,
                            parent_num,
                            *merge_parent_cs_id_to_id
                                .get(&node.cs_id)
                                .ok_or_else(|| anyhow!("Failed to fetch id for {}", node.cs_id))?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;

                let ref_merge_parent_rows = merge_parent_rows
                    .iter()
                    .map(|(id, parent_num, parent_id)| (id, parent_num, parent_id))
                    .collect::<Vec<_>>();

                let (transaction, result) = InsertMergeParents::query_with_transaction(
                    transaction,
                    ref_merge_parent_rows.as_slice(),
                )
                .await?;

                transaction.commit().await?;

                Ok(true)
            }
            _ => {
                transaction.rollback().await?;
                Ok(false)
            }
        }
    }

    async fn fetch_edges(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEdges>> {
        Ok(self
            .fetch_many_edges(ctx, &[cs_id], None)
            .await?
            .remove(&cs_id))
    }

    async fn fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch_hint: Option<Generation>,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        self.fetch_many_edges_impl(ctx, cs_ids, prefetch_hint, &self.read_connection)
            .await
    }

    async fn fetch_many_edges_required(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch_hint: Option<Generation>,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        let mut edges = self
            .fetch_many_edges_impl(ctx, cs_ids, prefetch_hint, &self.read_connection)
            .await?;
        let unfetched_ids: Vec<ChangesetId> = cs_ids
            .iter()
            .filter(|id| !edges.contains_key(id))
            .copied()
            .collect();
        let unfetched_ids = if !unfetched_ids.is_empty() {
            // Let's go to master with the remaining edges
            let extra_edges = self
                .fetch_many_edges_impl(
                    ctx,
                    &unfetched_ids,
                    prefetch_hint,
                    &self.read_master_connection,
                )
                .await?;
            edges.extend(extra_edges);
            cs_ids
                .iter()
                .filter(|id| !edges.contains_key(id))
                .copied()
                .collect()
        } else {
            unfetched_ids
        };
        if !unfetched_ids.is_empty() {
            anyhow::bail!(
                "Missing changesets in commit graph: {}",
                unfetched_ids
                    .into_iter()
                    .map(|id| format!("{}, ", id))
                    .collect::<String>()
            );
        }
        Ok(edges)
    }

    async fn find_by_prefix(
        &self,
        ctx: &CoreContext,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        let mut fetched_ids = SelectChangesetsInRange::query(
            &self.read_connection.conn,
            &self.repo_id,
            &cs_prefix.min_bound(),
            &cs_prefix.max_bound(),
            &(limit + 1),
        )
        .await?
        .into_iter()
        .map(|(cs_id,)| cs_id)
        .collect::<Vec<_>>();

        match fetched_ids.len() {
            0 => Ok(ChangesetIdsResolvedFromPrefix::NoMatch),
            1 => Ok(ChangesetIdsResolvedFromPrefix::Single(fetched_ids[0])),
            l if l <= limit => Ok(ChangesetIdsResolvedFromPrefix::Multiple(fetched_ids)),
            _ => Ok(ChangesetIdsResolvedFromPrefix::TooMany({
                fetched_ids.pop();
                fetched_ids
            })),
        }
    }
}
