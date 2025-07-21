/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use cloned::cloned;
use context::CoreContext;
use mononoke_macros::mononoke;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use mononoke_types::hash::GitSha1;
use repo_update_logger::PlainBookmarkInfo;
use slog::error;
use slog::info;
use stats::define_stats;
use stats::prelude::TimeseriesStatic;
use tokio::sync::broadcast::Receiver;
use tokio::task::JoinHandle;

use crate::BonsaiTagMapping;
use crate::BonsaiTagMappingEntry;
use crate::Freshness;

type Swappable<T> = Arc<ArcSwap<T>>;

define_stats! {
    prefix = "mononoke.tags_cache_update";
    update_failure_count: timeseries(Average, Sum, Count),
}

pub struct CachedBonsaiTagMapping {
    inner: Arc<dyn BonsaiTagMapping>,
    entries: Swappable<Vec<BonsaiTagMappingEntry>>,
    updater_task: JoinHandle<()>,
}

#[allow(dead_code)]
impl CachedBonsaiTagMapping {
    pub async fn new(
        ctx: &CoreContext,
        inner: Arc<dyn BonsaiTagMapping>,
        update_notification_receiver: Receiver<PlainBookmarkInfo>,
        logger: slog::Logger,
    ) -> Result<Self> {
        let initial_entries = inner
            .get_all_entries(ctx)
            .await
            .context("Error while getting initial set of bonsai tag mapping entries")?;
        let entries = Arc::new(ArcSwap::from_pointee(initial_entries));
        let updater_task = mononoke::spawn_task({
            cloned!(entries, inner);
            let ctx = ctx.clone();
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
    entries: Swappable<Vec<BonsaiTagMappingEntry>>,
    bonsai_tag_mapping: Arc<dyn BonsaiTagMapping>,
    mut update_notification_receiver: Receiver<PlainBookmarkInfo>,
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
                    "Received update notification from scribe for updating tags cache"
                );
                match bonsai_tag_mapping.get_all_entries(&ctx).await {
                    Ok(new_entries) => {
                        let new_entries = Arc::new(new_entries);
                        entries.store(new_entries);
                        info!(
                            logger,
                            "Successfully updated the cache with new entries from the inner bonsai tag mapping"
                        );
                    }
                    Err(e) => {
                        error!(
                            logger,
                            "Failure in updating the cache with new entries from the inner bonsai tag mapping: {:?}",
                            e
                        );
                        // TODO(rajshar): Add ODS based alerting for this if the number of errors is high
                        STATS::update_failure_count.add_value(1);
                    }
                }
            }
            Err(e) => {
                error!(
                    logger,
                    "Failure in receiving notification from tags scribe category. Error: {:?}", e
                );
                // TODO(rajshar): Add ODS based alerting for this if the number of errors is high
                STATS::update_failure_count.add_value(1);
            }
        }
    }
}

impl Drop for CachedBonsaiTagMapping {
    fn drop(&mut self) {
        // Need to abort the task before dropping the cache mapping
        self.updater_task.abort();
    }
}

#[async_trait]
impl BonsaiTagMapping for CachedBonsaiTagMapping {
    /// The repository for which this mapping has been created
    fn repo_id(&self) -> RepositoryId {
        self.inner.repo_id()
    }

    /// Fetch all the tag mapping entries for the given repo
    async fn get_all_entries(&self, ctx: &CoreContext) -> Result<Vec<BonsaiTagMappingEntry>> {
        if justknobs::eval("scm/mononoke:enable_bonsai_tag_mapping_caching", None, None)
            .unwrap_or(false)
        {
            Ok(self.entries.load_full().to_vec())
        } else {
            self.inner.get_all_entries(ctx).await
        }
    }

    /// Fetch the tag mapping entries corresponding to the input changeset ids
    /// for the given repo
    async fn get_entries_by_changesets(
        &self,
        ctx: &CoreContext,
        changeset_ids: Vec<ChangesetId>,
    ) -> Result<Vec<BonsaiTagMappingEntry>> {
        if justknobs::eval("scm/mononoke:enable_bonsai_tag_mapping_caching", None, None)
            .unwrap_or(false)
        {
            let changeset_ids = changeset_ids.into_iter().collect::<HashSet<_>>();
            Ok(self
                .entries
                .load()
                .iter()
                .filter(|&entry| changeset_ids.contains(&entry.changeset_id))
                .cloned()
                .collect())
        } else {
            self.inner
                .get_entries_by_changesets(ctx, changeset_ids)
                .await
        }
    }

    /// Fetch the tag mapping entry corresponding to the tag name in the
    /// given repo, if one exists
    async fn get_entry_by_tag_name(
        &self,
        ctx: &CoreContext,
        tag_name: String,
        freshness: Freshness,
    ) -> Result<Option<BonsaiTagMappingEntry>> {
        match freshness {
            // If the caller wants the latest view of data, we delegate to the inner bonsai tag mapping
            // instead of relying on the cache
            Freshness::Latest => {
                self.inner
                    .get_entry_by_tag_name(ctx, tag_name, freshness)
                    .await
            }
            Freshness::MaybeStale => {
                if justknobs::eval("scm/mononoke:enable_bonsai_tag_mapping_caching", None, None)
                    .unwrap_or(false)
                {
                    let entry = self
                        .entries
                        .load()
                        .iter()
                        .find(|&entry| entry.tag_name == tag_name)
                        .cloned();
                    Ok(entry)
                } else {
                    self.inner
                        .get_entry_by_tag_name(ctx, tag_name, freshness)
                        .await
                }
            }
        }
    }

    /// Fetch the tag mapping entries corresponding to the input tag hashes
    async fn get_entries_by_tag_hashes(
        &self,
        ctx: &CoreContext,
        tag_hashes: Vec<GitSha1>,
    ) -> Result<Vec<BonsaiTagMappingEntry>> {
        if justknobs::eval("scm/mononoke:enable_bonsai_tag_mapping_caching", None, None)
            .unwrap_or(false)
        {
            let tag_hashes = tag_hashes.into_iter().collect::<HashSet<_>>();
            Ok(self
                .entries
                .load()
                .iter()
                .filter(|&entry| tag_hashes.contains(&entry.tag_hash))
                .cloned()
                .collect())
        } else {
            self.inner.get_entries_by_tag_hashes(ctx, tag_hashes).await
        }
    }

    /// Add new tag name to bonsai changeset mappings
    async fn add_or_update_mappings(
        &self,
        ctx: &CoreContext,
        entries: Vec<BonsaiTagMappingEntry>,
    ) -> Result<()> {
        // Writes are directly delegated to inner bonsai tag mapping
        self.inner.add_or_update_mappings(ctx, entries).await
    }

    /// Delete existing bonsai tag mappings based on the input tag names
    async fn delete_mappings_by_name(
        &self,
        ctx: &CoreContext,
        tag_names: Vec<String>,
    ) -> Result<()> {
        // Writes are directly delegated to inner bonsai tag mapping
        self.inner.delete_mappings_by_name(ctx, tag_names).await
    }
}
