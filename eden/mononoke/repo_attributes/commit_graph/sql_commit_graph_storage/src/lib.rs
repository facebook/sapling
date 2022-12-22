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
        _ctx: &CoreContext,
        _cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEdges>> {
        todo!()
    }

    async fn fetch_many_edges(
        &self,
        _ctx: &CoreContext,
        _cs_ids: &[ChangesetId],
        _prefetch_hint: Option<Generation>,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        todo!()
    }

    async fn find_by_prefix(
        &self,
        _ctx: &CoreContext,
        _cs_prefix: ChangesetIdPrefix,
        _limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        todo!()
    }
}
