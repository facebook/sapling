/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use blobstore::Blobstore;
use blobstore::Loadable;
use bookmarks::ArcBookmarks;
use bookmarks::BookmarkKey;
use context::CoreContext;
use fbinit::FacebookInit;
use fsnodes::RootFsnodeId;
use futures::future::abortable;
use futures::future::AbortHandle;
use mononoke_types::ChangesetId;
use repo_derived_data::ArcRepoDerivedData;
use slog::warn;
use slog::Logger;
use stats::define_stats;
use stats::prelude::DynamicSingletonCounter;

define_stats! {
    prefix = "mononoke.app.repo.stats";
    repo_objects_count: dynamic_singleton_counter("{}.objects.count", (repo_name: String)),
}

const DEFAULT_REPO_OBJECTS_COUNT: i64 = 1_000_000;

#[derive(Clone)]
#[facet::facet]
pub struct RepoStatsLogger {
    abort_handle: AbortHandle,
}

impl RepoStatsLogger {
    pub async fn new(
        fb: FacebookInit,
        logger: Logger,
        repo_name: String,
        bookmarks: ArcBookmarks,
        repo_blobstore: Arc<dyn Blobstore>,
        repo_derived_data: ArcRepoDerivedData,
    ) -> Result<Self, Error> {
        let ctx = CoreContext::new_for_bulk_processing(fb, logger.clone());

        // XXX Not all repos have a master bookmark. Make it configurable?
        let master = BookmarkKey::new("master")?;

        let fut = async move {
            loop {
                let interval = Duration::from_secs(60);
                tokio::time::sleep(interval).await;

                match Self::get_repo_objects_count(
                    &ctx,
                    &master,
                    bookmarks.clone(),
                    repo_blobstore.clone(),
                    repo_derived_data.clone(),
                )
                .await
                {
                    Ok(count) => {
                        STATS::repo_objects_count.set_value(fb, count, (repo_name.clone(),));
                    }
                    Err(e) => {
                        warn!(ctx.logger(), "Finding bookmark for {}: {}", repo_name, e);
                    }
                }
            }
        };

        let (fut, abort_handle) = abortable(fut);
        tokio::spawn(fut);

        Ok(Self { abort_handle })
    }

    async fn get_repo_objects_count(
        ctx: &CoreContext,
        master: &BookmarkKey,
        bookmarks: ArcBookmarks,
        repo_blobstore: Arc<dyn Blobstore>,
        repo_derived_data: ArcRepoDerivedData,
    ) -> Result<i64, Error> {
        let maybe_bookmark = bookmarks.get(ctx.clone(), master).await?;
        if let Some(cs_id) = maybe_bookmark {
            Self::get_descendant_count(
                ctx,
                repo_blobstore.clone(),
                repo_derived_data.clone(),
                cs_id,
            )
            .await
        } else {
            Ok(DEFAULT_REPO_OBJECTS_COUNT)
        }
    }

    async fn get_descendant_count(
        ctx: &CoreContext,
        repo_blobstore: Arc<dyn Blobstore>,
        repo_derived_data: ArcRepoDerivedData,
        cs_id: ChangesetId,
    ) -> Result<i64, Error> {
        let root_fsnode_id = repo_derived_data.derive::<RootFsnodeId>(ctx, cs_id).await?;
        let count = root_fsnode_id
            .fsnode_id()
            .load(ctx, &repo_blobstore)
            .await?
            .summary()
            .descendant_files_count;
        Ok(i64::try_from(count).expect("file count overflows i64"))
    }

    // A null implementation that does nothing. Useful for tests.
    pub fn noop() -> Self {
        Self {
            abort_handle: AbortHandle::new_pair().0,
        }
    }
}

impl Drop for RepoStatsLogger {
    fn drop(&mut self) {
        self.abort_handle.abort()
    }
}
