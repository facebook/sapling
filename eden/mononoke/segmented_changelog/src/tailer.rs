/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::{format_err, Context, Result};
use futures_stats::TimedFutureExt;
use slog::{debug, error, info};

use dag::{Group, Id as Vertex, InProcessIdDag};
use stats::prelude::*;

use bookmarks::{BookmarkName, Bookmarks};
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::RepositoryId;

use crate::dag::Dag;
use crate::manager::SegmentedChangelogManager;
use crate::update::build_incremental;

define_stats! {
    prefix = "mononoke.segmented_changelog.update";
    count: timeseries(Sum),
    failure: timeseries(Sum),
    success: timeseries(Sum),
    duration_ms:
        histogram(1000, 0, 60_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99),
    count_per_repo: dynamic_timeseries("{}.count", (repo_id: i32); Sum),
    failure_per_repo: dynamic_timeseries("{}.failure", (repo_id: i32); Sum),
    success_per_repo: dynamic_timeseries("{}.success", (repo_id: i32); Sum),
    duration_ms_per_repo: dynamic_histogram(
        "{}.duration_ms", (repo_id: i32);
        1000, 0, 60_000, Average, Sum, Count; P 5; P 25; P 50; P 75; P 95; P 97; P 99
    ),
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

    pub async fn run(&self, ctx: &CoreContext, period: Duration) {
        STATS::success.add_value(0);
        STATS::success_per_repo.add_value(0, (self.repo_id.id(),));

        let mut interval = tokio::time::interval(period);
        loop {
            let _ = interval.tick().await;
            debug!(ctx.logger(), "repo {}: woke up to update", self.repo_id,);

            STATS::count.add_value(1);
            STATS::count_per_repo.add_value(1, (self.repo_id.id(),));

            let (stats, update_result) = self.once(&ctx).timed().await;

            STATS::duration_ms.add_value(stats.completion_time.as_millis() as i64);
            STATS::duration_ms_per_repo.add_value(
                stats.completion_time.as_millis() as i64,
                (self.repo_id.id(),),
            );

            if let Err(err) = update_result {
                STATS::failure.add_value(1);
                STATS::failure_per_repo.add_value(1, (self.repo_id.id(),));
                error!(
                    ctx.logger(),
                    "repo {}: failed to incrementally update segmented changelog: {:?}",
                    self.repo_id,
                    err
                );
            } else {
                STATS::success.add_value(1);
                STATS::success_per_repo.add_value(1, (self.repo_id.id(),));
            }
        }
    }

    pub async fn once(&self, ctx: &CoreContext) -> Result<(Dag, Vertex)> {
        info!(
            ctx.logger(),
            "repo {}: starting incremental update to segmented changelog", self.repo_id,
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
            "repo {}: bookmark {} resolved to {}", self.repo_id, self.bookmark_name, head
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
                "repo {}: dag already up to date, skipping update to iddag", self.repo_id
            );
            return Ok((dag, head_vertex));
        } else {
            info!(
                ctx.logger(),
                "repo {}: IdMap updated, IdDag updated", self.repo_id
            );
        }

        // Let's rebuild the dag to keep segment fragmentation low
        let mut new_iddag = InProcessIdDag::new_in_process();
        let get_parents = |id| dag.iddag.parent_ids(id);
        new_iddag.build_segments_volatile(head_vertex, &get_parents)?;
        info!(ctx.logger(), "repo {}: IdDag rebuilt", self.repo_id);

        // Save the Dag
        self.manager
            .save_dag(&ctx, &new_iddag, bundle.idmap_version)
            .await
            .context("failed to save updated dag")?;

        info!(
            ctx.logger(),
            "repo {}: successful incremental update to segmented changelog", self.repo_id,
        );

        let new_dag = Dag::new(new_iddag, dag.idmap);
        Ok((new_dag, head_vertex))
    }
}
