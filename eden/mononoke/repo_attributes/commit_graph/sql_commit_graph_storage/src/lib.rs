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
use std::collections::HashSet;
use std::fmt::Write;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use commit_graph_types::edges::ChangesetEdges;
use commit_graph_types::edges::ChangesetNode;
use commit_graph_types::edges::ChangesetNodeParents;
use commit_graph_types::edges::ChangesetNodeSubtreeSources;
use commit_graph_types::edges::ChangesetParents;
use commit_graph_types::storage::CommitGraphStorage;
use commit_graph_types::storage::FetchedChangesetEdges;
use commit_graph_types::storage::Prefetch;
use commit_graph_types::storage::PrefetchTarget;
use context::CoreContext;
use context::PerfCounterType;
use itertools::Itertools;
use mononoke_types::ChangesetId;
use mononoke_types::ChangesetIdPrefix;
use mononoke_types::ChangesetIdsResolvedFromPrefix;
use mononoke_types::Generation;
use mononoke_types::RepositoryId;
use rendezvous::ConfigurableRendezVousController;
use rendezvous::RendezVous;
use rendezvous::RendezVousOptions;
use rendezvous::RendezVousStats;
use retry::RetryLogic;
use retry::retry;
use sql::Connection;
use sql::SqlConnections;
use sql::mysql::IsolationLevel;
use sql_construct::SqlConstruct;
use sql_construct::SqlConstructFromMetadataDatabaseConfig;
use sql_ext::mononoke_queries;
use sql_ext::should_retry_query;
use vec1::Vec1;
use vec1::vec1;

pub use crate::bulkops::ArcCommitGraphBulkFetcher;
pub use crate::bulkops::CommitGraphBulkFetcher;
pub use crate::bulkops::CommitGraphBulkFetcherArc;
pub use crate::bulkops::CommitGraphBulkFetcherRef;

mod bulkops;
#[cfg(test)]
mod tests;

/// Maximum number of recursive steps to take when prefetching commits.
///
/// The configured maximum number of recursive steps in MySQL is 1000.
const DEFAULT_PREFETCH_STEP_LIMIT: u64 = 1000;

pub struct SqlCommitGraphStorageBuilder {
    connections: SqlConnections,
}

impl SqlConstruct for SqlCommitGraphStorageBuilder {
    const LABEL: &'static str = "commit_graph";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-commit-graph.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        let SqlConnections {
            read_connection,
            read_master_connection,
            mut write_connection,
        } = connections;

        if let Connection::Mysql(conn) = &mut write_connection {
            conn.set_isolation_level(Some(IsolationLevel::ReadCommitted));
        }

        Self {
            connections: SqlConnections {
                read_connection,
                read_master_connection,
                write_connection,
            },
        }
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
    fetch_single: RendezVous<ChangesetId, FetchedChangesetEdges>,

    // For prefetching, RendezVous works as mapping from `origin_cs_id` to a `Vec` of fetched edges.
    fetch_linear_prefetch: RendezVous<ChangesetId, Vec<FetchedChangesetEdges>>,
    fetch_skip_tree_prefetch: RendezVous<ChangesetId, Vec<FetchedChangesetEdges>>,
    fetch_exact_skip_tree_prefetch: RendezVous<ChangesetId, Vec<FetchedChangesetEdges>>,

    conn: Connection,
}

