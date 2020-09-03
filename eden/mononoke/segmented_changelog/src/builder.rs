/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use dag::InProcessIdDag;
use mononoke_types::RepositoryId;
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::replication::{NoReplicaLagMonitor, ReplicaLagMonitor};
use sql_ext::SqlConnections;

use crate::dag::Dag;
use crate::idmap::IdMap;

/// SegmentedChangelog instatiation helper.
/// It works together with SegmentedChangelogConfig and BlobRepoFactory to produce a
/// SegmentedChangelog.
/// Config options:
/// Enabled = false -> DisabledSegmentedChangelog
/// Enabled = true
///   update_algorithm = 'ondemand' -> OnDemandUpdateDag
///   update_algorithm != 'ondemand' -> Dag
pub struct SegmentedChangelogBuilder {
    connections: SqlConnections,
    replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
}

impl SqlConstruct for SegmentedChangelogBuilder {
    const LABEL: &'static str = "segmented_changelog";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-segmented-changelog.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            connections,
            replica_lag_monitor: Arc::new(NoReplicaLagMonitor()),
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SegmentedChangelogBuilder {}

impl SegmentedChangelogBuilder {
    pub fn with_replica_lag_monitor(
        mut self,
        replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
    ) -> Self {
        self.replica_lag_monitor = replica_lag_monitor;
        self
    }

    pub fn build_with_repo_id(self, repo_id: RepositoryId) -> Dag {
        let idmap = Arc::new(self.build_idmap());
        let iddag = InProcessIdDag::new_in_process();
        Dag::new(repo_id, iddag, idmap)
    }

    pub(crate) fn build_idmap(self) -> IdMap {
        IdMap::new(self.connections, self.replica_lag_monitor)
    }
}
