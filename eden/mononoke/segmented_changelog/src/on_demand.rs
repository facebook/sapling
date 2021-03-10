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
use async_trait::async_trait;
use futures::future::BoxFuture;
use futures::{FutureExt, TryFutureExt};
use parking_lot::Mutex;
use tokio::sync::{Notify, RwLock};

use cloned::cloned;
use dag::{self, CloneData, InProcessIdDag, Location};
use futures_ext::future::{spawn_controlled, ControlledHandle, FbTryFutureExt, TryShared};
use stats::prelude::*;

use bookmarks::{BookmarkName, Bookmarks};
use changeset_fetcher::ChangesetFetcher;
use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::idmap::IdMap;
use crate::owned::OwnedSegmentedChangelog;
use crate::read_only::ReadOnlySegmentedChangelog;
use crate::update::{prepare_incremental_iddag_update, update_iddag};
use crate::{segmented_changelog_delegate, SegmentedChangelog, StreamCloneData};

define_stats! {
    prefix = "mononoke.segmented_changelog.ondemand";
    location_to_changeset_id: timeseries(Sum),
    changeset_id_to_location: timeseries(Sum),
    missing_notification_handle: timeseries(Sum),
}

pub struct OnDemandUpdateSegmentedChangelog {
    iddag: Arc<RwLock<InProcessIdDag>>,
    idmap: Arc<dyn IdMap>,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    ongoing_update: Arc<Mutex<Option<TryShared<BoxFuture<'static, Result<()>>>>>>,
}

impl OnDemandUpdateSegmentedChangelog {
    pub fn new(
        iddag: InProcessIdDag,
        idmap: Arc<dyn IdMap>,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
    ) -> Self {
        Self {
            iddag: Arc::new(RwLock::new(iddag)),
            idmap,
            changeset_fetcher,
            ongoing_update: Arc::new(Mutex::new(None)),
        }
    }

    pub fn from_owned(
        owned: OwnedSegmentedChangelog,
        changeset_fetcher: Arc<dyn ChangesetFetcher>,
    ) -> Self {
        Self::new(owned.iddag, owned.idmap, changeset_fetcher)
    }

    pub fn with_periodic_update_to_bookmark(
        self: Arc<Self>,
        ctx: &CoreContext,
        bookmarks: Arc<dyn Bookmarks>,
        bookmark_name: BookmarkName,
        period: Duration,
    ) -> PeriodicUpdateDag {
        PeriodicUpdateDag::for_bookmark(ctx, self, bookmarks, bookmark_name, period)
    }

    // Updating the Dag has 3 phases:
    // * loading the data that is required for the update;
    // * updating the IdMap;
    // * updating the IdDag;
    //
    // The Dag can function well for serving requests as long as the commits involved have been
    // built so we want to have easy read access to both the IdMap and the IdDag. The IdMap is a
    // very simple structure and because it's described as an Arc<dyn IdMap> we push the update
    // locking logic to the storage.  The IdDag is a complicated structure that we ask to update
    // itself. Those functions take mutable references. Updating the storage of the iddag to hide
    // the complexities of locking is more difficult. We deal with the IdDag directly by wrapping
    // it in a RwLock. The RwLock allows for easy read access which we expect to be the predominant
    // access pattern.
    //
    // Updates to the dag are not completely stable so racing updates can have conflicting results.
    // In case of conflics one of the update processes would have to restart. It's easier to reason
    // about the process if we just allow one "thread" to start an update process. The update
    // process is locked by a sync mutex. The "threads" that fail the race to update are asked to
    // wait until the ongoing update is complete. The waiters will poll on a shared future that
    // tracks the ongoing dag update. After the update is complete the waiters will go back to
    // checking if the data they have is available in the dag. It is possible that the dag is
    // updated in between determining that the an update is needed and acquiring the ongoing_update
    // lock. This is fine because the update building process checks the state of dag before the
    // dag and updates only what is necessary if necessary.
    /// Update the Dag to incorporate the commit pointed to by head.
    /// Returns true if it performed an actual update false if it simply waited for ongoing update
    /// to finish.
    async fn try_update(&self, ctx: &CoreContext, head: ChangesetId) -> Result<bool> {
        let to_wait = {
            let mut ongoing_update = self.ongoing_update.lock();

            if let Some(fut) = &*ongoing_update {
                fut.clone().map(|_| Ok(false)).boxed()
            } else {
                cloned!(ctx, self.iddag, self.idmap, self.changeset_fetcher);
                let task_ongoing_update = self.ongoing_update.clone();
                let update_task = async move {
                    let result =
                        the_actual_update(ctx, iddag, idmap, changeset_fetcher, head).await;
                    let mut ongoing_update = task_ongoing_update.lock();
                    *ongoing_update = None;
                    result
                }
                .boxed()
                .try_shared();

                *ongoing_update = Some(update_task.clone());

                update_task.map_ok(|_| true).boxed()
            }
        };
        Ok(to_wait.await?)
    }

    async fn build_up_to_cs(&self, ctx: &CoreContext, cs_id: ChangesetId) -> Result<()> {
        loop {
            if let Some(vertex) = self
                .idmap
                .find_vertex(ctx, cs_id)
                .await
                .context("fetching vertex for csid")?
            {
                // Note. This will result in two read locks being acquired for functions that call
                // into build_up. It would be nice to get to one lock being acquired. I tried but
                // had some issues with lifetimes :).
                let iddag = self.iddag.read().await;
                if iddag.contains_id(vertex)? {
                    return Ok(());
                }
            }
            if self.try_update(ctx, cs_id).await? {
                return Ok(());
            }
        }
    }
}