impl RendezVousConnection {
    fn new(conn: Connection, name: &str, opts: RendezVousOptions) -> Self {
        Self {
            conn,
            fetch_single: RendezVous::new(
                ConfigurableRendezVousController::new(opts),
                Arc::new(RendezVousStats::new(format!(
                    "commit_graph.fetch_single.{}",
                    name
                ))),
            ),
            fetch_linear_prefetch: RendezVous::new(
                ConfigurableRendezVousController::new(opts),
                Arc::new(RendezVousStats::new(format!(
                    "commit_graph.fetch_linear_prefetch.{}",
                    name
                ))),
            ),
            fetch_skip_tree_prefetch: RendezVous::new(
                ConfigurableRendezVousController::new(opts),
                Arc::new(RendezVousStats::new(format!(
                    "commit_graph.fetch_skip_tree_prefetch.{}",
                    name
                ))),
            ),
            fetch_exact_skip_tree_prefetch: RendezVous::new(
                ConfigurableRendezVousController::new(opts),
                Arc::new(RendezVousStats::new(format!(
                    "commit_graph.fetch_exact_skip_tree_prefetch.{}",
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

// Utility macro for defining a query that fetches commit graph edges.
//
// The first part of the query should create a common table named `csp`
// which has `id` and `origin_cs_id` fields for the edges that are
// requested.
macro_rules! fetch_commit_graph_edges {
    ($query:literal) => {
        concat!(
            $query,
            "SELECT
                cs0.cs_id AS cs_id,
                csp.origin_cs_id AS origin_cs_id,
                NULL AS gen,
                NULL AS subtree_source_gen,
                NULL AS skip_tree_depth,
                NULL AS p1_linear_depth,
                NULL AS subtree_source_depth,
                NULL AS parent_count,
                NULL AS subtree_source_count,
                NULL AS merge_ancestor,
                NULL AS merge_ancestor_gen,
                NULL AS merge_ancestor_subtree_source_gen, 
                NULL AS merge_ancestor_skip_tree_depth,
                NULL AS merge_ancestor_p1_linear_depth,
                NULL AS merge_ancestor_subtree_source_depth,
                NULL AS skip_tree_parent,
                NULL AS skip_tree_parent_gen,
                NULL AS skip_tree_parent_subtree_source_gen,
                NULL AS skip_tree_parent_skip_tree_depth,
                NULL AS skip_tree_parent_p1_linear_depth,
                NULL AS skip_tree_parent_subtree_source_depth,
                NULL AS skip_tree_skew_ancestor,
                NULL AS skip_tree_skew_ancestor_gen,
                NULL AS skip_tree_skew_ancestor_subtree_source_gen,
                NULL AS skip_tree_skew_ancestor_skip_tree_depth,
                NULL AS skip_tree_skew_ancestor_p1_linear_depth,
                NULL AS skip_tree_skew_ancestor_subtree_source_depth,
                NULL AS p1_linear_skew_ancestor,
                NULL AS p1_linear_skew_ancestor_gen,
                NULL AS p1_linear_skew_ancestor_subtree_source_gen,
                NULL AS p1_linear_skew_ancestor_skip_tree_depth,
                NULL AS p1_linear_skew_ancestor_p1_linear_depth,
                NULL AS p1_linear_skew_ancestor_subtree_source_depth,
                NULL AS subtree_or_merge_ancestor,
                NULL AS subtree_or_merge_ancestor_gen,
                NULL AS subtree_or_merge_ancestor_subtree_source_gen,
                NULL AS subtree_or_merge_ancestor_skip_tree_depth,
                NULL AS subtree_or_merge_ancestor_p1_linear_depth,
                NULL AS subtree_or_merge_ancestor_subtree_source_depth,
                NULL AS subtree_source_parent,
                NULL AS subtree_source_parent_gen,
                NULL AS subtree_source_parent_subtree_source_gen,
                NULL AS subtree_source_parent_skip_tree_depth,
                NULL AS subtree_source_parent_p1_linear_depth,
                NULL AS subtree_source_parent_subtree_source_depth,
                NULL AS subtree_source_skew_ancestor,
                NULL AS subtree_source_skew_ancestor_gen,
                NULL AS subtree_source_skew_ancestor_subtree_source_gen,
                NULL AS subtree_source_skew_ancestor_skip_tree_depth,
                NULL AS subtree_source_skew_ancestor_p1_linear_depth,
                NULL AS subtree_source_skew_ancestor_subtree_source_depth,
                cgmp.parent_num AS parent_num,
                NULL AS subtree_source_num,
                cs1.cs_id AS parent,
                cs1.gen AS parent_gen,
                cs1.subtree_source_gen AS parent_subtree_source_gen,
                cs1.skip_tree_depth AS parent_skip_tree_depth,
                cs1.p1_linear_depth AS parent_p1_linear_depth,
                cs1.subtree_source_depth AS parent_subtree_source_depth
            FROM csp
            INNER JOIN commit_graph_merge_parents cgmp ON csp.id = cgmp.id
            INNER JOIN commit_graph_edges cs0 ON cs0.id = cgmp.id
            INNER JOIN commit_graph_edges cs1 ON cs1.id = cgmp.parent
            WHERE cs0.parent_count >= 2

            UNION

            SELECT
                cs0.cs_id AS cs_id,
                csp.origin_cs_id AS origin_cs_id,
                NULL AS gen,
                NULL as subtree_source_gen,
                NULL AS skip_tree_depth,
                NULL AS p1_linear_depth,
                NULL AS subtree_source_depth,
                NULL AS parent_count,
                NULL as subtree_source_count,
                NULL AS merge_ancestor,
                NULL AS merge_ancestor_gen,
                NULL AS merge_ancestor_subtree_source_gen, 
                NULL AS merge_ancestor_skip_tree_depth,
                NULL AS merge_ancestor_p1_linear_depth,
                NULL AS merge_ancestor_subtree_source_depth,
                NULL AS skip_tree_parent,
                NULL AS skip_tree_parent_gen,
                NULL AS skip_tree_parent_subtree_source_gen,
                NULL AS skip_tree_parent_skip_tree_depth,
                NULL AS skip_tree_parent_p1_linear_depth,
                NULL AS skip_tree_parent_subtree_source_depth,
                NULL AS skip_tree_skew_ancestor,
                NULL AS skip_tree_skew_ancestor_gen,
                NULL AS skip_tree_skew_ancestor_subtree_source_gen,
                NULL AS skip_tree_skew_ancestor_skip_tree_depth,
                NULL AS skip_tree_skew_ancestor_p1_linear_depth,
                NULL AS skip_tree_skew_ancestor_subtree_source_depth,
                NULL AS p1_linear_skew_ancestor,
                NULL AS p1_linear_skew_ancestor_gen,
                NULL AS p1_linear_skew_ancestor_subtree_source_gen,
                NULL AS p1_linear_skew_ancestor_skip_tree_depth,
                NULL AS p1_linear_skew_ancestor_p1_linear_depth,
                NULL AS p1_linear_skew_ancestor_subtree_source_depth,
                NULL AS subtree_or_merge_ancestor,
                NULL AS subtree_or_merge_ancestor_gen,
                NULL AS subtree_or_merge_ancestor_subtree_source_gen,
                NULL AS subtree_or_merge_ancestor_skip_tree_depth,
                NULL AS subtree_or_merge_ancestor_p1_linear_depth,
                NULL AS subtree_or_merge_ancestor_subtree_source_depth,
                NULL AS subtree_source_parent,
                NULL AS subtree_source_parent_gen,
                NULL AS subtree_source_parent_subtree_source_gen,
                NULL AS subtree_source_parent_skip_tree_depth,
                NULL AS subtree_source_parent_p1_linear_depth,
                NULL AS subtree_source_parent_subtree_source_depth,
                NULL AS subtree_source_skew_ancestor,
                NULL AS subtree_source_skew_ancestor_gen,
                NULL AS subtree_source_skew_ancestor_subtree_source_gen,
                NULL AS subtree_source_skew_ancestor_skip_tree_depth,
                NULL AS subtree_source_skew_ancestor_p1_linear_depth,
                NULL AS subtree_source_skew_ancestor_subtree_source_depth,
                NULL AS parent_num,
                cgss.subtree_source_num AS subtree_source_num,
                cs1.cs_id AS parent,
                cs1.gen AS parent_gen,
                cs1.subtree_source_gen AS parent_subtree_source_gen,
                cs1.skip_tree_depth AS parent_skip_tree_depth,
                cs1.p1_linear_depth AS parent_p1_linear_depth,
                cs1.subtree_source_depth AS parent_subtree_source_depth
            FROM csp
            INNER JOIN commit_graph_subtree_sources cgss ON csp.id = cgss.id
            INNER JOIN commit_graph_edges cs0 ON cs0.id = cgss.id
            INNER JOIN commit_graph_edges cs1 ON cs1.id = cgss.subtree_source
            WHERE cs0.subtree_source_count >= 1

            UNION

            SELECT
                cs0.cs_id AS cs_id,
                csp.origin_cs_id AS origin_cs_id,
                cs0.gen AS gen,
                cs0.subtree_source_gen AS subtree_source_gen,
                cs0.skip_tree_depth AS skip_tree_depth,
                cs0.p1_linear_depth AS p1_linear_depth,
                cs0.subtree_source_depth AS subtree_source_depth,
                cs0.parent_count AS parent_count,
                cs0.subtree_source_count AS subtree_source_count,
                cs_merge_ancestor.cs_id AS merge_ancestor,
                cs_merge_ancestor.gen AS merge_ancestor_gen,
                cs_merge_ancestor.subtree_source_gen AS merge_ancestor_subtree_source_gen, 
                cs_merge_ancestor.skip_tree_depth AS merge_ancestor_skip_tree_depth,
                cs_merge_ancestor.p1_linear_depth AS merge_ancestor_p1_linear_depth,
                cs_merge_ancestor.subtree_source_depth AS merge_ancestor_subtree_source_depth,
                cs_skip_tree_parent.cs_id AS skip_tree_parent,
                cs_skip_tree_parent.gen AS skip_tree_parent_gen,
                cs_skip_tree_parent.subtree_source_gen AS skip_tree_parent_subtree_source_gen,
                cs_skip_tree_parent.skip_tree_depth AS skip_tree_parent_skip_tree_depth,
                cs_skip_tree_parent.p1_linear_depth AS skip_tree_parent_p1_linear_depth,
                cs_skip_tree_parent.subtree_source_depth AS skip_tree_parent_subtree_source_depth,
                cs_skip_tree_skew_ancestor.cs_id AS skip_tree_skew_ancestor,
                cs_skip_tree_skew_ancestor.gen AS skip_tree_skew_ancestor_gen,
                cs_skip_tree_skew_ancestor.subtree_source_gen AS skip_tree_skew_ancestor_subtree_source_gen,
                cs_skip_tree_skew_ancestor.skip_tree_depth AS skip_tree_skew_ancestor_skip_tree_depth,
                cs_skip_tree_skew_ancestor.p1_linear_depth AS skip_tree_skew_ancestor_p1_linear_depth,
                cs_skip_tree_skew_ancestor.subtree_source_depth AS skip_tree_skew_ancestor_subtree_source_depth,
                cs_p1_linear_skew_ancestor.cs_id AS p1_linear_skew_ancestor,
                cs_p1_linear_skew_ancestor.gen AS p1_linear_skew_ancestor_gen,
                cs_p1_linear_skew_ancestor.subtree_source_gen AS p1_linear_skew_ancestor_subtree_source_gen,
                cs_p1_linear_skew_ancestor.skip_tree_depth AS p1_linear_skew_ancestor_skip_tree_depth,
                cs_p1_linear_skew_ancestor.p1_linear_depth AS p1_linear_skew_ancestor_p1_linear_depth,
                cs_p1_linear_skew_ancestor.subtree_source_depth AS p1_linear_skew_ancestor_subtree_source_depth,
                cs_subtree_or_merge_ancestor.cs_id AS subtree_or_merge_ancestor,
                cs_subtree_or_merge_ancestor.gen AS subtree_or_merge_ancestor_gen,
                cs_subtree_or_merge_ancestor.subtree_source_gen AS subtree_or_merge_ancestor_subtree_source_gen,
                cs_subtree_or_merge_ancestor.skip_tree_depth AS subtree_or_merge_ancestor_skip_tree_depth,
                cs_subtree_or_merge_ancestor.p1_linear_depth AS subtree_or_merge_ancestor_p1_linear_depth,
                cs_subtree_or_merge_ancestor.subtree_source_depth AS subtree_or_merge_ancestor_subtree_source_depth,
                cs_subtree_source_parent.cs_id AS subtree_source_parent,
                cs_subtree_source_parent.gen AS subtree_source_parent_gen,
                cs_subtree_source_parent.subtree_source_gen AS subtree_source_parent_subtree_source_gen,
                cs_subtree_source_parent.skip_tree_depth AS subtree_source_parent_skip_tree_depth,
                cs_subtree_source_parent.p1_linear_depth AS subtree_source_parent_p1_linear_depth,
                cs_subtree_source_parent.subtree_source_depth AS subtree_source_parent_subtree_source_depth,
                cs_subtree_source_skew_ancestor.cs_id AS subtree_source_skew_ancestor,
                cs_subtree_source_skew_ancestor.gen AS subtree_source_skew_ancestor_gen,
                cs_subtree_source_skew_ancestor.subtree_source_gen AS subtree_source_skew_ancestor_subtree_source_gen,
                cs_subtree_source_skew_ancestor.skip_tree_depth AS subtree_source_skew_ancestor_skip_tree_depth,
                cs_subtree_source_skew_ancestor.p1_linear_depth AS subtree_source_skew_ancestor_p1_linear_depth,
                cs_subtree_source_skew_ancestor.subtree_source_depth AS subtree_source_skew_ancestor_subtree_source_depth,
                0 AS parent_num,
                NULL AS subtree_source_num,
                cs_p1_parent.cs_id AS parent,
                cs_p1_parent.gen AS parent_gen,
                cs_p1_parent.subtree_source_gen AS parent_subtree_source_gen,
                cs_p1_parent.skip_tree_depth AS parent_skip_tree_depth,
                cs_p1_parent.p1_linear_depth AS parent_p1_linear_depth,
                cs_p1_parent.subtree_source_depth AS parent_subtree_source_depth
            FROM csp
            INNER JOIN commit_graph_edges cs0 ON csp.id = cs0.id
            LEFT JOIN commit_graph_edges cs_p1_parent ON cs_p1_parent.id = cs0.p1_parent
            LEFT JOIN commit_graph_edges cs_merge_ancestor ON cs_merge_ancestor.id = cs0.merge_ancestor
            LEFT JOIN commit_graph_edges cs_skip_tree_parent ON cs_skip_tree_parent.id = cs0.skip_tree_parent
            LEFT JOIN commit_graph_edges cs_skip_tree_skew_ancestor ON cs_skip_tree_skew_ancestor.id = cs0.skip_tree_skew_ancestor
            LEFT JOIN commit_graph_edges cs_p1_linear_skew_ancestor ON cs_p1_linear_skew_ancestor.id = cs0.p1_linear_skew_ancestor
            LEFT JOIN commit_graph_edges cs_subtree_or_merge_ancestor ON cs_subtree_or_merge_ancestor.id = cs0.subtree_or_merge_ancestor
            LEFT JOIN commit_graph_edges cs_subtree_source_parent ON cs_subtree_source_parent.id = cs0.subtree_source_parent
            LEFT JOIN commit_graph_edges cs_subtree_source_skew_ancestor ON cs_subtree_source_skew_ancestor.id = cs0.subtree_source_skew_ancestor
            ORDER BY subtree_source_num, parent_num ASC
            "
        )
    }
}

mononoke_queries! {
    write InsertChangeset(
        repo_id: RepositoryId,
        cs_id: ChangesetId,
        r#gen: u64,
        subtree_source_gen: Option<u64>,
        skip_tree_depth: u64,
        p1_linear_depth: u64,
        subtree_source_depth: Option<u64>,
        parent_count: usize,
        subtree_source_count: usize,
        p1_parent: Option<ChangesetId>,
        merge_ancestor: Option<ChangesetId>,
        skip_tree_parent: Option<ChangesetId>,
        skip_tree_skew_ancestor: Option<ChangesetId>,
        p1_linear_skew_ancestor: Option<ChangesetId>,
        subtree_or_merge_ancestor: Option<ChangesetId>,
        subtree_source_parent: Option<ChangesetId>,
        subtree_source_skew_ancestor: Option<ChangesetId>,
    ) {
        insert_or_ignore,
        "
        {insert_or_ignore} INTO commit_graph_edges (
            repo_id,
            cs_id,
            gen,
            subtree_source_gen,
            skip_tree_depth,
            p1_linear_depth,
            subtree_source_depth,
            parent_count,
            subtree_source_count,
            p1_parent,
            merge_ancestor,
            skip_tree_parent,
            skip_tree_skew_ancestor,
            p1_linear_skew_ancestor,
            subtree_or_merge_ancestor,
            subtree_source_parent,
            subtree_source_skew_ancestor
        ) VALUES (
            {repo_id},
            {cs_id},
            {gen},
            {subtree_source_gen},
            {skip_tree_depth},
            {p1_linear_depth},
            {subtree_source_depth},
            {parent_count},
            {subtree_source_count},
            (SELECT cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id = {p1_parent}),
            (SELECT cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id = {merge_ancestor}),
            (SELECT cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id = {skip_tree_parent}),
            (SELECT cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id = {skip_tree_skew_ancestor}),
            (SELECT cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id = {p1_linear_skew_ancestor}),
            (SELECT cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id = {subtree_or_merge_ancestor}),
            (SELECT cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id = {subtree_source_parent}),
            (SELECT cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id = {subtree_source_skew_ancestor})
        )
        "
    }

    write InsertChangesetsNoEdges(values: (
        repo_id: RepositoryId,
        cs_id: ChangesetId,
        r#gen: u64,
        subtree_source_gen: Option<u64>,
        skip_tree_depth: u64,
        p1_linear_depth: u64,
        subtree_source_depth: Option<u64>,
        parent_count: usize,
        subtree_source_count: usize,
    )) {
        insert_or_ignore,
        "
        {insert_or_ignore} INTO commit_graph_edges (
            repo_id,
            cs_id,
            gen,
            subtree_source_gen,
            skip_tree_depth,
            p1_linear_depth,
            subtree_source_depth,
            parent_count,
            subtree_source_count
        ) VALUES {values}
        "
    }

    // Fix edges for changesets previously added with InsertChangesetsNoEdges
    write FixEdges(values: (
        repo_id: RepositoryId,
        cs_id: ChangesetId,
        // We need the depths otherwise we get an error on sqlite. Though this won't be used because we
        // always replace the edges only.
        r#gen: u64,
        subtree_source_gen: Option<u64>,
        skip_tree_depth: u64,
        p1_linear_depth: u64,
        subtree_source_depth: Option<u64>,
        parent_count: usize,
        subtree_source_count: usize,
        p1_parent: Option<u64>,
        merge_ancestor: Option<u64>,
        skip_tree_parent: Option<u64>,
        skip_tree_skew_ancestor: Option<u64>,
        p1_linear_skew_ancestor: Option<u64>,
        subtree_or_merge_ancestor: Option<u64>,
        subtree_source_parent: Option<u64>,
        subtree_source_skew_ancestor: Option<u64>,
    )) {
        none,
        mysql("INSERT INTO commit_graph_edges
            (repo_id, cs_id, gen, subtree_source_gen, skip_tree_depth, p1_linear_depth, subtree_source_depth, parent_count, subtree_source_count,
                p1_parent, merge_ancestor, skip_tree_parent, skip_tree_skew_ancestor, p1_linear_skew_ancestor,
                subtree_or_merge_ancestor, subtree_source_parent, subtree_source_skew_ancestor)
        VALUES {values}
        ON DUPLICATE KEY UPDATE
            p1_parent = VALUES(p1_parent),
            merge_ancestor = VALUES(merge_ancestor),
            skip_tree_parent = VALUES(skip_tree_parent),
            skip_tree_skew_ancestor = VALUES(skip_tree_skew_ancestor),
            p1_linear_skew_ancestor = VALUES(p1_linear_skew_ancestor),
            subtree_or_merge_ancestor = VALUES(subtree_or_merge_ancestor),
            subtree_source_parent = VALUES(subtree_source_parent),
            subtree_source_skew_ancestor = VALUES(subtree_source_skew_ancestor)")
        sqlite("INSERT INTO commit_graph_edges
            (repo_id, cs_id, gen, subtree_source_gen, skip_tree_depth, p1_linear_depth, subtree_source_depth, parent_count, subtree_source_count,
                p1_parent, merge_ancestor, skip_tree_parent, skip_tree_skew_ancestor, p1_linear_skew_ancestor,
                subtree_or_merge_ancestor, subtree_source_parent, subtree_source_skew_ancestor)
        VALUES {values}
        ON CONFLICT(repo_id, cs_id) DO UPDATE SET
            p1_parent = excluded.p1_parent,
            merge_ancestor = excluded.merge_ancestor,
            skip_tree_parent = excluded.skip_tree_parent,
            skip_tree_skew_ancestor = excluded.skip_tree_skew_ancestor,
            p1_linear_skew_ancestor = excluded.p1_linear_skew_ancestor,
            subtree_or_merge_ancestor = excluded.subtree_or_merge_ancestor,
            subtree_source_parent = excluded.subtree_source_parent,
            subtree_source_skew_ancestor = excluded.subtree_source_skew_ancestor")
    }

    read SelectManyIds(repo_id: RepositoryId, >list cs_ids: ChangesetId) -> (ChangesetId, u64) {
        "SELECT cs.cs_id, cs.id FROM commit_graph_edges cs WHERE cs.repo_id = {repo_id} AND cs.cs_id IN {cs_ids}"
    }

    write InsertMergeParents(values: (id: u64, parent_num: usize, parent: u64)) {
        insert_or_ignore,
        "{insert_or_ignore} INTO commit_graph_merge_parents (id, parent_num, parent) VALUES {values}"
    }

    write InsertSubtreeSources(values: (id: u64, subtree_source_num: usize, subtree_source: u64)) {
        insert_or_ignore,
        "{insert_or_ignore} INTO commit_graph_subtree_sources (id, subtree_source_num, subtree_source) VALUES {values}"
    }

    read SelectManyChangesets(repo_id: RepositoryId, >list cs_ids: ChangesetId) -> (
        ChangesetId, // cs_id
        Option<ChangesetId>, // origin_cs_id
        Option<u64>, // gen
        Option<u64>, // subtree_source_gen
        Option<u64>, // skip_tree_depth
        Option<u64>, // p1_linear_depth
        Option<u64>, // subtree_source_depth
        Option<usize>, // parent_count
        Option<usize>, // subtree_source_count
        Option<ChangesetId>, // merge_ancestor
        Option<u64>, // merge_ancestor_gen
        Option<u64>, // merge_ancestor_subtree_source_gen
        Option<u64>, // merge_ancestor_skip_tree_depth
        Option<u64>, // merge_ancestor_p1_linear_depth
        Option<u64>, // merge_ancestor_subtree_source_depth
        Option<ChangesetId>, // skip_tree_parent
        Option<u64>, // skip_tree_parent_gen
        Option<u64>, // skip_tree_parent_subtree_source_gen
        Option<u64>, // skip_tree_parent_skip_tree_depth
        Option<u64>, // skip_tree_parent_p1_linear_depth
        Option<u64>, // skip_tree_parent_subtree_source_depth
        Option<ChangesetId>, // skip_tree_skew_ancestor
        Option<u64>, // skip_tree_skew_ancestor_gen
        Option<u64>, // skip_tree_skew_ancestor_subtree_source_gen
        Option<u64>, // skip_tree_skew_ancestor_skip_tree_depth
        Option<u64>, // skip_tree_skew_ancestor_p1_linear_depth
        Option<u64>, // skip_tree_skew_ancestor_subtree_source_depth
        Option<ChangesetId>, // p1_linear_skew_ancestor
        Option<u64>, // p1_linear_skew_ancestor_gen
        Option<u64>, // p1_linear_skew_ancestor_subtree_source_gen
        Option<u64>, // p1_linear_skew_ancestor_skip_tree_depth
        Option<u64>, // p1_linear_skew_ancestor_p1_linear_depth
        Option<u64>, // p1_linear_skew_ancestor_subtree_source_depth
        Option<ChangesetId>, // subtree_or_merge_ancestor
        Option<u64>, // subtree_or_merge_ancestor_gen
        Option<u64>, // subtree_or_merge_ancestor_subtree_source_gen
        Option<u64>, // subtree_or_merge_ancestor_skip_tree_depth
        Option<u64>, // subtree_or_merge_ancestor_p1_linear_depth
        Option<u64>, // subtree_or_merge_ancestor_subtree_source_depth
        Option<ChangesetId>, // subtree_source_parent
        Option<u64>, // subtree_source_parent_gen
        Option<u64>, // subtree_source_parent_subtree_source_gen
        Option<u64>, // subtree_source_parent_skip_tree_depth
        Option<u64>, // subtree_source_parent_p1_linear_depth
        Option<u64>, // subtree_source_parent_subtree_source_depth
        Option<ChangesetId>, // subtree_source_skew_ancestor
        Option<u64>, // subtree_source_skew_ancestor_gen
        Option<u64>, // subtree_source_skew_ancestor_subtree_source_gen
        Option<u64>, // subtree_source_skew_ancestor_skip_tree_depth
        Option<u64>, // subtree_source_skew_ancestor_p1_linear_depth
        Option<u64>, // subtree_source_skew_ancestor_subtree_source_depth
        Option<usize>, // parent_num
        Option<usize>, // subtree_source_num
        Option<ChangesetId>, // parent
        Option<u64>, // parent_gen
        Option<u64>, // parent_subtree_source_gen
        Option<u64>, // parent_skip_tree_depth
        Option<u64>, // parent_p1_linear_depth
        Option<u64>, // parent_subtree_source_depth
    ) {
        fetch_commit_graph_edges!(
            "WITH csp AS (
                SELECT cge.id, NULL AS origin_cs_id
                FROM commit_graph_edges cge
                WHERE cge.repo_id = {repo_id} AND cge.cs_id IN {cs_ids}
            )"
        )
    }

    read SelectManyChangesetsWithFirstParentPrefetch(repo_id: RepositoryId, step_limit: u64, prefetch_gen: u64, >list cs_ids: ChangesetId) -> (
        ChangesetId, // cs_id
        Option<ChangesetId>, // origin_cs_id
        Option<u64>, // gen
        Option<u64>, // subtree_source_gen
        Option<u64>, // skip_tree_depth
        Option<u64>, // p1_linear_depth
        Option<u64>, // subtree_source_depth
        Option<usize>, // parent_count
        Option<usize>, // subtree_source_count
        Option<ChangesetId>, // merge_ancestor
        Option<u64>, // merge_ancestor_gen
        Option<u64>, // merge_ancestor_subtree_source_gen
        Option<u64>, // merge_ancestor_skip_tree_depth
        Option<u64>, // merge_ancestor_p1_linear_depth
        Option<u64>, // merge_ancestor_subtree_source_depth
        Option<ChangesetId>, // skip_tree_parent
        Option<u64>, // skip_tree_parent_gen
        Option<u64>, // skip_tree_parent_subtree_source_gen
        Option<u64>, // skip_tree_parent_skip_tree_depth
        Option<u64>, // skip_tree_parent_p1_linear_depth
        Option<u64>, // skip_tree_parent_subtree_source_depth
        Option<ChangesetId>, // skip_tree_skew_ancestor
        Option<u64>, // skip_tree_skew_ancestor_gen
        Option<u64>, // skip_tree_skew_ancestor_subtree_source_gen
        Option<u64>, // skip_tree_skew_ancestor_skip_tree_depth
        Option<u64>, // skip_tree_skew_ancestor_p1_linear_depth
        Option<u64>, // skip_tree_skew_ancestor_subtree_source_depth
        Option<ChangesetId>, // p1_linear_skew_ancestor
        Option<u64>, // p1_linear_skew_ancestor_gen
        Option<u64>, // p1_linear_skew_ancestor_subtree_source_gen
        Option<u64>, // p1_linear_skew_ancestor_skip_tree_depth
        Option<u64>, // p1_linear_skew_ancestor_p1_linear_depth
        Option<u64>, // p1_linear_skew_ancestor_subtree_source_depth
        Option<ChangesetId>, // subtree_or_merge_ancestor
        Option<u64>, // subtree_or_merge_ancestor_gen
        Option<u64>, // subtree_or_merge_ancestor_subtree_source_gen
        Option<u64>, // subtree_or_merge_ancestor_skip_tree_depth
        Option<u64>, // subtree_or_merge_ancestor_p1_linear_depth
        Option<u64>, // subtree_or_merge_ancestor_subtree_source_depth
        Option<ChangesetId>, // subtree_source_parent
        Option<u64>, // subtree_source_parent_gen
        Option<u64>, // subtree_source_parent_subtree_source_gen
        Option<u64>, // subtree_source_parent_skip_tree_depth
        Option<u64>, // subtree_source_parent_p1_linear_depth
        Option<u64>, // subtree_source_parent_subtree_source_depth
        Option<ChangesetId>, // subtree_source_skew_ancestor
        Option<u64>, // subtree_source_skew_ancestor_gen
        Option<u64>, // subtree_source_skew_ancestor_subtree_source_gen
        Option<u64>, // subtree_source_skew_ancestor_skip_tree_depth
        Option<u64>, // subtree_source_skew_ancestor_p1_linear_depth
        Option<u64>, // subtree_source_skew_ancestor_subtree_source_depth
        Option<usize>, // parent_num
        Option<usize>, // subtree_source_num
        Option<ChangesetId>, // parent
        Option<u64>, // parent_gen
        Option<u64>, // parent_subtree_source_gen
        Option<u64>, // parent_skip_tree_depth
        Option<u64>, // parent_p1_linear_depth
        Option<u64>, // parent_subtree_source_depth
    ) {
        fetch_commit_graph_edges!(
            "WITH RECURSIVE csp AS (
                SELECT
                    cs.id, cs.cs_id AS origin_cs_id, 1 AS step, cs.p1_parent AS next
                FROM commit_graph_edges cs
                WHERE cs.repo_id = {repo_id} AND cs.cs_id IN {cs_ids}
                UNION ALL
                SELECT
                    cs.id, csp.origin_cs_id AS origin_cs_id, csp.step + 1, cs.p1_parent AS next
                FROM csp
                INNER JOIN commit_graph_edges cs ON cs.id = csp.next
                WHERE csp.step < {step_limit} AND cs.gen >= {prefetch_gen}
            )"
        )
    }

    read SelectManyChangesetsWithExactSkipTreeAncestorPrefetch(repo_id: RepositoryId, prefetch_gen: u64, >list cs_ids: ChangesetId) -> (
        ChangesetId, // cs_id
        Option<ChangesetId>, // origin_cs_id
        Option<u64>, // gen
        Option<u64>, // subtree_source_gen
        Option<u64>, // skip_tree_depth
        Option<u64>, // p1_linear_depth
        Option<u64>, // subtree_source_depth
        Option<usize>, // parent_count
        Option<usize>, // subtree_source_count
        Option<ChangesetId>, // merge_ancestor
        Option<u64>, // merge_ancestor_gen
        Option<u64>, // merge_ancestor_subtree_source_gen
        Option<u64>, // merge_ancestor_skip_tree_depth
        Option<u64>, // merge_ancestor_p1_linear_depth
        Option<u64>, // merge_ancestor_subtree_source_depth
        Option<ChangesetId>, // skip_tree_parent
        Option<u64>, // skip_tree_parent_gen
        Option<u64>, // skip_tree_parent_subtree_source_gen
        Option<u64>, // skip_tree_parent_skip_tree_depth
        Option<u64>, // skip_tree_parent_p1_linear_depth
        Option<u64>, // skip_tree_parent_subtree_source_depth
        Option<ChangesetId>, // skip_tree_skew_ancestor
        Option<u64>, // skip_tree_skew_ancestor_gen
        Option<u64>, // skip_tree_skew_ancestor_subtree_source_gen
        Option<u64>, // skip_tree_skew_ancestor_skip_tree_depth
        Option<u64>, // skip_tree_skew_ancestor_p1_linear_depth
        Option<u64>, // skip_tree_skew_ancestor_subtree_source_depth
        Option<ChangesetId>, // p1_linear_skew_ancestor
        Option<u64>, // p1_linear_skew_ancestor_gen
        Option<u64>, // p1_linear_skew_ancestor_subtree_source_gen
        Option<u64>, // p1_linear_skew_ancestor_skip_tree_depth
        Option<u64>, // p1_linear_skew_ancestor_p1_linear_depth
        Option<u64>, // p1_linear_skew_ancestor_subtree_source_depth
        Option<ChangesetId>, // subtree_or_merge_ancestor
        Option<u64>, // subtree_or_merge_ancestor_gen
        Option<u64>, // subtree_or_merge_ancestor_subtree_source_gen
        Option<u64>, // subtree_or_merge_ancestor_skip_tree_depth
        Option<u64>, // subtree_or_merge_ancestor_p1_linear_depth
        Option<u64>, // subtree_or_merge_ancestor_subtree_source_depth
        Option<ChangesetId>, // subtree_source_parent
        Option<u64>, // subtree_source_parent_gen
        Option<u64>, // subtree_source_parent_subtree_source_gen
        Option<u64>, // subtree_source_parent_skip_tree_depth
        Option<u64>, // subtree_source_parent_p1_linear_depth
        Option<u64>, // subtree_source_parent_subtree_source_depth
        Option<ChangesetId>, // subtree_source_skew_ancestor
        Option<u64>, // subtree_source_skew_ancestor_gen
        Option<u64>, // subtree_source_skew_ancestor_subtree_source_gen
        Option<u64>, // subtree_source_skew_ancestor_skip_tree_depth
        Option<u64>, // subtree_source_skew_ancestor_p1_linear_depth
        Option<u64>, // subtree_source_skew_ancestor_subtree_source_depth
        Option<usize>, // parent_num
        Option<usize>, // subtree_source_num
        Option<ChangesetId>, // parent
        Option<u64>, // parent_gen
        Option<u64>, // parent_subtree_source_gen
        Option<u64>, // parent_skip_tree_depth
        Option<u64>, // parent_p1_linear_depth
        Option<u64>, // parent_subtree_source_depth
    ) {
        fetch_commit_graph_edges!(
            "WITH RECURSIVE csp AS (
                SELECT
                    cs.cs_id as origin_cs_id, cs.id, cs.skip_tree_parent, cs.skip_tree_skew_ancestor
                FROM commit_graph_edges cs
                WHERE cs.repo_id = {repo_id} AND cs.cs_id IN {cs_ids}

                UNION ALL

                SELECT
                    csp.origin_cs_id, skip_tree_parent.id, skip_tree_parent.skip_tree_parent, skip_tree_parent.skip_tree_skew_ancestor
                FROM csp
                INNER JOIN commit_graph_edges skip_tree_parent ON skip_tree_parent.id = csp.skip_tree_parent
                INNER JOIN commit_graph_edges skip_tree_skew_ancestor ON skip_tree_skew_ancestor.id = csp.skip_tree_skew_ancestor
                WHERE skip_tree_parent.gen >= {prefetch_gen} and skip_tree_skew_ancestor.gen < {prefetch_gen}

                UNION ALL

                SELECT
                    csp.origin_cs_id, skip_tree_skew_ancestor.id, skip_tree_skew_ancestor.skip_tree_parent, skip_tree_skew_ancestor.skip_tree_skew_ancestor
                FROM csp
                INNER JOIN commit_graph_edges skip_tree_skew_ancestor ON skip_tree_skew_ancestor.id = csp.skip_tree_skew_ancestor
                WHERE skip_tree_skew_ancestor.gen >= {prefetch_gen}
            )"
        )
    }

    // The only difference between mysql and sqlite is the FORCE INDEX
    read SelectManyChangesetsInIdRange(repo_id: RepositoryId, start_id: u64, end_id: u64, limit: u64) -> (
        ChangesetId, // cs_id
        Option<ChangesetId>, // origin_cs_id
        Option<u64>, // gen
        Option<u64>, // subtree_source_gen
        Option<u64>, // skip_tree_depth
        Option<u64>, // p1_linear_depth
        Option<u64>, // subtree_source_depth
        Option<usize>, // parent_count
        Option<usize>, // subtree_source_count
        Option<ChangesetId>, // merge_ancestor
        Option<u64>, // merge_ancestor_gen
        Option<u64>, // merge_ancestor_subtree_source_gen
        Option<u64>, // merge_ancestor_skip_tree_depth
        Option<u64>, // merge_ancestor_p1_linear_depth
        Option<u64>, // merge_ancestor_subtree_source_depth
        Option<ChangesetId>, // skip_tree_parent
        Option<u64>, // skip_tree_parent_gen
        Option<u64>, // skip_tree_parent_subtree_source_gen
        Option<u64>, // skip_tree_parent_skip_tree_depth
        Option<u64>, // skip_tree_parent_p1_linear_depth
        Option<u64>, // skip_tree_parent_subtree_source_depth
        Option<ChangesetId>, // skip_tree_skew_ancestor
        Option<u64>, // skip_tree_skew_ancestor_gen
        Option<u64>, // skip_tree_skew_ancestor_subtree_source_gen
        Option<u64>, // skip_tree_skew_ancestor_skip_tree_depth
        Option<u64>, // skip_tree_skew_ancestor_p1_linear_depth
        Option<u64>, // skip_tree_skew_ancestor_subtree_source_depth
        Option<ChangesetId>, // p1_linear_skew_ancestor
        Option<u64>, // p1_linear_skew_ancestor_gen
        Option<u64>, // p1_linear_skew_ancestor_subtree_source_gen
        Option<u64>, // p1_linear_skew_ancestor_skip_tree_depth
        Option<u64>, // p1_linear_skew_ancestor_p1_linear_depth
        Option<u64>, // p1_linear_skew_ancestor_subtree_source_depth
        Option<ChangesetId>, // subtree_or_merge_ancestor
        Option<u64>, // subtree_or_merge_ancestor_gen
        Option<u64>, // subtree_or_merge_ancestor_subtree_source_gen
        Option<u64>, // subtree_or_merge_ancestor_skip_tree_depth
        Option<u64>, // subtree_or_merge_ancestor_p1_linear_depth
        Option<u64>, // subtree_or_merge_ancestor_subtree_source_depth
        Option<ChangesetId>, // subtree_source_parent
        Option<u64>, // subtree_source_parent_gen
        Option<u64>, // subtree_source_parent_subtree_source_gen
        Option<u64>, // subtree_source_parent_skip_tree_depth
        Option<u64>, // subtree_source_parent_p1_linear_depth
        Option<u64>, // subtree_source_parent_subtree_source_depth
        Option<ChangesetId>, // subtree_source_skew_ancestor
        Option<u64>, // subtree_source_skew_ancestor_gen
        Option<u64>, // subtree_source_skew_ancestor_subtree_source_gen
        Option<u64>, // subtree_source_skew_ancestor_skip_tree_depth
        Option<u64>, // subtree_source_skew_ancestor_p1_linear_depth
        Option<u64>, // subtree_source_skew_ancestor_subtree_source_depth
        Option<usize>, // parent_num
        Option<usize>, // subtree_source_num
        Option<ChangesetId>, // parent
        Option<u64>, // parent_gen
        Option<u64>, // parent_subtree_source_gen
        Option<u64>, // parent_skip_tree_depth
        Option<u64>, // parent_p1_linear_depth
        Option<u64>, // parent_subtree_source_depth
    ) {
        mysql(fetch_commit_graph_edges!(
            "WITH csp AS (
                SELECT cs.id, NULL AS origin_cs_id
                FROM commit_graph_edges cs FORCE INDEX(repo_id_id)
                WHERE cs.repo_id = {repo_id} AND cs.id >= {start_id} AND cs.id <= {end_id}
                ORDER BY cs.id ASC
                LIMIT {limit}
            )"
        ))
        sqlite(fetch_commit_graph_edges!(
            "WITH csp AS (
                SELECT cs.id, NULL AS origin_cs_id
                FROM commit_graph_edges cs
                WHERE cs.repo_id = {repo_id} AND cs.id >= {start_id} AND cs.id <= {end_id}
                ORDER BY cs.id ASC
                LIMIT {limit}
            )"
        ))
    }

    // The only difference between mysql and sqlite is the FORCE INDEX
    read SelectManyChangesetsIdsInIdRange(repo_id: RepositoryId, start_id: u64, end_id: u64, limit: u64) -> (ChangesetId) {
        mysql("SELECT cs.cs_id
        FROM commit_graph_edges cs FORCE INDEX(repo_id_id)
        WHERE cs.repo_id = {repo_id} AND cs.id >= {start_id} AND cs.id <= {end_id}
        ORDER BY cs.id ASC
        LIMIT {limit}")
        sqlite("SELECT cs.cs_id
        FROM commit_graph_edges cs
        WHERE cs.repo_id = {repo_id} AND cs.id >= {start_id} AND cs.id <= {end_id}
        ORDER BY cs.id ASC
        LIMIT {limit}")
    }

    // The only difference between mysql and sqlite is the FORCE INDEX
    read SelectMaxIdInRange(repo_id: RepositoryId, start_id: u64, end_id: u64, limit: u64) -> (u64) {
        mysql("SELECT MAX(id)
        FROM (
            SELECT id
            FROM commit_graph_edges FORCE INDEX(repo_id_id)
            WHERE repo_id = {repo_id} AND id >= {start_id} AND id <= {end_id}
            ORDER BY id ASC
            LIMIT {limit}
        ) AS ids")
        sqlite("SELECT MAX(id)
        FROM (
            SELECT id
            FROM commit_graph_edges
            WHERE repo_id = {repo_id} AND id >= {start_id} AND id <= {end_id}
            ORDER BY id ASC
            LIMIT {limit}
        ) AS ids")
    }

    read SelectMaxId(repo_id: RepositoryId) -> (u64) {
        "
        SELECT CAST(COALESCE(MAX(id), 0) AS UNSIGNED)
        FROM commit_graph_edges
        WHERE repo_id = {repo_id}
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

    read SelectChildren(repo_id: RepositoryId, cs_id: ChangesetId) -> (ChangesetId) {
        "
        SELECT
            cs.cs_id
        FROM commit_graph_edges cs
        INNER JOIN commit_graph_edges cs_p1_parent
            ON cs_p1_parent.id = cs.p1_parent
        WHERE
            cs_p1_parent.repo_id = {repo_id}
            AND cs_p1_parent.cs_id = {cs_id}

        UNION

        SELECT
            cs.cs_id
        FROM commit_graph_edges cs
        INNER JOIN commit_graph_merge_parents cgmp
            ON cgmp.id = cs.id
        INNER JOIN commit_graph_edges cs_merge_parent
            ON cgmp.parent = cs_merge_parent.id
        WHERE
            cs_merge_parent.repo_id = {repo_id}
            AND cs_merge_parent.cs_id
                = {cs_id};
        "
    }

    read SelectChangesetsIdsBounds(repo_id: RepositoryId) -> (u64, u64) {
        "SELECT min(id), max(id)
         FROM commit_graph_edges
         WHERE repo_id = {repo_id}"
    }

    read SelectOldestChangesetsIdsInRange(repo_id: RepositoryId, lower_bound: u64, upper_bound: u64, limit: u64) -> (ChangesetId, u64) {
        mysql(
            "SELECT cs_id, id
            FROM commit_graph_edges FORCE INDEX(repo_id_id)
            WHERE repo_id = {repo_id}
              AND {lower_bound} <= id AND id < {upper_bound}
            ORDER BY id ASC
            LIMIT {limit}"
        )
        sqlite(
            "SELECT cs_id, id
            FROM commit_graph_edges
            WHERE repo_id = {repo_id}
              AND {lower_bound} <= id AND id < {upper_bound}
            ORDER BY id ASC
            LIMIT {limit}"
        )
    }

    read SelectNewestChangesetsIdsInRange(repo_id: RepositoryId, lower_bound: u64, upper_bound: u64, limit: u64) -> (ChangesetId, u64) {
        mysql(
            "SELECT cs_id, id
            FROM commit_graph_edges FORCE INDEX(repo_id_id)
            WHERE repo_id = {repo_id}
              AND {lower_bound} <= id AND id < {upper_bound}
            ORDER BY id DESC
            LIMIT {limit}"
        )
        sqlite(
            "SELECT cs_id, id
            FROM commit_graph_edges
            WHERE repo_id = {repo_id}
              AND {lower_bound} <= id AND id < {upper_bound}
            ORDER BY id DESC
            LIMIT {limit}"
        )
    }

    read GetCommitCount(id: RepositoryId) -> (u64) {
        "SELECT COUNT(*) FROM commit_graph_edges WHERE repo_id={id}"
    }
}

type FetchedEdgesRow = (
    ChangesetId,         // cs_id
    Option<ChangesetId>, // origin_cs_id
    Option<u64>,         // gen
    Option<u64>,         // subtree_source_gen
    Option<u64>,         // skip_tree_depth
    Option<u64>,         // p1_linear_depth
    Option<u64>,         // subtree_source_depth
    Option<usize>,       // parent_count
    Option<usize>,       // subtree_source_count
    Option<ChangesetId>, // merge_ancestor
    Option<u64>,         // merge_ancestor_gen
    Option<u64>,         // merge_ancestor_subtree_source_gen
    Option<u64>,         // merge_ancestor_skip_tree_depth
    Option<u64>,         // merge_ancestor_p1_linear_depth
    Option<u64>,         // merge_ancestor_subtree_source_depth
    Option<ChangesetId>, // skip_tree_parent
    Option<u64>,         // skip_tree_parent_gen
    Option<u64>,         // skip_tree_parent_subtree_source_gen
    Option<u64>,         // skip_tree_parent_skip_tree_depth
    Option<u64>,         // skip_tree_parent_p1_linear_depth
    Option<u64>,         // skip_tree_parent_subtree_source_depth
    Option<ChangesetId>, // skip_tree_skew_ancestor
    Option<u64>,         // skip_tree_skew_ancestor_gen
    Option<u64>,         // skip_tree_skew_ancestor_subtree_source_gen
    Option<u64>,         // skip_tree_skew_ancestor_skip_tree_depth
    Option<u64>,         // skip_tree_skew_ancestor_p1_linear_depth
    Option<u64>,         // skip_tree_skew_ancestor_subtree_source_depth
    Option<ChangesetId>, // p1_linear_skew_ancestor
    Option<u64>,         // p1_linear_skew_ancestor_gen
    Option<u64>,         // p1_linear_skew_ancestor_subtree_source_gen
    Option<u64>,         // p1_linear_skew_ancestor_skip_tree_depth
    Option<u64>,         // p1_linear_skew_ancestor_p1_linear_depth
    Option<u64>,         // p1_linear_skew_ancestor_subtree_source_depth
    Option<ChangesetId>, // subtree_or_merge_ancestor
    Option<u64>,         // subtree_or_merge_ancestor_gen
    Option<u64>,         // subtree_or_merge_ancestor_subtree_source_gen
    Option<u64>,         // subtree_or_merge_ancestor_skip_tree_depth
    Option<u64>,         // subtree_or_merge_ancestor_p1_linear_depth
    Option<u64>,         // subtree_or_merge_ancestor_subtree_source_depth
    Option<ChangesetId>, // subtree_source_parent
    Option<u64>,         // subtree_source_parent_gen
    Option<u64>,         // subtree_source_parent_subtree_source_gen
    Option<u64>,         // subtree_source_parent_skip_tree_depth
    Option<u64>,         // subtree_source_parent_p1_linear_depth
    Option<u64>,         // subtree_source_parent_subtree_source_depth
    Option<ChangesetId>, // subtree_source_skew_ancestor
    Option<u64>,         // subtree_source_skew_ancestor_gen
    Option<u64>,         // subtree_source_skew_ancestor_subtree_source_gen
    Option<u64>,         // subtree_source_skew_ancestor_skip_tree_depth
    Option<u64>,         // subtree_source_skew_ancestor_p1_linear_depth
    Option<u64>,         // subtree_source_skew_ancestor_subtree_source_depth
    Option<usize>,       // parent_num
    Option<usize>,       // subtree_source_num
    Option<ChangesetId>, // parent
    Option<u64>,         // parent_gen
    Option<u64>,         // parent_subtree_source_gen
    Option<u64>,         // parent_skip_tree_depth
    Option<u64>,         // parent_p1_linear_depth
    Option<u64>,         // parent_subtree_source_depth
);

impl SqlCommitGraphStorage {
    fn collect_changeset_edges_impl(
        fetched_rows: &[FetchedEdgesRow],
    ) -> HashMap<(ChangesetId, Option<ChangesetId>), FetchedChangesetEdges> {
        let option_fields_to_option_node =
            |cs_id,
             r#gen,
             subtree_source_gen: Option<u64>,
             skip_tree_depth,
             p1_linear_depth,
             subtree_source_depth: Option<u64>| match (
                cs_id,
                r#gen,
                skip_tree_depth,
                p1_linear_depth,
            ) {
                (Some(cs_id), Some(r#gen), Some(skip_tree_depth), Some(p1_linear_depth)) => {
                    let subtree_source_depth = subtree_source_depth.unwrap_or(skip_tree_depth);
                    let subtree_source_gen = subtree_source_gen.unwrap_or(r#gen);
                    Some(ChangesetNode {
                        cs_id,
                        generation: Generation::new(r#gen),
                        subtree_source_generation: Generation::new(subtree_source_gen),
                        skip_tree_depth,
                        p1_linear_depth,
                        subtree_source_depth,
                    })
                }
                _ => None,
            };
        let mut cs_id_and_origin_to_edges = HashMap::new();
        for row in fetched_rows.iter() {
            match *row {
                (
                    cs_id,
                    origin_cs_id,
                    Some(r#gen),
                    subtree_source_gen,
                    Some(skip_tree_depth),
                    Some(p1_linear_depth),
                    subtree_source_depth,
                    Some(parent_count),
                    Some(subtree_source_count),
                    merge_ancestor,
                    merge_ancestor_gen,
                    merge_ancestor_subtree_source_gen,
                    merge_ancestor_skip_tree_depth,
                    merge_ancestor_p1_linear_depth,
                    merge_ancestor_subtree_source_depth,
                    skip_tree_parent,
                    skip_tree_parent_gen,
                    skip_tree_parent_subtree_source_gen,
                    skip_tree_parent_skip_tree_depth,
                    skip_tree_parent_p1_linear_depth,
                    skip_tree_parent_subtree_source_depth,
                    skip_tree_skew_ancestor,
                    skip_tree_skew_ancestor_gen,
                    skip_tree_skew_ancestor_subtree_source_gen,
                    skip_tree_skew_ancestor_skip_tree_depth,
                    skip_tree_skew_ancestor_p1_linear_depth,
                    skip_tree_skew_ancestor_subtree_source_depth,
                    p1_linear_skew_ancestor,
                    p1_linear_skew_ancestor_gen,
                    p1_linear_skew_ancestor_subtree_source_gen,
                    p1_linear_skew_ancestor_skip_tree_depth,
                    p1_linear_skew_ancestor_p1_linear_depth,
                    p1_linear_skew_ancestor_subtree_source_depth,
                    subtree_or_merge_ancestor,
                    subtree_or_merge_ancestor_gen,
                    subtree_or_merge_ancestor_subtree_source_gen,
                    subtree_or_merge_ancestor_skip_tree_depth,
                    subtree_or_merge_ancestor_p1_linear_depth,
                    subtree_or_merge_ancestor_subtree_source_depth,
                    subtree_source_parent,
                    subtree_source_parent_gen,
                    subtree_source_parent_subtree_source_gen,
                    subtree_source_parent_skip_tree_depth,
                    subtree_source_parent_p1_linear_depth,
                    subtree_source_parent_subtree_source_depth,
                    subtree_source_skew_ancestor,
                    subtree_source_skew_ancestor_gen,
                    subtree_source_skew_ancestor_subtree_source_gen,
                    subtree_source_skew_ancestor_skip_tree_depth,
                    subtree_source_skew_ancestor_p1_linear_depth,
                    subtree_source_skew_ancestor_subtree_source_depth,
                    ..,
                ) => {
                    let subtree_source_depth = subtree_source_depth.unwrap_or(skip_tree_depth);
                    let merge_ancestor = option_fields_to_option_node(
                        merge_ancestor,
                        merge_ancestor_gen,
                        merge_ancestor_subtree_source_gen,
                        merge_ancestor_skip_tree_depth,
                        merge_ancestor_p1_linear_depth,
                        merge_ancestor_subtree_source_depth,
                    );
                    let skip_tree_parent = option_fields_to_option_node(
                        skip_tree_parent,
                        skip_tree_parent_gen,
                        skip_tree_parent_subtree_source_gen,
                        skip_tree_parent_skip_tree_depth,
                        skip_tree_parent_p1_linear_depth,
                        skip_tree_parent_subtree_source_depth,
                    );
                    let skip_tree_skew_ancestor = option_fields_to_option_node(
                        skip_tree_skew_ancestor,
                        skip_tree_skew_ancestor_gen,
                        skip_tree_skew_ancestor_subtree_source_gen,
                        skip_tree_skew_ancestor_skip_tree_depth,
                        skip_tree_skew_ancestor_p1_linear_depth,
                        skip_tree_skew_ancestor_subtree_source_depth,
                    );
                    let p1_linear_skew_ancestor = option_fields_to_option_node(
                        p1_linear_skew_ancestor,
                        p1_linear_skew_ancestor_gen,
                        p1_linear_skew_ancestor_subtree_source_gen,
                        p1_linear_skew_ancestor_skip_tree_depth,
                        p1_linear_skew_ancestor_p1_linear_depth,
                        p1_linear_skew_ancestor_subtree_source_depth,
                    );
                    let subtree_or_merge_ancestor = option_fields_to_option_node(
                        subtree_or_merge_ancestor,
                        subtree_or_merge_ancestor_gen,
                        subtree_or_merge_ancestor_subtree_source_gen,
                        subtree_or_merge_ancestor_skip_tree_depth,
                        subtree_or_merge_ancestor_p1_linear_depth,
                        subtree_or_merge_ancestor_subtree_source_depth,
                    )
                    .or_else(|| {
                        if subtree_source_count == 0 {
                            merge_ancestor.clone()
                        } else {
                            None
                        }
                    });
                    let subtree_source_parent = option_fields_to_option_node(
                        subtree_source_parent,
                        subtree_source_parent_gen,
                        subtree_source_parent_subtree_source_gen,
                        subtree_source_parent_skip_tree_depth,
                        subtree_source_parent_p1_linear_depth,
                        subtree_source_parent_subtree_source_depth,
                    )
                    .or_else(|| skip_tree_parent.clone());
                    let subtree_source_skew_ancestor = option_fields_to_option_node(
                        subtree_source_skew_ancestor,
                        subtree_source_skew_ancestor_gen,
                        subtree_source_skew_ancestor_subtree_source_gen,
                        subtree_source_skew_ancestor_skip_tree_depth,
                        subtree_source_skew_ancestor_p1_linear_depth,
                        subtree_source_skew_ancestor_subtree_source_depth,
                    )
                    .or_else(|| skip_tree_skew_ancestor.clone());

                    cs_id_and_origin_to_edges.insert(
                        (cs_id, origin_cs_id),
                        FetchedChangesetEdges::new(
                            origin_cs_id,
                            ChangesetEdges {
                                node: ChangesetNode {
                                    cs_id,
                                    generation: Generation::new(r#gen),
                                    subtree_source_generation: Generation::new(
                                        subtree_source_gen.unwrap_or(r#gen),
                                    ),
                                    skip_tree_depth,
                                    p1_linear_depth,
                                    subtree_source_depth,
                                },
                                parents: ChangesetNodeParents::new(),
                                subtree_sources: ChangesetNodeSubtreeSources::new(),
                                merge_ancestor,
                                skip_tree_parent,
                                skip_tree_skew_ancestor,
                                p1_linear_skew_ancestor,
                                subtree_or_merge_ancestor,
                                subtree_source_parent,
                                subtree_source_skew_ancestor,
                            },
                        ),
                    );
                }
                _ => continue,
            }
        }

        for row in fetched_rows {
            match *row {
                (
                    cs_id,
                    origin_cs_id,
                    ..,
                    Some(parent_num),
                    None,
                    Some(parent),
                    Some(parent_gen),
                    parent_subtree_source_gen,
                    Some(parent_skip_tree_depth),
                    Some(parent_p1_linear_depth),
                    parent_subtree_source_depth,
                ) => {
                    if let Some(edges) = cs_id_and_origin_to_edges.get_mut(&(cs_id, origin_cs_id)) {
                        edges.parents.push(ChangesetNode {
                            cs_id: parent,
                            generation: Generation::new(parent_gen),
                            subtree_source_generation: Generation::new(
                                parent_subtree_source_gen.unwrap_or(parent_gen),
                            ),
                            skip_tree_depth: parent_skip_tree_depth,
                            p1_linear_depth: parent_p1_linear_depth,
                            subtree_source_depth: parent_subtree_source_depth
                                .unwrap_or(parent_skip_tree_depth),
                        })
                    }
                }
                (
                    cs_id,
                    origin_cs_id,
                    ..,
                    None,
                    Some(subtree_source_num),
                    Some(subtree_source),
                    Some(subtree_source_gen),
                    subtree_source_subtree_source_gen,
                    Some(subtree_source_skip_tree_depth),
                    Some(subtree_source_p1_linear_depth),
                    subtree_source_subtree_source_depth,
                ) => {
                    if let Some(edges) = cs_id_and_origin_to_edges.get_mut(&(cs_id, origin_cs_id)) {
                        edges.subtree_sources.push(ChangesetNode {
                            cs_id: subtree_source,
                            generation: Generation::new(subtree_source_gen),
                            subtree_source_generation: Generation::new(
                                subtree_source_subtree_source_gen.unwrap_or(subtree_source_gen),
                            ),
                            skip_tree_depth: subtree_source_skip_tree_depth,
                            p1_linear_depth: subtree_source_p1_linear_depth,
                            subtree_source_depth: subtree_source_subtree_source_depth
                                .unwrap_or(subtree_source_skip_tree_depth),
                        })
                    }
                }
                _ => continue,
            }
        }

        cs_id_and_origin_to_edges
    }

    /// Group edges by their `cs_id`, deduplicating edges that differ only by their `origin_cs_id`.
    fn collect_changeset_edges(
        fetched_rows: &[FetchedEdgesRow],
    ) -> HashMap<ChangesetId, FetchedChangesetEdges> {
        let cs_id_and_origin_to_edges = Self::collect_changeset_edges_impl(fetched_rows);
        cs_id_and_origin_to_edges
            .into_iter()
            .map(|((cs_id, _origin_cs_id), edges)| (cs_id, edges))
            .collect()
    }

    /// Group edges by their `origin_cs_id`.
    fn collect_prefetched_changeset_edges(
        fetched_rows: &[FetchedEdgesRow],
    ) -> HashMap<ChangesetId, Vec<FetchedChangesetEdges>> {
        let edges = Self::collect_changeset_edges_impl(fetched_rows);
        edges
            .into_iter()
            .flat_map(|((_cs_id, origin_cs_id), edges)| origin_cs_id.map(|origin| (origin, edges)))
            .into_group_map()
    }

    async fn fetch_many_edges_impl(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
        rendezvous: &RendezVousConnection,
    ) -> Result<HashMap<ChangesetId, FetchedChangesetEdges>> {
        if cs_ids.is_empty() {
            // This is actually NECESSARY, because SQL doesn't deal well with
            // querying empty arrays
            return Ok(HashMap::new());
        }

        if let Some(target) = prefetch.target() {
            let steps_limit =
                justknobs::get_as::<u64>("scm/mononoke:commit_graph_prefetch_step_limit", None)
                    .unwrap_or(DEFAULT_PREFETCH_STEP_LIMIT);

            let fetched_edges = match target {
                PrefetchTarget::LinearAncestors { steps, generation } => {
                    rendezvous
                        .fetch_linear_prefetch
                        .dispatch(ctx.fb.clone(), cs_ids.iter().copied().collect(), || {
                            let conn = rendezvous.conn.clone();
                            let repo_id = self.repo_id.clone();
                            let cri = ctx.client_request_info().cloned();

                            move |cs_ids| async move {
                                let cs_ids = cs_ids.into_iter().collect::<Vec<_>>();
                                let fetched_rows =
                                    SelectManyChangesetsWithFirstParentPrefetch::maybe_traced_query(
                                        &conn,
                                        cri.as_ref(),
                                        &repo_id,
                                        &std::cmp::min(steps, steps_limit),
                                        &generation.value(),
                                        &cs_ids,
                                    )
                                    .await?;
                                Ok(Self::collect_prefetched_changeset_edges(&fetched_rows))
                            }
                        })
                        .await?
                }
                PrefetchTarget::ExactSkipTreeAncestors { generation } => {
                    rendezvous
                        .fetch_exact_skip_tree_prefetch
                        .dispatch(ctx.fb.clone(), cs_ids.iter().copied().collect(), || {
                            let conn = rendezvous.conn.clone();
                            let repo_id = self.repo_id.clone();
                            let cri = ctx.client_request_info().cloned();

                            move |cs_ids| async move {
                                let cs_ids = cs_ids.into_iter().collect::<Vec<_>>();
                                let fetched_rows =
                                    SelectManyChangesetsWithExactSkipTreeAncestorPrefetch::maybe_traced_query(
                                        &conn,
                                        cri.as_ref(),
                                        &repo_id,
                                        &generation.value(),
                                        &cs_ids,
                                    )
                                    .await?;
                                Ok(Self::collect_prefetched_changeset_edges(&fetched_rows))
                            }
                        })
                        .await?
                }
            };
            Ok(fetched_edges
                .into_values()
                .flatten()
                .flatten()
                .map(|edges| (edges.node.cs_id, edges))
                .collect())
        } else {
            let ret = rendezvous
                .fetch_single
                .dispatch(ctx.fb.clone(), cs_ids.iter().copied().collect(), || {
                    let conn = rendezvous.conn.clone();
                    let repo_id = self.repo_id.clone();
                    let cri = ctx.client_request_info().cloned();

                    move |cs_ids| async move {
                        let cs_ids = cs_ids.into_iter().collect::<Vec<_>>();
                        let fetched_edges = SelectManyChangesets::maybe_traced_query(
                            &conn,
                            cri.as_ref(),
                            &repo_id,
                            cs_ids.as_slice(),
                        )
                        .await?;
                        Ok(Self::collect_changeset_edges(&fetched_edges))
                    }
                })
                .await?;
            Ok(ret
                .into_iter()
                .filter_map(|(cs_id, cs_edge)| cs_edge.map(|cs_edge| (cs_id, cs_edge)))
                .collect())
        }
    }

    // Lower level APIs for quickly iterating over all changeset edges

    fn read_conn(&self, read_from_master: bool) -> &Connection {
        match read_from_master {
            true => &self.read_master_connection.conn,
            false => &self.read_connection.conn,
        }
    }

    /// Fetch a maximum of `limit` changeset edges for changesets having
    /// auto-increment ids between `start_id` and `end_id`.
    pub async fn fetch_many_edges_in_id_range(
        &self,
        ctx: &CoreContext,
        start_id: u64,
        end_id: u64,
        limit: u64,
        read_from_master: bool,
    ) -> Result<HashMap<ChangesetId, ChangesetEdges>> {
        Ok(Self::collect_changeset_edges(
            &SelectManyChangesetsInIdRange::maybe_traced_query(
                self.read_conn(read_from_master),
                ctx.client_request_info(),
                &self.repo_id,
                &start_id,
                &end_id,
                &limit,
            )
            .await?,
        )
        .into_iter()
        .map(|(k, v)| (k, v.into()))
        .collect())
    }

    /// Fetch a maximum of `limit` changeset ids for changesets having
    /// auto-increment ids between `start_id` and `end_id`.
    pub async fn fetch_many_cs_ids_in_id_range(
        &self,
        ctx: &CoreContext,
        start_id: u64,
        end_id: u64,
        limit: u64,
        read_from_master: bool,
    ) -> Result<Vec<ChangesetId>> {
        Ok(SelectManyChangesetsIdsInIdRange::maybe_traced_query(
            self.read_conn(read_from_master),
            ctx.client_request_info(),
            &self.repo_id,
            &start_id,
            &end_id,
            &limit,
        )
        .await?
        .into_iter()
        .map(|(cs_id,)| cs_id)
        .collect())
    }

    /// Returns the maximum auto-increment id for any changeset in the repo,
    /// or `None` if there are no changesets.
    pub async fn max_id(&self, ctx: &CoreContext, read_from_master: bool) -> Result<Option<u64>> {
        Ok(SelectMaxId::maybe_traced_query(
            self.read_conn(read_from_master),
            ctx.client_request_info(),
            &self.repo_id,
        )
        .await?
        .first()
        .map(|(id,)| *id))
    }

    /// Returns the maximum auto-increment id of changesets having auto-increment
    /// ids between `start_id` and `end_id`, or `None` if there are no such changesets.
    pub async fn max_id_in_range(
        &self,
        ctx: &CoreContext,
        start_id: u64,
        end_id: u64,
        limit: u64,
        read_from_master: bool,
    ) -> Result<Option<u64>> {
        Ok(SelectMaxIdInRange::maybe_traced_query(
            self.read_conn(read_from_master),
            ctx.client_request_info(),
            &self.repo_id,
            &start_id,
            &end_id,
            &limit,
        )
        .await?
        .first()
        .map(|(id,)| *id))
    }

    /// Returns the bounds of the auto-increment ids of changesets in the repo.
    /// The bounds are returns as a half open interval [lo, hi).
    ///
    /// If there are no changesets in the repo, returns `None`.
    pub(crate) async fn repo_bounds(
        &self,
        ctx: &CoreContext,
        read_from_master: bool,
    ) -> Result<Option<Range<u64>>> {
        let conn = self.read_conn(read_from_master);
        let rows = SelectChangesetsIdsBounds::maybe_traced_query(
            conn,
            ctx.client_request_info(),
            &self.repo_id,
        )
        .await?;
        Ok(rows.first().map(|(lo, hi)| *lo..*hi + 1))
    }

    /// Fetch the oldest `limit` changesets from all changesets that have auto-increment ids
    /// in the range [range.start, range.end).
    ///
    /// For each changeset we return a tuple of its changeset id and its auto-increment id.
    pub(crate) async fn fetch_oldest_changesets_in_range(
        &self,
        ctx: &CoreContext,
        range: Range<u64>,
        limit: u64,
        read_from_master: bool,
    ) -> Result<Vec<(ChangesetId, u64)>> {
        let conn = self.read_conn(read_from_master);
        SelectOldestChangesetsIdsInRange::maybe_traced_query(
            conn,
            ctx.client_request_info(),
            &self.repo_id,
            &range.start,
            &range.end,
            &limit,
        )
        .await
    }

    /// Fetch the newest `limit` changesets from all changesets that have auto-increment ids
    /// in the range [range.start, range.end).
    ///
    /// For each changeset we return a tuple of its changeset id and its auto-increment id.
    pub(crate) async fn fetch_newest_changesets_in_range(
        &self,
        ctx: &CoreContext,
        range: Range<u64>,
        limit: u64,
        read_from_master: bool,
    ) -> Result<Vec<(ChangesetId, u64)>> {
        let conn = self.read_conn(read_from_master);
        SelectNewestChangesetsIdsInRange::maybe_traced_query(
            conn,
            ctx.client_request_info(),
            &self.repo_id,
            &range.start,
            &range.end,
            &limit,
        )
        .await
    }

    // Returns the amount of commits in a repo.  Only to be used for ad-hoc internal operations
    pub async fn fetch_commit_count(&self, ctx: &CoreContext, id: RepositoryId) -> Result<u64> {
        let conn = self.read_conn(true);
        let result =
            GetCommitCount::maybe_traced_query(conn, ctx.client_request_info(), &id).await?;
        Ok(result.first().map_or(0, |(count,)| *count))
    }

    async fn _add_many(
        &self,
        ctx: &CoreContext,
        many_edges: &Vec1<ChangesetEdges>,
    ) -> Result<usize> {
        // If we're inserting a single changeset, use the faster single insertion method.
        if many_edges.len() == 1 {
            return Ok(self.add(ctx, many_edges.first().clone()).await? as usize);
        }

        // We need to be careful because there might be dependencies among the edges
        // Part 1 - Add all nodes without any edges, so we generate ids for them
        let transaction = self.write_connection.start_transaction().await?;
        let cri = ctx.client_request_info();
        let cs_no_edges = many_edges
            .iter()
            .map(|e| {
                (
                    self.repo_id,
                    e.node.cs_id,
                    e.node.generation.value(),
                    Some(e.node.subtree_source_generation.value())
                        .filter(|r#gen| *r#gen != e.node.generation.value()),
                    e.node.skip_tree_depth,
                    e.node.p1_linear_depth,
                    Some(e.node.subtree_source_depth)
                        .filter(|depth| *depth != e.node.skip_tree_depth),
                    e.parents.len(),
                    e.subtree_sources.len(),
                )
            })
            .collect::<Vec<_>>();
        let (transaction, result) = InsertChangesetsNoEdges::maybe_traced_query_with_transaction(
            transaction,
            cri,
            // This pattern is used to convert a ref to tuple into a tuple of refs.
            #[allow(clippy::map_identity)]
            cs_no_edges
                .iter()
                .map(|(a, b, c, d, e, f, g, h, i)| (a, b, c, d, e, f, g, h, i))
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .await?;

        let modified = result.affected_rows();
        if modified == 0 {
            // Early return, everything is already stored
            return Ok(0);
        }
        // Part 2 - Collect all changesets we need the ids from, and query them
        // using the same transaction
        let mut need_ids = HashSet::new();
        for edges in many_edges {
            edges.for_all_ids(|cs_id| {
                need_ids.insert(cs_id);
            });
        }
        let (transaction, cs_to_ids) = if !need_ids.is_empty() {
            // Use the same transaction to make sure we see the new values
            let (transaction, result) = SelectManyIds::maybe_traced_query_with_transaction(
                transaction,
                cri,
                &self.repo_id,
                need_ids.into_iter().collect::<Vec<_>>().as_slice(),
            )
            .await?;
            (transaction, result.into_iter().collect())
        } else {
            (transaction, HashMap::new())
        };
        // Part 3 - Fix edges on all changesets we previously inserted
        let get_id = |node: &ChangesetNode| {
            cs_to_ids
                .get(&node.cs_id)
                .copied()
                .with_context(|| format!("Failed to fetch id for changeset {}", node.cs_id))
        };
        let maybe_get_id = |maybe_node: Option<&ChangesetNode>| maybe_node.map(get_id).transpose();
        let rows = match many_edges
            .iter()
            .map(|e| {
                Ok((
                    self.repo_id,
                    e.node.cs_id,
                    e.node.generation.value(),
                    Some(e.node.subtree_source_generation.value())
                        .filter(|r#gen| *r#gen != e.node.generation.value()),
                    e.node.skip_tree_depth,
                    e.node.p1_linear_depth,
                    Some(e.node.subtree_source_depth)
                        .filter(|depth| depth != &e.node.skip_tree_depth),
                    e.parents.len(),
                    e.subtree_sources.len(),
                    maybe_get_id(e.parents.first())?,
                    maybe_get_id(e.merge_ancestor.as_ref())?,
                    maybe_get_id(e.skip_tree_parent.as_ref())?,
                    maybe_get_id(e.skip_tree_skew_ancestor.as_ref())?,
                    maybe_get_id(e.p1_linear_skew_ancestor.as_ref())?,
                    maybe_get_id(
                        e.subtree_or_merge_ancestor
                            .as_ref()
                            .filter(|node| Some(*node) != e.merge_ancestor.as_ref()),
                    )?,
                    maybe_get_id(
                        e.subtree_source_parent
                            .as_ref()
                            .filter(|node| Some(*node) != e.skip_tree_parent.as_ref()),
                    )?,
                    maybe_get_id(
                        e.subtree_source_skew_ancestor
                            .as_ref()
                            .filter(|node| Some(*node) != e.skip_tree_skew_ancestor.as_ref()),
                    )?,
                ))
            })
            .collect::<Result<Vec<_>>>()
        {
            Ok(rows) => rows,
            Err(err) => {
                transaction.rollback().await?;
                return Err(err);
            }
        };

        let (transaction, _) = FixEdges::maybe_traced_query_with_transaction(
            transaction,
            cri,
            // This pattern is used to convert a ref to tuple into a tuple of refs.
            #[allow(clippy::map_identity)]
            rows.iter()
                .map(|(a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q)| {
                    (a, b, c, d, e, f, g, h, i, j, k, l, m, n, o, p, q)
                })
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .await?;

        let merge_parent_rows = many_edges
            .iter()
            .flat_map(|edges| {
                edges
                    .parents
                    .iter()
                    .enumerate()
                    .skip(1)
                    .map(|(parent_num, node)| Ok((get_id(&edges.node)?, parent_num, get_id(node)?)))
            })
            .collect::<Result<Vec<_>>>()?;

        let (transaction, result) = InsertMergeParents::maybe_traced_query_with_transaction(
            transaction,
            cri,
            // This pattern is used to convert a ref to tuple into a tuple of refs.
            #[allow(clippy::map_identity)]
            merge_parent_rows
                .iter()
                .map(|(a, b, c)| (a, b, c))
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .await?;

        let subtree_source_rows = many_edges
            .iter()
            .flat_map(|edges| {
                edges
                    .subtree_sources
                    .iter()
                    .enumerate()
                    .map(|(subtree_source_num, node)| {
                        Ok((get_id(&edges.node)?, subtree_source_num, get_id(node)?))
                    })
            })
            .collect::<Result<Vec<_>>>()?;

        let (transaction, result) = InsertSubtreeSources::maybe_traced_query_with_transaction(
            transaction,
            cri,
            // This pattern is used to convert a ref to tuple into a tuple of refs.
            #[allow(clippy::map_identity)]
            subtree_source_rows
                .iter()
                .map(|(a, b, c)| (a, b, c))
                .collect::<Vec<_>>()
                .as_slice(),
        )
        .await?;

        // All good, nodes were added and correctly updated, let's commit.
        transaction.commit().await?;
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlWrites);

        Ok(modified.try_into()?)
    }
}

#[async_trait]
impl CommitGraphStorage for SqlCommitGraphStorage {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add_many(&self, ctx: &CoreContext, many_edges: Vec1<ChangesetEdges>) -> Result<usize> {
        Ok(retry(
            None,
            |_| self._add_many(ctx, &many_edges),
            should_retry_query,
            RetryLogic::ExponentialWithJitter {
                base: Duration::from_secs(1),
                factor: 1.2,
                jitter: Duration::from_secs(2),
            },
            justknobs::get_as::<usize>("scm/mononoke:commit_graph_storage_sql_retries_num", None)
                .unwrap_or(1),
        )
        .await?
        .0)
    }

    async fn add(&self, ctx: &CoreContext, edges: ChangesetEdges) -> Result<bool> {
        let cri = ctx.client_request_info();
        let merge_parent_cs_id_to_id: HashMap<ChangesetId, u64> = if edges.parents.len() >= 2 {
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsReplica);
            SelectManyIds::maybe_traced_query(
                &self.read_connection.conn,
                cri,
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

        let subtree_source_cs_id_to_id: HashMap<ChangesetId, u64> =
            if edges.subtree_sources.is_empty() {
                Default::default()
            } else {
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlReadsReplica);
                SelectManyIds::maybe_traced_query(
                    &self.read_connection.conn,
                    cri,
                    &self.repo_id,
                    &edges
                        .subtree_sources
                        .iter()
                        .map(|node| node.cs_id)
                        .collect::<Vec<_>>(),
                )
                .await?
                .into_iter()
                .collect()
            };

        let transaction = self.write_connection.start_transaction().await?;

        let (transaction, result) = InsertChangeset::maybe_traced_query_with_transaction(
            transaction,
            cri,
            &self.repo_id,
            &edges.node.cs_id,
            &edges.node.generation.value(),
            &Some(edges.node.subtree_source_generation.value())
                .filter(|r#gen| *r#gen != edges.node.generation.value()),
            &edges.node.skip_tree_depth,
            &edges.node.p1_linear_depth,
            &Some(edges.node.subtree_source_depth)
                .filter(|depth| *depth != edges.node.skip_tree_depth),
            &edges.parents.len(),
            &edges.subtree_sources.len(),
            &edges.parents.first().map(|node| node.cs_id),
            &edges.merge_ancestor.map(|node| node.cs_id),
            &edges.skip_tree_parent.map(|node| node.cs_id),
            &edges.skip_tree_skew_ancestor.map(|node| node.cs_id),
            &edges.p1_linear_skew_ancestor.map(|node| node.cs_id),
            &edges
                .subtree_or_merge_ancestor
                .filter(|node| edges.merge_ancestor.as_ref() != Some(node))
                .map(|node| node.cs_id),
            &edges
                .subtree_source_parent
                .filter(|node| edges.skip_tree_parent.as_ref() != Some(node))
                .map(|node| node.cs_id),
            &edges
                .subtree_source_skew_ancestor
                .filter(|node| edges.skip_tree_skew_ancestor.as_ref() != Some(node))
                .map(|node| node.cs_id),
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

                let (transaction, result) =
                    InsertMergeParents::maybe_traced_query_with_transaction(
                        transaction,
                        cri,
                        // This pattern is used to convert a ref to tuple into a tuple of refs.
                        #[allow(clippy::map_identity)]
                        merge_parent_rows
                            .iter()
                            .map(|(a, b, c)| (a, b, c))
                            .collect::<Vec<_>>()
                            .as_slice(),
                    )
                    .await?;

                let subtree_source_rows = edges
                    .subtree_sources
                    .iter()
                    .enumerate()
                    .map(|(subtree_source_num, node)| {
                        Ok((
                            last_insert_id,
                            subtree_source_num,
                            *subtree_source_cs_id_to_id
                                .get(&node.cs_id)
                                .ok_or_else(|| anyhow!("Failed to fetch id for {}", node.cs_id))?,
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;

                let (transaction, result) =
                    InsertSubtreeSources::maybe_traced_query_with_transaction(
                        transaction,
                        cri,
                        // This pattern is used to convert a ref to tuple into a tuple of refs.
                        #[allow(clippy::map_identity)]
                        subtree_source_rows
                            .iter()
                            .map(|(a, b, c)| (a, b, c))
                            .collect::<Vec<_>>()
                            .as_slice(),
                    )
                    .await?;

                transaction.commit().await?;
                ctx.perf_counters()
                    .increment_counter(PerfCounterType::SqlWrites);

                Ok(true)
            }
            _ => {
                transaction.rollback().await?;
                Ok(false)
            }
        }
    }

    async fn fetch_edges(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<ChangesetEdges> {
        self.fetch_many_edges(ctx, &[cs_id], Prefetch::None)
            .await?
            .remove(&cs_id)
            .map(|edges| edges.into())
            .ok_or_else(|| anyhow!("Missing changeset from sql commit graph storage: {}", cs_id))
    }

    async fn maybe_fetch_edges(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Option<ChangesetEdges>> {
        Ok(self
            .maybe_fetch_many_edges(ctx, &[cs_id], Prefetch::None)
            .await?
            .remove(&cs_id)
            .map(|edges| edges.into()))
    }

    async fn fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, FetchedChangesetEdges>> {
        let mut edges = self.maybe_fetch_many_edges(ctx, cs_ids, prefetch).await?;
        let unfetched_ids: Vec<ChangesetId> = cs_ids
            .iter()
            .filter(|id| !edges.contains_key(id))
            .copied()
            .collect();
        if !unfetched_ids.is_empty() {
            anyhow::bail!(
                "Missing changesets from sql commit graph storage: {}",
                unfetched_ids
                    .into_iter()
                    .fold(String::new(), |mut acc, cs_id| {
                        let _ = write!(acc, "{}, ", cs_id);
                        acc
                    })
            );
        }
        Ok(edges)
    }

    async fn maybe_fetch_many_edges(
        &self,
        ctx: &CoreContext,
        cs_ids: &[ChangesetId],
        prefetch: Prefetch,
    ) -> Result<HashMap<ChangesetId, FetchedChangesetEdges>> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let mut edges = self
            .fetch_many_edges_impl(ctx, cs_ids, prefetch, &self.read_connection)
            .await?;
        let unfetched_ids: Vec<ChangesetId> = cs_ids
            .iter()
            .filter(|id| !edges.contains_key(id))
            .copied()
            .collect();
        if !unfetched_ids.is_empty() {
            // Let's go to master with the remaining edges
            ctx.perf_counters()
                .increment_counter(PerfCounterType::SqlReadsMaster);
            let extra_edges = self
                .fetch_many_edges_impl(ctx, &unfetched_ids, prefetch, &self.read_master_connection)
                .await?;
            edges.extend(extra_edges);
        }
        Ok(edges)
    }

    async fn find_by_prefix(
        &self,
        ctx: &CoreContext,
        cs_prefix: ChangesetIdPrefix,
        limit: usize,
    ) -> Result<ChangesetIdsResolvedFromPrefix> {
        ctx.perf_counters()
            .increment_counter(PerfCounterType::SqlReadsReplica);
        let fetched_ids = SelectChangesetsInRange::maybe_traced_query(
            &self.read_connection.conn,
            ctx.client_request_info(),
            &self.repo_id,
            &cs_prefix.min_bound(),
            &cs_prefix.max_bound(),
            &(limit + 1),
        )
        .await?
        .into_iter()
        .map(|(cs_id,)| cs_id)
        .collect::<Vec<_>>();

        Ok(ChangesetIdsResolvedFromPrefix::from_vec_and_limit(
            fetched_ids,
            limit,
        ))
    }

    async fn fetch_children(
        &self,
        ctx: &CoreContext,
        cs_id: ChangesetId,
    ) -> Result<Vec<ChangesetId>> {
        Ok(SelectChildren::maybe_traced_query(
            &self.read_master_connection.conn,
            ctx.client_request_info(),
            &self.repo_id,
            &cs_id,
        )
        .await?
        .into_iter()
        .map(|(cs_id,)| cs_id)
        .collect())
    }
}
