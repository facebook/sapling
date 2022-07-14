/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::format_err;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;

use futures_stats::TimedFutureExt;

use bookmarks::Bookmarks;
use changeset_fetcher::ArcChangesetFetcher;
use context::CoreContext;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;

use crate::iddag::IdDagSaveStore;
use crate::idmap::IdMapFactory;
use crate::on_demand::OnDemandUpdateSegmentedChangelog;
use crate::owned::OwnedSegmentedChangelog;
use crate::segmented_changelog_delegate;
use crate::types::SegmentedChangelogVersion;
use crate::version_store::SegmentedChangelogVersionStore;
use crate::CloneData;
use crate::CloneHints;
use crate::Location;
use crate::SeedHead;
use crate::SegmentedChangelog;

pub enum SegmentedChangelogType {
    OnDemand {
        update_to_master_bookmark_period: Option<Duration>,
    },
    #[cfg(test)]
    Owned,
}

#[facet::facet]
pub struct SegmentedChangelogManager {
    repo_id: RepositoryId,
    sc_version_store: SegmentedChangelogVersionStore,
    iddag_save_store: IdDagSaveStore,
    idmap_factory: IdMapFactory,
    changeset_fetcher: ArcChangesetFetcher,
    bookmarks: Arc<dyn Bookmarks>,
    seed_heads: Vec<SeedHead>,
    segmented_changelog_type: SegmentedChangelogType,
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
        segmented_changelog_type: SegmentedChangelogType,
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
            segmented_changelog_type,
            clone_hints,
        }
    }

    pub async fn load(
        &self,
        ctx: &CoreContext,
    ) -> Result<(
        Arc<dyn SegmentedChangelog + Send + Sync>,
        SegmentedChangelogVersion,
    )> {
        let monitored = async {
            let (asc, sc_version): (Arc<dyn SegmentedChangelog + Send + Sync>, _) =
                match self.segmented_changelog_type {
                    SegmentedChangelogType::OnDemand {
                        update_to_master_bookmark_period,
                    } => {
                        let (on_demand, sc_version) = self.load_ondemand_update(ctx).await?;
                        let on_demand: Arc<dyn SegmentedChangelog + Send + Sync> =
                            match update_to_master_bookmark_period {
                                None => on_demand,
                                Some(period) => Arc::new(
                                    on_demand.with_periodic_update_to_master_bookmark(ctx, period),
                                ),
                            };
                        (on_demand, sc_version)
                    }
                    #[cfg(test)]
                    SegmentedChangelogType::Owned => {
                        let (sc, sc_version) = self.load_owned(ctx).await?;
                        (Arc::new(sc), sc_version)
                    }
                };
            Ok((asc, sc_version))
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
    ) -> Result<(
        Arc<OnDemandUpdateSegmentedChangelog>,
        SegmentedChangelogVersion,
    )> {
        let (owned, sc_version) = self.load_owned(ctx).await.with_context(|| {
            format!("repo {}: failed to load segmented changelog", self.repo_id)
        })?;
        Ok((
            Arc::new(OnDemandUpdateSegmentedChangelog::new(
                ctx.clone(),
                self.repo_id,
                owned.iddag,
                owned.idmap,
                Arc::clone(&self.changeset_fetcher),
                Arc::clone(&self.bookmarks),
                self.seed_heads.clone(),
                self.clone_hints.clone(),
            )?),
            sc_version,
        ))
    }

    // public for builder only
    pub async fn load_owned(
        &self,
        ctx: &CoreContext,
    ) -> Result<(OwnedSegmentedChangelog, SegmentedChangelogVersion)> {
        let sc_version = self.latest_version(ctx).await?;
        let iddag = self
            .iddag_save_store
            .load(ctx, sc_version.iddag_version)
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
        Ok((owned, sc_version))
    }

    pub async fn latest_version(&self, ctx: &CoreContext) -> Result<SegmentedChangelogVersion> {
        self.sc_version_store
            .get(ctx)
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
            })
    }

    /// Checks if given changeset is indexed by given segmented changelog version.
    pub async fn check_if_changeset_indexed(
        &self,
        ctx: &CoreContext,
        sc_version: &SegmentedChangelogVersion,
        cs_id: ChangesetId,
    ) -> Result<bool> {
        let iddag = self
            .iddag_save_store
            .load(ctx, sc_version.iddag_version)
            .await
            .with_context(|| format!("repo {}: failed to load iddag", self.repo_id))?;
        let idmap = self
            .idmap_factory
            .for_server(ctx, sc_version.idmap_version, &iddag)?;
        let result = idmap.find_dag_id(ctx, cs_id).await?;
        Ok(result.is_some())
    }
}

segmented_changelog_delegate!(SegmentedChangelogManager, |&self, ctx: &CoreContext| {
    // using load_owned for backwards compatibility until we deprecate upload algorithm
    // we would then remove this implementation for SegmentedChangelog
    let (sc, _sc_version) = self.load_owned(ctx).await.with_context(|| {
        format!(
            "repo {}: error loading segmented changelog from save",
            self.repo_id
        )
    })?;
    sc
});
