/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use context::CoreContext;
use futures_ext::FutureExt;
use futures_old::{future, Future};
use mononoke_types::{ChangesetId, RepositoryId};
use sql::{queries, Connection};
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::SqlConnections;
use stats::prelude::*;
use std::collections::HashSet;

use crate::Phase;

define_stats! {
    prefix = "mononoke.phases";
    get_single: timeseries(Rate, Sum),
    get_many: timeseries(Rate, Sum),
    add_many: timeseries(Rate, Sum),
}

/// Object that reads/writes to phases db
#[derive(Clone)]
pub struct SqlPhasesStore {
    write_connection: Connection,
    read_connection: Connection,
    read_master_connection: Connection,
}

impl SqlPhasesStore {
    pub fn get_single_raw(
        &self,
        repo_id: RepositoryId,
        cs_id: ChangesetId,
    ) -> impl Future<Item = Option<Phase>, Error = Error> {
        STATS::get_single.add_value(1);
        SelectPhase::query(&self.read_connection, &repo_id, &cs_id)
            .map(move |rows| rows.into_iter().next().map(|row| row.0))
    }

    pub fn get_public_raw(
        &self,
        repo_id: RepositoryId,
        csids: &[ChangesetId],
    ) -> impl Future<Item = HashSet<ChangesetId>, Error = Error> {
        if csids.is_empty() {
            return future::ok(Default::default()).left_future();
        }
        STATS::get_many.add_value(1);
        SelectPhases::query(&self.read_connection, &repo_id, &csids)
            .map(move |rows| {
                rows.into_iter()
                    .filter(|row| row.1 == Phase::Public)
                    .map(|row| row.0)
                    .collect()
            })
            .right_future()
    }

    pub fn add_public_raw(
        &self,
        _ctx: CoreContext,
        repoid: RepositoryId,
        csids: Vec<ChangesetId>,
    ) -> impl Future<Item = (), Error = Error> {
        if csids.is_empty() {
            return future::ok(()).left_future();
        }
        let phases: Vec<_> = csids
            .iter()
            .map(|csid| (&repoid, csid, &Phase::Public))
            .collect();
        STATS::add_many.add_value(1);
        InsertPhase::query(&self.write_connection, &phases)
            .map(|_| ())
            .right_future()
    }

    pub fn list_all_public(
        &self,
        _ctx: CoreContext,
        repo_id: RepositoryId,
    ) -> impl Future<Item = Vec<ChangesetId>, Error = Error> {
        SelectAllPublic::query(&self.read_connection, &repo_id)
            .map(|ans| ans.into_iter().map(|x| x.0).collect())
    }
}

queries! {
    write InsertPhase(values: (repo_id: RepositoryId, cs_id: ChangesetId, phase: Phase)) {
        none,
        mysql("INSERT INTO phases (repo_id, cs_id, phase) VALUES {values} ON DUPLICATE KEY UPDATE phase = VALUES(phase)")
        // sqlite query currently doesn't support changing the value
        // there is not usage for changing the phase at the moment
        // TODO (liubovd): improve sqlite query to make it semantically the same
        sqlite("INSERT OR IGNORE INTO phases (repo_id, cs_id, phase) VALUES {values}")
    }

    read SelectPhase(repo_id: RepositoryId, cs_id: ChangesetId) -> (Phase) {
        "SELECT phase FROM phases WHERE repo_id = {repo_id} AND cs_id = {cs_id}"
    }

    read SelectPhases(
        repo_id: RepositoryId,
        >list cs_ids: ChangesetId
    ) -> (ChangesetId, Phase) {
        "SELECT cs_id, phase
         FROM phases
         WHERE repo_id = {repo_id}
           AND cs_id IN {cs_ids}"
    }

    read SelectAllPublic(repo_id: RepositoryId) -> (ChangesetId, ) {
        "SELECT cs_id
         FROM phases
         WHERE repo_id = {repo_id}
           AND phase = 'Public'"
    }
}

impl SqlConstruct for SqlPhasesStore {
    const LABEL: &'static str = "phases";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-phases.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            write_connection: connections.write_connection,
            read_connection: connections.read_connection,
            read_master_connection: connections.read_master_connection,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SqlPhasesStore {}
