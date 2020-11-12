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
use stats::prelude::*;

use bookmarks::{BookmarkName, Bookmarks};
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::RepositoryId;

use crate::dag::Dag;
use crate::manager::SegmentedChangelogManager;
use crate::on_demand::build_incremental;

define_stats! {
    prefix = "mononoke.segmented_changelog.tailer";
    build_incremental: timeseries(Sum),
}

pub struct SegmentedChangelogTailer {
    repo_id: RepositoryId,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    bookmarks: Arc<dyn Bookmarks>,
    bookmark_name: BookmarkName,
    manager: SegmentedChangelogManager,
}

impl SegmentedChangelogTailer {
    pub fn new(
        repo_id: RepositoryId,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
        bookmarks: Arc<dyn Bookmarks>,
        bookmark_name: BookmarkName,
        manager: SegmentedChangelogManager,
    ) -> Self {
        Self {
            repo_id,
            changeset_fetcher,
            bookmarks,
            bookmark_name,
            manager,
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
            "starting incremental update to segmented changelog for repo {}", self.repo_id,
        );

        let (bundle, mut dag) = self
            .manager
            .load_dag(&ctx)
            .await
            .context("failed to load base dag")?;

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

        // Save the Dag
        self.manager
            .save_dag(&ctx, &new_iddag, bundle.idmap_version)
            .await
            .context("failed to save updated dag")?;

        info!(
            ctx.logger(),
            "successful incremental update to segmented changelog for repo {}", self.repo_id,
        );

        let new_dag = Dag::new(new_iddag, dag.idmap);
        Ok((new_dag, head_vertex))
    }
}
