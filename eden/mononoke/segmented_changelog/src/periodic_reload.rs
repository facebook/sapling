/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use tokio::sync::Notify;
use tokio::time::Instant;

use futures_ext::future::{spawn_controlled, ControlledHandle};

use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::manager::SegmentedChangelogManager;
use crate::{
    segmented_changelog_delegate, CloneData, Location, SegmentedChangelog, StreamCloneData,
};

pub struct PeriodicReloadSegmentedChangelog {
    sc: Arc<ArcSwap<Arc<dyn SegmentedChangelog + Send + Sync>>>,
    _handle: ControlledHandle,
    #[allow(dead_code)] // useful for testing
    update_notify: Arc<Notify>,
}

impl PeriodicReloadSegmentedChangelog {
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
