/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::{format_err, Result};
use changeset_fetcher::ChangesetFetcher;
use dag::InProcessIdDag;
use mononoke_types::RepositoryId;
use sql_construct::{SqlConstruct, SqlConstructFromMetadataDatabaseConfig};
use sql_ext::replication::{NoReplicaLagMonitor, ReplicaLagMonitor};
use sql_ext::SqlConnections;

use crate::dag::{Dag, OnDemandUpdateDag};
use crate::idmap::IdMap;
use crate::DisabledSegmentedChangelog;

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
    repo_id: Option<RepositoryId>,
    replica_lag_monitor: Option<Arc<dyn ReplicaLagMonitor>>,
    changeset_fetcher: Option<Arc<dyn ChangesetFetcher>>,
}

impl SqlConstruct for SegmentedChangelogBuilder {
    const LABEL: &'static str = "segmented_changelog";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-segmented-changelog.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            connections,
            repo_id: None,
            replica_lag_monitor: None,
            changeset_fetcher: None,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SegmentedChangelogBuilder {}

impl SegmentedChangelogBuilder {
    pub fn with_repo_id(mut self, repo_id: RepositoryId) -> Self {
        self.repo_id = Some(repo_id);
        self
    }

    pub fn with_replica_lag_monitor(
        mut self,
        replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
    ) -> Self {
        self.replica_lag_monitor = Some(replica_lag_monitor);
        self
    }

    pub fn with_changeset_fetcher(mut self, changeset_fetcher: Arc<dyn ChangesetFetcher>) -> Self {
        self.changeset_fetcher = Some(changeset_fetcher);
        self
    }

    pub fn build_disabled(self) -> DisabledSegmentedChangelog {
        DisabledSegmentedChangelog::new()
    }

    pub fn build_read_only(mut self) -> Result<Dag> {
        let iddag = InProcessIdDag::new_in_process();
        let replica_lag_monitor = self.replica_lag_monitor();
        let repo_id = self.repo_id()?;
        let idmap = Arc::new(IdMap::new(self.connections, replica_lag_monitor));
        Ok(Dag::new(repo_id, iddag, idmap))
    }

    pub fn build_on_demand_update(mut self) -> Result<OnDemandUpdateDag> {
        let iddag = InProcessIdDag::new_in_process();
        let replica_lag_monitor = self.replica_lag_monitor();
        let repo_id = self.repo_id()?;
        let changeset_fetcher = self.changeset_fetcher()?;
        let idmap = Arc::new(IdMap::new(self.connections, replica_lag_monitor));
        let dag = Dag::new(repo_id, iddag, idmap);
        Ok(OnDemandUpdateDag::new(dag, changeset_fetcher))
    }

    #[cfg(test)]
    pub(crate) fn build_idmap(mut self) -> IdMap {
        let replica_lag_monitor = self.replica_lag_monitor();
        IdMap::new(self.connections, replica_lag_monitor)
    }

    fn repo_id(&mut self) -> Result<RepositoryId> {
        self.repo_id.take().ok_or_else(|| {
            format_err!("SegmentedChangelog cannot be built without RepositoryId being specified.")
        })
    }

    fn replica_lag_monitor(&mut self) -> Arc<dyn ReplicaLagMonitor> {
        self.replica_lag_monitor
            .take()
            .unwrap_or_else(|| Arc::new(NoReplicaLagMonitor()))
    }

    fn changeset_fetcher(&mut self) -> Result<Arc<dyn ChangesetFetcher>> {
        self.changeset_fetcher.take().ok_or_else(|| {
            format_err!(
                "SegmentedChangelog cannot be built without ChangesetFetcher being specified."
            )
        })
    }
}
