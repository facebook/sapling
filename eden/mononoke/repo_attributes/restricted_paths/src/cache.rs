/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! In-memory cache for restricted paths manifest IDs.
//!
//! This module provides an in-memory cache that stores mappings from manifest IDs
//! to their associated restricted paths. The cache is periodically refreshed from
//! the database to reduce the number of DB queries for high-QPS operations.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;

use anyhow::Result;
use context::CoreContext;
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::select;
use metaconfig_types::RestrictedPathsConfig;
use mononoke_macros::mononoke;
use mononoke_types::NonRootMPath;
use mononoke_types::RepoPath;
use tracing::Instrument;

use crate::RestrictedPathManifestIdEntry;
use crate::manifest_id_store::ArcRestrictedPathsManifestIdStore;
use crate::manifest_id_store::ManifestId;
use crate::manifest_id_store::ManifestType;

/// Type alias for the manifest ID cache structure.
pub type ManifestIdCache =
    Arc<RwLock<HashMap<ManifestType, HashMap<ManifestId, Vec<NonRootMPath>>>>>;

/// The restricted paths cache maintains an in-memory copy of manifest ID mappings
/// that are refreshed periodically by querying the database.
pub struct RestrictedPathsManifestIdCache {
    /// The in-memory cache shared across threads
    cache: ManifestIdCache,
    /// Channel to terminate the background updater
    terminate: Option<oneshot::Sender<()>>,
    /// How often to refresh the cache from the database
    refresh_interval: Duration,
}

impl RestrictedPathsManifestIdCache {
    /// Create a new restricted paths cache and start the background updater.
    pub async fn new(
        ctx: &CoreContext,
        manifest_id_store: &ArcRestrictedPathsManifestIdStore,
        refresh_interval: Duration,
    ) -> Result<Self> {
        let cache = Arc::new(RwLock::new(HashMap::new()));
        let (sender, receiver) = oneshot::channel();

        // Perform initial cache refresh
        let updater = CacheUpdater {
            ctx: ctx.clone(),
            cache: cache.clone(),
            manifest_id_store: manifest_id_store.clone(),
        };

        tracing::debug!("Starting restricted paths cache updater");

        // Do initial refresh
        updater.refresh_cache().await?;

        // Spawn background updater thread. This runs in a separate OS thread,
        // so it won't be affected by tokio runtime scheduling
        updater.spawn(receiver, refresh_interval).await;

        Ok(Self {
            cache,
            terminate: Some(sender),
            refresh_interval,
        })
    }

    /// Get a reference to the cache for reading.
    pub fn cache(&self) -> &ManifestIdCache {
        &self.cache
    }

    /// Get the refresh interval.
    pub fn refresh_interval(&self) -> Duration {
        self.refresh_interval
    }
}

impl Drop for RestrictedPathsManifestIdCache {
    fn drop(&mut self) {
        // Signal the background updater to terminate
        if let Some(terminate) = self.terminate.take() {
            let _ = terminate.send(());
        }
    }
}

/// Builder for creating a RestrictedPathsManifestIdCache with custom configuration.
pub struct RestrictedPathsManifestIdCacheBuilder {
    ctx: CoreContext,
    manifest_id_store: ArcRestrictedPathsManifestIdStore,
    refresh_interval: Duration,
}

impl RestrictedPathsManifestIdCacheBuilder {
    /// Create a new cache builder with default settings.
    pub fn new(ctx: CoreContext, manifest_id_store: ArcRestrictedPathsManifestIdStore) -> Self {
        Self {
            ctx,
            manifest_id_store,
            refresh_interval: Duration::from_millis(
                RestrictedPathsConfig::default().cache_update_interval_ms,
            ),
        }
    }

    /// Set the cache refresh interval.
    pub fn with_refresh_interval(mut self, interval: Duration) -> Self {
        self.refresh_interval = interval;
        self
    }

    /// Build and initialize the cache.
    pub async fn build(self) -> Result<RestrictedPathsManifestIdCache> {
        RestrictedPathsManifestIdCache::new(
            &self.ctx,
            &self.manifest_id_store,
            self.refresh_interval,
        )
        .await
    }
}

/// Internal structure responsible for updating the cache from the database.
struct CacheUpdater {
    cache: ManifestIdCache,
    manifest_id_store: ArcRestrictedPathsManifestIdStore,
    ctx: CoreContext,
}

impl CacheUpdater {
    /// Refresh the cache by fetching all entries from the database.
    async fn refresh_cache(&self) -> Result<()> {
        // Fetch all entries from the database
        let entries = self.manifest_id_store.get_all_entries(&self.ctx).await?;

        // Build new cache structure
        let mut new_cache: HashMap<ManifestType, HashMap<ManifestId, Vec<NonRootMPath>>> =
            HashMap::new();

        for RestrictedPathManifestIdEntry {
            manifest_type,
            manifest_id,
            path,
            ..
        } in entries
        {
            let repo_path = RepoPath::dir(NonRootMPath::new(path.0)?)?;

            // Extract the NonRootMPath from the repo path
            let path = match repo_path {
                mononoke_types::RepoPath::DirectoryPath(non_root) => non_root,
                _ => {
                    continue;
                }
            };

            new_cache
                .entry(manifest_type)
                .or_insert_with(HashMap::new)
                .entry(manifest_id)
                .or_insert_with(Vec::new)
                .push(path);
        }

        // Atomically update the cache
        let mut cache = self.cache.write().unwrap();
        *cache = new_cache;

        Ok(())
    }

    /// Spawn a background thread that periodically refreshes the cache.
    pub async fn spawn(self, terminate: oneshot::Receiver<()>, refresh_interval: Duration) {
        let loop_fut = async move {
            loop {
                // Refresh the cache
                let refresh_result = self.refresh_cache().await;

                if let Err(err) = refresh_result {
                    tracing::error!("Failed to refresh restricted paths cache: {:#}", err);
                }

                // Sleep for the refresh interval
                tokio::time::sleep(refresh_interval).await;
            }
        }
        .boxed();

        let fut = async move {
            let _ = select(terminate, loop_fut).await; // select terminates when either of its inputs return
            tracing::debug!("Stopped restricted paths cache updater");
        }
        .instrument(tracing::debug_span!(
            "Restricted paths manifest id cache updater"
        ));

        mononoke::spawn_task(fut);
    }
}
