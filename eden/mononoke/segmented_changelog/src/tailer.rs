/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::{format_err, Context, Result};
use fbinit::FacebookInit;
use futures_stats::TimedFutureExt;
use slog::{debug, error, info};
use sql_ext::replication::ReplicaLagMonitor;

use stats::prelude::*;

use blobstore::Blobstore;
use bookmarks::{BookmarkName, Bookmarks};
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::{ChangesetId, RepositoryId};

use crate::dag::ops::DagAddHeads;
use crate::dag::VertexListWithOptions;
use crate::iddag::IdDagSaveStore;
use crate::idmap::{CacheHandlers, IdMapFactory};
use crate::owned::OwnedSegmentedChangelog;
use crate::parents::FetchParents;
use crate::types::SegmentedChangelogVersion;
use crate::update::{head_with_options, server_namedag};
use crate::version_store::SegmentedChangelogVersionStore;
use crate::SegmentedChangelogSqlConnections;

define_stats! {
    prefix = "mononoke.segmented_changelog.tailer.update";
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
    sc_version_store: SegmentedChangelogVersionStore,
    iddag_save_store: IdDagSaveStore,
    idmap_factory: IdMapFactory,
}

impl SegmentedChangelogTailer {
    pub fn new(
        repo_id: RepositoryId,
        connections: SegmentedChangelogSqlConnections,
        replica_lag_monitor: Arc<dyn ReplicaLagMonitor>,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
        blobstore: Arc<dyn Blobstore>,
        bookmarks: Arc<dyn Bookmarks>,
        bookmark_name: BookmarkName,
        caching: Option<(FacebookInit, cachelib::VolatileLruCachePool)>,
    ) -> Self {
        let sc_version_store = SegmentedChangelogVersionStore::new(connections.0.clone(), repo_id);
        let iddag_save_store = IdDagSaveStore::new(repo_id, blobstore);
        let mut idmap_factory = IdMapFactory::new(connections.0, replica_lag_monitor, repo_id);
        if let Some((fb, pool)) = caching {
            let cache_handlers = CacheHandlers::prod(fb, pool);
            idmap_factory = idmap_factory.with_cache_handlers(cache_handlers);
        }
        Self {
            repo_id,
            changeset_fetcher,
            bookmarks,
            bookmark_name,
            sc_version_store,
            iddag_save_store,
            idmap_factory,
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

            let mut scuba = ctx.scuba().clone();
            scuba.add_future_stats(&stats);
            scuba.add("repo_id", self.repo_id.id());
            scuba.add("success", update_result.is_ok());

            let msg = match update_result {
                Ok((_, head_cs)) => {
                    STATS::success.add_value(1);
                    STATS::success_per_repo.add_value(1, (self.repo_id.id(),));
                    scuba.add("changeset_id", format!("{}", head_cs));
                    None
                }
                Err(err) => {
                    STATS::failure.add_value(1);
                    STATS::failure_per_repo.add_value(1, (self.repo_id.id(),));
                    error!(
                        ctx.logger(),
                        "repo {}: failed to incrementally update segmented changelog: {:?}",
                        self.repo_id,
                        err
                    );
                    Some(format!("{:?}", err))
                }
            };
            scuba.log_with_msg("segmented_changelog_tailer_update", msg);
        }
    }

    pub async fn once(&self, ctx: &CoreContext) -> Result<(OwnedSegmentedChangelog, ChangesetId)> {
        info!(
            ctx.logger(),
            "repo {}: starting incremental update to segmented changelog", self.repo_id,
        );

        let sc_version = self
            .sc_version_store
            .get(&ctx)
            .await
            .with_context(|| {
                format!(
                    "repo {}: error loading segmented changelog version",
                    self.repo_id
                )
            })?
            .ok_or_else(|| {
                format_err!(
                    "repo {}: segmented changelog metadata not found, maybe repo is not seeded",
                    self.repo_id
                )
            })?;
        let idmap = self.idmap_factory.for_writer(ctx, sc_version.idmap_version);

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

        let iddag = self
            .iddag_save_store
            .load(&ctx, sc_version.iddag_version)
            .await
            .with_context(|| format!("repo {}: failed to load iddag", self.repo_id))?;

        let mut namedag = server_namedag(ctx.clone(), iddag, idmap)?;
        let parent_fetcher = FetchParents::new(ctx.clone(), self.changeset_fetcher.clone());

        let heads = VertexListWithOptions::from(vec![head_with_options(head)]);
        // Note on memory use: we do not flush the changes out in the middle
        // of writing to the IdMap.
        // Thus, if OOMs happen here, the IdMap may need to flush writes to the DB
        // at interesting points.
        let changed = namedag.add_heads(&parent_fetcher, &heads).await?;

        let (idmap, iddag) = namedag.into_idmap_dag();
        let idmap = idmap.finish().await?;

        if !changed {
            info!(
                ctx.logger(),
                "repo {}: segmented changelog already up to date, skipping update to iddag",
                self.repo_id
            );
            let owned = OwnedSegmentedChangelog::new(iddag, idmap);
            return Ok((owned, head));
        }

        info!(
            ctx.logger(),
            "repo {}: IdMap updated, IdDag updated", self.repo_id
        );

        // Save the IdDag
        let iddag_version = self
            .iddag_save_store
            .save(&ctx, &iddag)
            .await
            .with_context(|| format!("repo {}: error saving iddag", self.repo_id))?;

        // Update SegmentedChangelogVersion
        let sc_version = SegmentedChangelogVersion::new(iddag_version, sc_version.idmap_version);
        self.sc_version_store
            .update(&ctx, sc_version)
            .await
            .with_context(|| {
                format!(
                    "repo {}: error updating segmented changelog version store",
                    self.repo_id
                )
            })?;

        info!(
            ctx.logger(),
            "repo {}: successful incremental update to segmented changelog", self.repo_id,
        );

        let owned = OwnedSegmentedChangelog::new(iddag, idmap);
        Ok((owned, head))
    }
}
