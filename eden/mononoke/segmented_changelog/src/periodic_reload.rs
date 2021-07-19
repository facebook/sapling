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
use async_trait::async_trait;

use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::manager::SegmentedChangelogManager;
use crate::{
    segmented_changelog_delegate, CloneData, Location, SegmentedChangelog, StreamCloneData,
};
use reloader::{Loader, Reloader};

struct SegmentedChangelogLoader {
    manager: SegmentedChangelogManager,
    ctx: CoreContext,
}

type LoadedSegmentedChangelog = Arc<dyn SegmentedChangelog + Send + Sync>;

#[async_trait]
impl Loader<LoadedSegmentedChangelog> for SegmentedChangelogLoader {
    async fn load(&mut self) -> Result<Option<LoadedSegmentedChangelog>> {
        Ok(Some(self.manager.load(&self.ctx).await?))
    }
}

pub struct PeriodicReloadSegmentedChangelog(Reloader<LoadedSegmentedChangelog>);

impl PeriodicReloadSegmentedChangelog {
    pub async fn start<L: Loader<LoadedSegmentedChangelog> + Send + Sync + 'static>(
        ctx: &CoreContext,
        period: Duration,
        loader: L,
    ) -> Result<Self> {
        Ok(Self(
            Reloader::reload_periodically_with_skew(ctx.clone(), period, loader).await?,
        ))
    }

    pub async fn start_from_manager(
        ctx: &CoreContext,
        period: Duration,
        manager: SegmentedChangelogManager,
    ) -> Result<Self> {
        Self::start(
            ctx,
            period,
            SegmentedChangelogLoader {
                manager,
                ctx: ctx.clone(),
            },
        )
        .await
    }

    #[cfg(test)]
    pub async fn wait_for_update(&self) {
        self.0.wait_for_update().await
    }
}

segmented_changelog_delegate!(PeriodicReloadSegmentedChangelog, |
    &self,
    ctx: &CoreContext,
| { self.0.load() });