async fn the_actual_update(
    ctx: CoreContext,
    iddag: Arc<RwLock<InProcessIdDag>>,
    idmap: Arc<dyn IdMap>,
    changeset_fetcher: Arc<dyn ChangesetFetcher>,
    head: ChangesetId,
) -> Result<()> {
    let (head_vertex, idmap_update_state) = {
        let iddag = iddag.read().await;
        prepare_incremental_iddag_update(&ctx, &iddag, &idmap, &changeset_fetcher, head)
            .await
            .context("error preparing an incremental update for iddag")?
    };
    if let Some((start_state, mem_idmap)) = idmap_update_state {
        let mut iddag = iddag.write().await;
        update_iddag(&ctx, &mut iddag, &start_state, &mem_idmap, head_vertex)?;
    }
    Ok(())
}

#[async_trait]
impl SegmentedChangelog for OnDemandUpdateSegmentedChangelog {
    async fn location_to_many_changeset_ids(
        &self,
        ctx: &CoreContext,
        location: Location<ChangesetId>,
        count: u64,
    ) -> Result<Vec<ChangesetId>> {
        STATS::location_to_changeset_id.add_value(1);
        // Location descendant may not be the ideal entry to build up to, it could be a good idea
        // to have client_head here too.
        self.build_up_to_cs(ctx, location.descendant)
            .await
            .context("error while getting an up to date dag")?;
        let iddag = self.iddag.read().await;
        let read_dag = ReadOnlySegmentedChangelog::new(&iddag, self.idmap.clone());
        read_dag
            .location_to_many_changeset_ids(ctx, location, count)
            .await
    }

    async fn many_changeset_ids_to_locations(
        &self,
        ctx: &CoreContext,
        client_head: ChangesetId,
        cs_ids: Vec<ChangesetId>,
    ) -> Result<HashMap<ChangesetId, Location<ChangesetId>>> {
        STATS::changeset_id_to_location.add_value(1);
        self.build_up_to_cs(ctx, client_head)
            .await
            .context("error while getting an up to date dag")?;
        let iddag = self.iddag.read().await;
        let read_dag = ReadOnlySegmentedChangelog::new(&iddag, self.idmap.clone());
        read_dag
            .many_changeset_ids_to_locations(ctx, client_head, cs_ids)
            .await
    }

    async fn clone_data(&self, ctx: &CoreContext) -> Result<CloneData<ChangesetId>> {
        let iddag = self.iddag.read().await;
        let read_dag = ReadOnlySegmentedChangelog::new(&iddag, self.idmap.clone());
        read_dag.clone_data(ctx).await
    }

    async fn full_idmap_clone_data(
        &self,
        ctx: &CoreContext,
    ) -> Result<StreamCloneData<ChangesetId>> {
        let iddag = self.iddag.read().await;
        let read_dag = ReadOnlySegmentedChangelog::new(&iddag, self.idmap.clone());
        read_dag.full_idmap_clone_data(ctx).await
    }
}

pub struct PeriodicUpdateDag {
    on_demand_update_sc: Arc<OnDemandUpdateSegmentedChangelog>,
    _handle: ControlledHandle,
    #[allow(dead_code)] // useful for testing
    notify: Arc<Notify>,
}

impl PeriodicUpdateDag {
    pub fn for_bookmark(
        ctx: &CoreContext,
        on_demand_update_sc: Arc<OnDemandUpdateSegmentedChangelog>,
        bookmarks: Arc<dyn Bookmarks>,
        bookmark_name: BookmarkName,
        period: Duration,
    ) -> Self {
        let notify = Arc::new(Notify::new());
        let _handle = spawn_controlled({
            let ctx = ctx.clone();
            let my_dag = Arc::clone(&on_demand_update_sc);
            let notify = Arc::clone(&notify);
            async move {
                let mut interval = tokio::time::interval(period);
                loop {
                    let _ = interval.tick().await;
                    if let Err(err) =
                        update_dag_from_bookmark(&ctx, &my_dag, &*bookmarks, &bookmark_name).await
                    {
                        slog::error!(
                            ctx.logger(),
                            "failed to update segmented changelog dag: {:?}",
                            err
                        );
                    }
                    notify.notify();
                }
            }
        });
        Self {
            on_demand_update_sc,
            _handle,
            notify,
        }
    }

    #[cfg(test)]
    pub async fn wait_for_update(&self) {
        self.notify.notified().await;
    }
}

async fn update_dag_from_bookmark(
    ctx: &CoreContext,
    on_demand_update_sc: &OnDemandUpdateSegmentedChangelog,
    bookmarks: &dyn Bookmarks,
    bookmark_name: &BookmarkName,
) -> Result<()> {
    let bookmark_cs = bookmarks
        .get(ctx.clone(), bookmark_name)
        .await
        .with_context(|| {
            format!(
                "error while fetching changeset for bookmark {}",
                bookmark_name
            )
        })?
        .ok_or_else(|| format_err!("'{}' bookmark could not be found", bookmark_name))?;
    on_demand_update_sc.try_update(&ctx, bookmark_cs).await?;
    Ok(())
}

segmented_changelog_delegate!(PeriodicUpdateDag, |&self, ctx: &CoreContext| {
    &self.on_demand_update_sc
});
