/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{format_err, Context, Result};
use async_trait::async_trait;

use futures_stats::TimedFutureExt;

use bookmarks::Bookmarks;
use changeset_fetcher::ArcChangesetFetcher;
use context::CoreContext;
use mononoke_types::{ChangesetId, RepositoryId};

use crate::iddag::IdDagSaveStore;
use crate::idmap::IdMapFactory;
use crate::on_demand::OnDemandUpdateSegmentedChangelog;
use crate::owned::OwnedSegmentedChangelog;
use crate::version_store::SegmentedChangelogVersionStore;
use crate::{
    segmented_changelog_delegate, CloneData, CloneHints, Location, SeedHead, SegmentedChangelog,
};

pub struct SegmentedChangelogManager {
    repo_id: RepositoryId,
    sc_version_store: SegmentedChangelogVersionStore,
    iddag_save_store: IdDagSaveStore,
    idmap_factory: IdMapFactory,
    changeset_fetcher: ArcChangesetFetcher,
    bookmarks: Arc<dyn Bookmarks>,
    seed_heads: Vec<SeedHead>,
    update_to_master_bookmark_period: Option<Duration>,
    clone_hints: Option<CloneHints>,
}

impl SegmentedChangelogManager {
    pub fn new(
        repo_id: RepositoryId,
        sc_version_store: SegmentedChangelogVersionStore,
        iddag_save_store: IdDagSaveStore,
        idmap_factory: IdMapFactory,
        changeset_fetcher: ArcChangesetFetcher,
        bookmarks: Arc<dyn Bookmarks>,
        seed_heads: Vec<SeedHead>,
        update_to_master_bookmark_period: Option<Duration>,
        clone_hints: Option<CloneHints>,
    ) -> Self {
        Self {
            repo_id,
            sc_version_store,
            iddag_save_store,
            idmap_factory,
            changeset_fetcher,
            bookmarks,
            seed_heads,
            update_to_master_bookmark_period,
            clone_hints,
        }
    }

    pub async fn load(
        &self,
        ctx: &CoreContext,
    ) -> Result<Arc<dyn SegmentedChangelog + Send + Sync>> {
        let monitored = async {
            let on_demand = self.load_ondemand_update(ctx).await?;
            let asc: Arc<dyn SegmentedChangelog + Send + Sync> =
                match self.update_to_master_bookmark_period {
                    None => on_demand,
                    Some(period) => {
                        Arc::new(on_demand.with_periodic_update_to_master_bookmark(ctx, period))
                    }
                };
            Ok(asc)
        };

        let (stats, ret) = monitored.timed().await;
        let mut scuba = ctx.scuba().clone();
        scuba.add_future_stats(&stats);
        scuba.add("repo_id", self.repo_id.id());
        scuba.add("success", ret.is_ok());
        let msg = ret.as_ref().err().map(|err| format!("{:?}", err));
        scuba.log_with_msg("segmented_changelog_load", msg);

        ret
    }

    async fn load_ondemand_update(
        &self,
        ctx: &CoreContext,
    ) -> Result<Arc<OnDemandUpdateSegmentedChangelog>> {
        let owned = self.load_owned(ctx).await.with_context(|| {
            format!("repo {}: failed to load segmented changelog", self.repo_id)
        })?;
        Ok(Arc::new(OnDemandUpdateSegmentedChangelog::new(
            ctx.clone(),
            self.repo_id,
            owned.iddag,
            owned.idmap,
            Arc::clone(&self.changeset_fetcher),
            Arc::clone(&self.bookmarks),
            self.seed_heads.clone(),
            self.clone_hints.clone(),
        )?))
    }

    // public for builder only
    pub async fn load_owned(&self, ctx: &CoreContext) -> Result<OwnedSegmentedChangelog> {
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
        let iddag = self
            .iddag_save_store
            .load(&ctx, sc_version.iddag_version)
            .await
            .with_context(|| format!("repo {}: failed to load iddag", self.repo_id))?;
        let idmap = self
            .idmap_factory
            .for_server(ctx, sc_version.idmap_version, &iddag)?;
        slog::debug!(
            ctx.logger(),
            "segmented changelog dag successfully loaded - repo_id: {}, idmap_version: {}, \
            iddag_version: {} ",
            self.repo_id,
            sc_version.idmap_version,
            sc_version.iddag_version,
        );
        let owned = OwnedSegmentedChangelog::new(iddag, idmap);
        Ok(owned)
    }
}

segmented_changelog_delegate!(SegmentedChangelogManager, |&self, ctx: &CoreContext| {
    // using load_owned for backwards compatibility until we deprecate upload algorithm
    // we would then remove this implementation for SegmentedChangelog
    self.load_owned(&ctx).await.with_context(|| {
        format!(
            "repo {}: error loading segmented changelog from save",
            self.repo_id
        )
    })?
});
