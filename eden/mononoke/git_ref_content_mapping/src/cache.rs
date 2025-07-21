/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use cloned::cloned;
use context::CoreContext;
use mononoke_macros::mononoke;
use mononoke_types::RepositoryId;
use repo_update_logger::GitContentRefInfo;
use slog::error;
use slog::info;
use stats::define_stats;
use stats::prelude::TimeseriesStatic;
use tokio::sync::broadcast::Receiver;
use tokio::task::JoinHandle;

use crate::GitRefContentMapping;
use crate::GitRefContentMappingEntry;

type Swappable<T> = Arc<ArcSwap<T>>;

define_stats! {
    prefix = "mononoke.content_refs_cache_update";
    update_failure_count: timeseries(Average, Sum, Count),
}

pub struct CachedGitRefContentMapping {
    inner: Arc<dyn GitRefContentMapping>,
    entries: Swappable<Vec<GitRefContentMappingEntry>>,
    updater_task: JoinHandle<()>,
}

#[allow(dead_code)]
impl CachedGitRefContentMapping {
    pub async fn new(
        ctx: &CoreContext,
        inner: Arc<dyn GitRefContentMapping>,
        update_notification_receiver: Receiver<GitContentRefInfo>,
        logger: slog::Logger,
    ) -> Result<Self> {
        let initial_entries = inner
            .get_all_entries(ctx)
            .await
            .context("Error while getting initial set of git ref content mapping entries")?;
        let entries = Arc::new(ArcSwap::from_pointee(initial_entries));
        let updater_task = mononoke::spawn_task({
            cloned!(ctx, entries, inner);
            update_cache(ctx, entries, inner, update_notification_receiver, logger)
        });
        Ok(Self {
            inner,
            entries,
            updater_task,
        })
    }
}

async fn update_cache(
    ctx: CoreContext,
    entries: Swappable<Vec<GitRefContentMappingEntry>>,
    bonsai_tag_mapping: Arc<dyn GitRefContentMapping>,
    mut update_notification_receiver: Receiver<GitContentRefInfo>,
    logger: slog::Logger,
) {
    loop {
        let fallible_notification = update_notification_receiver
            .recv()
            .await
            .context("Error while receiving update notification");
        match fallible_notification {
            Ok(_) => {
                info!(
                    logger,
                    "Received update notification from scribe for updating content refs cache"
                );
                match bonsai_tag_mapping.get_all_entries(&ctx).await {
                    Ok(new_entries) => {
                        let new_entries = Arc::new(new_entries);
                        entries.store(new_entries);
                        info!(
                            logger,
                            "Successfully updated the cache with new entries from the inner git ref content mapping"
                        );
                    }
                    Err(e) => {
                        error!(
                            logger,
                            "Failure in updating the cache with new entries from the inner git ref content mapping: {:?}",
                            e
                        );
                        STATS::update_failure_count.add_value(1);
                    }
                }
            }
            Err(e) => {
                error!(
                    logger,
                    "Failure in receiving notification from tags scribe category. Error: {:?}", e
                );
                STATS::update_failure_count.add_value(1);
            }
        }
    }
}

impl Drop for CachedGitRefContentMapping {
    fn drop(&mut self) {
        // Need to abort the task before dropping the cache mapping
        self.updater_task.abort();
    }
}

#[async_trait]
impl GitRefContentMapping for CachedGitRefContentMapping {
    /// The repository for which this mapping has been created
    fn repo_id(&self) -> RepositoryId {
        self.inner.repo_id()
    }

    /// Fetch all the tag mapping entries for the given repo
    async fn get_all_entries(&self, ctx: &CoreContext) -> Result<Vec<GitRefContentMappingEntry>> {
        if justknobs::eval(
            "scm/mononoke:enable_git_ref_content_mapping_caching",
            None,
            None,
        )
        .unwrap_or(false)
        {
            Ok(self.entries.load_full().to_vec())
        } else {
            self.inner.get_all_entries(ctx).await
        }
    }

    async fn get_entry_by_ref_name(
        &self,
        ctx: &CoreContext,
        ref_name: String,
    ) -> Result<Option<GitRefContentMappingEntry>> {
        if justknobs::eval(
            "scm/mononoke:enable_git_ref_content_mapping_caching",
            None,
            None,
        )
        .unwrap_or(false)
        {
            let entry = self
                .entries
                .load()
                .iter()
                .find(|&entry| entry.ref_name == ref_name)
                .cloned();
            Ok(entry)
        } else {
            self.inner.get_entry_by_ref_name(ctx, ref_name).await
        }
    }

    async fn add_or_update_mappings(
        &self,
        ctx: &CoreContext,
        entries: Vec<GitRefContentMappingEntry>,
    ) -> Result<()> {
        // Writes are directly delegated to inner git ref content mapping
        self.inner.add_or_update_mappings(ctx, entries).await
    }

    async fn delete_mappings_by_name(
        &self,
        ctx: &CoreContext,
        ref_names: Vec<String>,
    ) -> Result<()> {
        // Writes are directly delegated to inner git ref content mapping
        self.inner.delete_mappings_by_name(ctx, ref_names).await
    }
}
