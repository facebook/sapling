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

#[async_trait]
impl CommitGraphStorage for SqlCommitGraphStorage {
    fn repo_id(&self) -> RepositoryId {
        self.repo_id
    }

    async fn add(&self, _ctx: &CoreContext, _edges: ChangesetEdges) -> Result<bool> {
        todo!()
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
