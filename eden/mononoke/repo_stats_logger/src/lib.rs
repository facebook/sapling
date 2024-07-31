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
use metaconfig_types::ArcRepoConfig;
use mononoke_types::ChangesetId;
use repo_derived_data::ArcRepoDerivedData;
use sharding_ext::encode_repo_name;
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
        repo_config: ArcRepoConfig,
        bookmarks: ArcBookmarks,
        repo_blobstore: Arc<dyn Blobstore>,
        repo_derived_data: ArcRepoDerivedData,
    ) -> Result<Self, Error> {
        let ctx = CoreContext::new_for_bulk_processing(fb, logger.clone());

        let fut = async move {
            loop {
                let interval = Duration::from_secs(60);
                tokio::time::sleep(interval).await;

                let repo_key = encode_repo_name(&repo_name);
                let bookmark_name =
                    get_repo_bookmark_name(repo_config.clone()).expect("invalid bookmark name");
                let default_repo_objects_count =
                    get_repo_default_objects_count(repo_config.clone());

                match get_repo_objects_count(
                    &ctx,
                    &bookmark_name,
                    bookmarks.clone(),
                    repo_blobstore.clone(),
                    repo_derived_data.clone(),
                    default_repo_objects_count,
                )
                .await
                {
                    Ok(count) => {
                        let over = justknobs::get_as::<i64>(
                            "scm/mononoke:scs_override_repo_objects_count",
                            Some(&repo_name),
                        )
                        .unwrap_or(0);
                        let count = if over > 0 { over } else { count };
                        STATS::repo_objects_count.set_value(fb, count, (repo_key,));
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

    // A null implementation that does nothing. Useful for tests.
    pub fn noop() -> Self {
        Self {
            abort_handle: AbortHandle::new_pair().0,
        }
    }
}

fn get_repo_bookmark_name(
    repo_config: Arc<metaconfig_types::RepoConfig>,
) -> Result<BookmarkKey, Error> {
    let bookmark_name = repo_config
        .bookmark_name_for_objects_count
        .clone()
        .unwrap_or("master".to_string());
    BookmarkKey::new(bookmark_name)
}

fn get_repo_default_objects_count(repo_config: Arc<metaconfig_types::RepoConfig>) -> i64 {
    let default_repo_object_count =
        justknobs::get_as::<i64>("scm/mononoke:scs_default_repo_objects_count", None)
            .unwrap_or(DEFAULT_REPO_OBJECTS_COUNT);
    repo_config
        .default_objects_count
        .clone()
        .unwrap_or(default_repo_object_count)
}

async fn get_repo_objects_count(
    ctx: &CoreContext,
    bookmark_name: &BookmarkKey,
    bookmarks: ArcBookmarks,
    repo_blobstore: Arc<dyn Blobstore>,
    repo_derived_data: ArcRepoDerivedData,
    default_repo_object_count: i64,
) -> Result<i64, Error> {
    let maybe_bookmark = bookmarks.get(ctx.clone(), bookmark_name).await?;
    if let Some(cs_id) = maybe_bookmark {
        get_descendant_count(
            ctx,
            repo_blobstore.clone(),
            repo_derived_data.clone(),
            cs_id,
        )
        .await
    } else {
        Ok(default_repo_object_count)
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

impl Drop for RepoStatsLogger {
    fn drop(&mut self) {
        self.abort_handle.abort()
    }
}
