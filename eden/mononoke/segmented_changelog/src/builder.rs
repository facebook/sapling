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

use crate::bundle::SqlBundleStore;
use crate::dag::{Dag, OnDemandUpdateDag};
use crate::iddag::SqlIdDagVersionStore;
use crate::idmap::{SqlIdMap, SqlIdMapVersionStore};
use crate::types::IdMapVersion;
use crate::DisabledSegmentedChangelog;

/// SegmentedChangelog instatiation helper.
/// It works together with SegmentedChangelogConfig and BlobRepoFactory to produce a
/// SegmentedChangelog.
/// Config options:
/// Enabled = false -> DisabledSegmentedChangelog
/// Enabled = true
///   update_algorithm = 'ondemand' -> OnDemandUpdateDag
///   update_algorithm != 'ondemand' -> Dag
#[derive(Default, Clone)]
pub struct SegmentedChangelogBuilder {
    connections: Option<SqlConnections>,
    repo_id: Option<RepositoryId>,
    idmap_version: Option<IdMapVersion>,
    replica_lag_monitor: Option<Arc<dyn ReplicaLagMonitor>>,
    changeset_fetcher: Option<Arc<dyn ChangesetFetcher>>,
}

impl SqlConstruct for SegmentedChangelogBuilder {
    const LABEL: &'static str = "segmented_changelog";

    const CREATION_QUERY: &'static str = include_str!("../schemas/sqlite-segmented-changelog.sql");

    fn from_sql_connections(connections: SqlConnections) -> Self {
        Self {
            connections: Some(connections),
            repo_id: None,
            idmap_version: None,
            replica_lag_monitor: None,
            changeset_fetcher: None,
        }
    }
}

impl SqlConstructFromMetadataDatabaseConfig for SegmentedChangelogBuilder {}

impl SegmentedChangelogBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_sql_connections(mut self, connections: SqlConnections) -> Self {
        self.connections = Some(connections);
        self
    }

    pub fn with_repo_id(mut self, repo_id: RepositoryId) -> Self {
        self.repo_id = Some(repo_id);
        self
    }

    pub fn with_idmap_version(mut self, version: u64) -> Self {
        self.idmap_version = Some(IdMapVersion(version));
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
        self.build_dag()
    }

    pub fn build_on_demand_update(mut self) -> Result<OnDemandUpdateDag> {
        let dag = self.build_dag()?;
        let changeset_fetcher = self.changeset_fetcher()?;
        Ok(OnDemandUpdateDag::new(dag, changeset_fetcher))
    }

    pub fn build_dag(&mut self) -> Result<Dag> {
        let iddag = InProcessIdDag::new_in_process();
        let repo_id = self.repo_id()?;
        let idmap = Arc::new(self.build_sql_idmap()?);
        Ok(Dag::new(repo_id, iddag, idmap))
    }

    pub(crate) fn build_sql_idmap(&mut self) -> Result<SqlIdMap> {
        let connections = self.connections()?;
        let replica_lag_monitor = self.replica_lag_monitor();
        let repo_id = self.repo_id()?;
        let idmap_version = self.idmap_version();
        Ok(SqlIdMap::new(
            connections,
            replica_lag_monitor,
            repo_id,
            idmap_version,
        ))
    }

    // public to go around unused
    pub fn build_sql_idmap_version_store(&self) -> Result<SqlIdMapVersionStore> {
        let connections = self.connections_clone()?;
        let repo_id = self.repo_id()?;
        Ok(SqlIdMapVersionStore::new(connections, repo_id))
    }

    // public to go around unused
    pub fn build_sql_iddag_version_store(&self) -> Result<SqlIdDagVersionStore> {
        let connections = self.connections_clone()?;
        let repo_id = self.repo_id()?;
        Ok(SqlIdDagVersionStore::new(connections, repo_id))
    }

    // public to go around unused
    pub fn build_sql_bundle_store(&self) -> Result<SqlBundleStore> {
        let connections = self.connections_clone()?;
        let repo_id = self.repo_id()?;
        Ok(SqlBundleStore::new(connections, repo_id))
    }

    fn repo_id(&self) -> Result<RepositoryId> {
        self.repo_id.ok_or_else(|| {
            format_err!("SegmentedChangelog cannot be built without RepositoryId being specified.")
        })
    }

    fn idmap_version(&self) -> IdMapVersion {
        self.idmap_version.unwrap_or_default()
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

    fn connections(&mut self) -> Result<SqlConnections> {
        self.connections.take().ok_or_else(|| {
            format_err!(
                "SegmentedChangelog cannot be built without SqlConnections being specified."
            )
        })
    }

    fn connections_clone(&self) -> Result<SqlConnections> {
        let connections = self.connections.as_ref().ok_or_else(|| {
            format_err!(
                "SegmentedChangelog cannot be built without SqlConnections being specified."
            )
        })?;
        Ok(connections.clone())
    }
}
