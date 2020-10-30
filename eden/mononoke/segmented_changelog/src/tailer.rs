/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::{format_err, Context, Result};
use slog::info;

use dag::{Group, Id as Vertex, InProcessIdDag};
use sql_ext::replication::ReplicaLagMonitor;
use stats::prelude::*;

use bookmarks::{BookmarkName, Bookmarks};
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::RepositoryId;
use sql_ext::SqlConnections;

use crate::bundle::SqlBundleStore;
use crate::dag::Dag;
use crate::iddag::IdDagSaveStore;
use crate::idmap::SqlIdMap;
use crate::on_demand::build_incremental;
use crate::types::DagBundle;

define_stats! {
    prefix = "mononoke.segmented_changelog.tailer";
    build_incremental: timeseries(Sum),
}

pub struct SegmentedChangelogTailer {
    connections: SqlConnections,
    repo_id: RepositoryId,
    replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    bookmarks: Arc<dyn Bookmarks>,
    bookmark_name: BookmarkName,
    iddag_save_store: IdDagSaveStore,
    bundle_store: SqlBundleStore,
}

impl SegmentedChangelogTailer {
    pub fn new(
        connections: SqlConnections,
        repo_id: RepositoryId,
        replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
        bookmarks: Arc<dyn Bookmarks>,
        bookmark_name: BookmarkName,
        iddag_save_store: IdDagSaveStore,
        bundle_store: SqlBundleStore,
    ) -> Self {
        Self {
            connections,
            repo_id,
            replica_lag_monitor,
            changeset_fetcher,
            bookmarks,
            bookmark_name,
            iddag_save_store,
            bundle_store,
        }
    }

    pub async fn run(&self, ctx: &CoreContext, delay: Duration) -> Result<()> {
        loop {
            self.once(&ctx).await.context("running periodic update")?;
            info!(ctx.logger(), "sleeping for {} seconds", delay.as_secs());
            tokio::time::delay_for(delay).await;
        }
    }

    pub async fn once(&self, ctx: &CoreContext) -> Result<(Dag, Vertex)> {
        info!(
            ctx.logger(),
            "starting incremental update to segmented changelog"
        );

        let bundle = self
            .bundle_store
            .get(&ctx)
            .await
            .context("fetching version information")?
            .ok_or_else(|| {
                format_err!(
                    "could not find bundle information for repo {}, maybe it needs seeding",
                    self.repo_id
                )
            })?;
        let idmap_version = bundle.idmap_version;
        info!(
            ctx.logger(),
            "base idmap version: {}; base iddag version: {}", idmap_version, bundle.iddag_version
        );
        let idmap: Arc<SqlIdMap> = Arc::new(SqlIdMap::new(
            self.connections.clone(),
            self.replica_lag_monitor.clone(),
            self.repo_id,
            idmap_version,
        ));
        let iddag = self
            .iddag_save_store
            .load(&ctx, bundle.iddag_version)
            .await
            .context("loading iddag save")?;
        let mut dag = Dag::new(iddag, idmap.clone());
        info!(ctx.logger(), "base dag loaded successfully");

        let head = self
            .bookmarks
            .get(ctx.clone(), &self.bookmark_name)
            .await
            .context("fetching master changesetid")?
            .ok_or_else(|| format_err!("'{}' bookmark could not be found", self.bookmark_name))?;
        info!(
            ctx.logger(),
            "bookmark {} resolved to {}", self.bookmark_name, head
        );
        let old_master_vertex = dag
            .iddag
            .next_free_id(0, Group::MASTER)
            .context("fetching next free id")?;

        // This updates the IdMap common storage and also updates the dag we loaded.
        let head_vertex = build_incremental(&ctx, &mut dag, &self.changeset_fetcher, head)
            .await
            .context("when incrementally building dag")?;

        if old_master_vertex > head_vertex {
            info!(
                ctx.logger(),
                "dag already up to date, skipping update to iddag"
            );
            return Ok((dag, head_vertex));
        } else {
            info!(ctx.logger(), "IdMap updated, IdDag updated");
        }

        // Let's rebuild the dag to keep segment fragmentation low
        let mut new_iddag = InProcessIdDag::new_in_process();
        let get_parents = |id| dag.iddag.parent_ids(id);
        new_iddag.build_segments_volatile(head_vertex, &get_parents)?;
        info!(ctx.logger(), "IdDag rebuilt");

        // Save the IdDag
        let iddag_version = self
            .iddag_save_store
            .save(&ctx, &new_iddag)
            .await
            .context("saving iddag")?;
        // Update BundleStore
        self.bundle_store
            .set(&ctx, DagBundle::new(iddag_version, idmap_version))
            .await
            .context("updating bundle store")?;
        info!(
            ctx.logger(),
            "success - new iddag saved, idmap_version: {}, iddag_version: {} ",
            idmap_version,
            iddag_version,
        );
        Ok((Dag::new(new_iddag, idmap), head_vertex))
    }
}
