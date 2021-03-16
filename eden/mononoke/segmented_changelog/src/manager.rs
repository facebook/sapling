/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{format_err, Context, Result};
use arc_swap::ArcSwap;
use async_trait::async_trait;
use tokio::sync::Notify;
use tokio::time::Instant;

use bookmarks::{BookmarkName, Bookmarks};
use changeset_fetcher::ChangesetFetcher;
use dag::Location;
use futures_ext::future::{spawn_controlled, ControlledHandle};

use context::CoreContext;
use mononoke_types::{ChangesetId, RepositoryId};

use crate::iddag::IdDagSaveStore;
use crate::idmap::IdMapFactory;
use crate::on_demand::OnDemandUpdateSegmentedChangelog;
use crate::owned::OwnedSegmentedChangelog;
use crate::version_store::SegmentedChangelogVersionStore;
use crate::{segmented_changelog_delegate, CloneData, SegmentedChangelog, StreamCloneData};

pub struct SegmentedChangelogManager {
    repo_id: RepositoryId,
    sc_version_store: SegmentedChangelogVersionStore,
    iddag_save_store: IdDagSaveStore,
    idmap_factory: IdMapFactory,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    bookmarks: Arc<dyn Bookmarks>,
    bookmark_name: BookmarkName,
    update_to_master_bookmark_period: Option<Duration>,
}

impl SegmentedChangelogManager {
    pub fn new(
        repo_id: RepositoryId,
        sc_version_store: SegmentedChangelogVersionStore,
        iddag_save_store: IdDagSaveStore,
        idmap_factory: IdMapFactory,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
        bookmarks: Arc<dyn Bookmarks>,
        bookmark_name: BookmarkName,
        update_to_master_bookmark_period: Option<Duration>,
    ) -> Self {
        Self {
            repo_id,
            sc_version_store,
            iddag_save_store,
            idmap_factory,
            changeset_fetcher,
            bookmarks,
            bookmark_name,
            update_to_master_bookmark_period,
        }
    }

    pub async fn load(&self, ctx: &CoreContext) -> Result<Arc<dyn SegmentedChangelog>> {
        let on_demand = self.load_ondemand_update(ctx).await?;
        let asc: Arc<dyn SegmentedChangelog> = match self.update_to_master_bookmark_period {
            None => on_demand,
            Some(period) => Arc::new(on_demand.with_periodic_update_to_bookmark(
                ctx,
                self.bookmarks.clone(),
                self.bookmark_name.clone(),
                period,
            )),
        };
        Ok(asc)
    }

    async fn load_ondemand_update(
        &self,
        ctx: &CoreContext,
    ) -> Result<Arc<OnDemandUpdateSegmentedChangelog>> {
        let owned = self.load_owned(ctx).await.with_context(|| {
            format!("repo {}: failed to load segmented changelog", self.repo_id)
        })?;
        Ok(Arc::new(OnDemandUpdateSegmentedChangelog::new(
            self.repo_id,
            owned.iddag,
            owned.idmap,
            Arc::clone(&self.changeset_fetcher),
        )))
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

pub struct PeriodicReloadSegmentedChangelog {
    sc: Arc<ArcSwap<Arc<dyn SegmentedChangelog>>>,
    _handle: ControlledHandle,
    #[allow(dead_code)] // useful for testing
    update_notify: Arc<Notify>,
}

impl PeriodicReloadSegmentedChangelog {
    #[allow(dead_code)]
    pub async fn start(
        ctx: &CoreContext,
        manager: SegmentedChangelogManager,
        period: Duration,
    ) -> Result<Self> {
        let sc = Arc::new(ArcSwap::from_pointee(manager.load(ctx).await?));
        let update_notify = Arc::new(Notify::new());
        let _handle = spawn_controlled({
            let ctx = ctx.clone();
            let my_sc = Arc::clone(&sc);
            let my_notify = Arc::clone(&update_notify);
            async move {
                let start = Instant::now() + period;
                let mut interval = tokio::time::interval_at(start, period);
                loop {
                    interval.tick().await;
                    match manager.load(&ctx).await {
                        Ok(sc) => my_sc.store(Arc::new(sc)),
                        Err(err) => {
                            slog::error!(
                                ctx.logger(),
                                "failed to load segmented changelog: {:?}",
                                err
                            );
                        }
                    }
                    my_notify.notify();
                }
            }
        });
        Ok(Self {
            sc,
            _handle,
            update_notify,
        })
    }

    #[cfg(test)]
    pub async fn wait_for_update(&self) {
        self.update_notify.notified().await;
    }
}

segmented_changelog_delegate!(PeriodicReloadSegmentedChangelog, |
    &self,
    ctx: &CoreContext,
| { self.sc.load() });
