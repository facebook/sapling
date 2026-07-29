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
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;

use anyhow::Result;
use context::CoreContext;
use futures::FutureExt;
use futures::channel::oneshot;
use futures::future::select;
use metaconfig_types::RestrictedPathsConfig;
use metaconfig_types::RestrictedPathsManifestIdStoreConfig;
use mononoke_macros::mononoke;
use mononoke_types::NonRootMPath;
use mononoke_types::RepoPath;
use tracing::Instrument;

use crate::RestrictedPathManifestIdEntry;
use crate::manifest_id_store::ArcRestrictedPathsManifestIdStore;
use crate::manifest_id_store::ManifestType;
use crate::manifest_id_store::RestrictedManifestId;

/// Type alias for the manifest ID cache structure.
pub type ManifestIdCache =
    Arc<RwLock<HashMap<ManifestType, HashMap<RestrictedManifestId, HashSet<NonRootMPath>>>>>;

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
        config: RestrictedPathsManifestIdStoreConfig,
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
        let refresh_interval = Duration::from_millis(config.cache_update_interval_ms);
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
    config: RestrictedPathsManifestIdStoreConfig,
}

impl RestrictedPathsManifestIdCacheBuilder {
    /// Create a new cache builder with default settings.
    pub fn new(ctx: CoreContext, manifest_id_store: ArcRestrictedPathsManifestIdStore) -> Self {
        Self {
            ctx,
            manifest_id_store,
            config: RestrictedPathsConfig::default().manifest_id_store_config,
        }
    }

    /// Set the complete manifest ID store cache configuration.
    pub fn with_manifest_id_store_cache(
        mut self,
        config: RestrictedPathsManifestIdStoreConfig,
    ) -> Self {
        self.config = config;
        self
    }

    /// Build and initialize the cache.
    pub async fn build(self) -> Result<RestrictedPathsManifestIdCache> {
        RestrictedPathsManifestIdCache::new(&self.ctx, &self.manifest_id_store, self.config).await
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

        // Build new cache structure from entries using fold
        let new_cache = entries.into_iter().try_fold(
            HashMap::<ManifestType, HashMap<RestrictedManifestId, HashSet<NonRootMPath>>>::new(),
            |mut acc,
             RestrictedPathManifestIdEntry {
                 manifest_type,
                 manifest_id,
                 path,
                 ..
             }| {
                let repo_path = RepoPath::dir(NonRootMPath::new(path.0)?)?;

                // Extract the NonRootMPath from the repo path
                if let mononoke_types::RepoPath::DirectoryPath(non_root) = repo_path {
                    acc.entry(manifest_type)
                        .or_default()
                        .entry(manifest_id)
                        .or_default()
                        .insert(non_root);
                }

                anyhow::Ok(acc)
            },
        )?;

        // Atomically update the cache
        let mut cache = self
            .cache
            .write()
            .map_err(|e| anyhow::anyhow!("Failed to acquire cache write lock: {e}"))?;
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

#[cfg(test)]
mod tests {
    use fbinit::FacebookInit;
    use mononoke_types::RepositoryId;
    use smallvec::SmallVec;

    use super::*;
    use crate::manifest_id_store::RestrictedPathsManifestIdStore;

    struct TestManifestIdStore {
        rows: RwLock<Vec<RestrictedPathManifestIdEntry>>,
        fail_reads: RwLock<bool>,
    }

    impl TestManifestIdStore {
        fn new(rows: Vec<RestrictedPathManifestIdEntry>) -> Self {
            Self {
                rows: RwLock::new(rows),
                fail_reads: RwLock::new(false),
            }
        }

        fn replace_rows(&self, rows: Vec<RestrictedPathManifestIdEntry>) -> Result<()> {
            *self
                .rows
                .write()
                .map_err(|error| anyhow::anyhow!("locking test rows for writing: {error}"))? = rows;
            Ok(())
        }

        fn fail_reads(&self) -> Result<()> {
            *self
                .fail_reads
                .write()
                .map_err(|error| anyhow::anyhow!("locking test failure flag: {error}"))? = true;
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl RestrictedPathsManifestIdStore for TestManifestIdStore {
        async fn add_entry(
            &self,
            _ctx: &CoreContext,
            _entry: RestrictedPathManifestIdEntry,
        ) -> Result<bool> {
            Ok(false)
        }

        async fn add_entries(
            &self,
            _ctx: &CoreContext,
            _entries: &[RestrictedPathManifestIdEntry],
        ) -> Result<bool> {
            Ok(false)
        }

        async fn get_paths_by_manifest_id(
            &self,
            _ctx: &CoreContext,
            _manifest_id: &RestrictedManifestId,
            _manifest_type: &ManifestType,
        ) -> Result<Vec<NonRootMPath>> {
            Ok(Vec::new())
        }

        async fn get_all_entries(
            &self,
            _ctx: &CoreContext,
        ) -> Result<Vec<RestrictedPathManifestIdEntry>> {
            if *self
                .fail_reads
                .read()
                .map_err(|error| anyhow::anyhow!("locking test failure flag: {error}"))?
            {
                anyhow::bail!("injected manifest store read failure");
            }
            Ok(self
                .rows
                .read()
                .map_err(|error| anyhow::anyhow!("locking test rows for reading: {error}"))?
                .clone())
        }

        async fn get_all_paths_by_manifest_id(
            &self,
            _ctx: &CoreContext,
            _manifest_id: &RestrictedManifestId,
        ) -> Result<Vec<(ManifestType, NonRootMPath)>> {
            Ok(Vec::new())
        }

        async fn delete_by_manifest_id(
            &self,
            _ctx: &CoreContext,
            _manifest_id: &RestrictedManifestId,
        ) -> Result<u64> {
            Ok(0)
        }

        fn repo_id(&self) -> RepositoryId {
            RepositoryId::new(1)
        }
    }

    /// What it tests: a full refresh populates all manifest types and suppresses duplicate rows.
    /// Expected: lookups retain both manifest types while identical paths occupy one cache entry.
    #[mononoke::fbinit_test]
    async fn test_full_refresh_populates_and_deduplicates(fb: FacebookInit) -> Result<()> {
        let fsnode = entry(ManifestType::Fsnode, 1, "shared")?;
        let store = Arc::new(TestManifestIdStore::new(vec![
            fsnode.clone(),
            fsnode,
            entry(ManifestType::Hg, 2, "hg")?,
        ]));
        let updater = updater(fb, store);

        updater.refresh_cache().await?;

        assert_eq!(cached_path_count(&updater.cache)?, 2);
        assert_eq!(cached_manifest_type_count(&updater.cache)?, 2);
        Ok(())
    }

    /// What it tests: each successful full refresh atomically replaces prior cache contents.
    /// Expected: deleted rows disappear, replacement rows appear, and an empty store clears the cache.
    #[mononoke::fbinit_test]
    async fn test_full_refresh_replaces_deletions_and_empty_store(fb: FacebookInit) -> Result<()> {
        let store = Arc::new(TestManifestIdStore::new(vec![entry(
            ManifestType::Fsnode,
            1,
            "deleted",
        )?]));
        let updater = updater(fb, store.clone());
        updater.refresh_cache().await?;

        store.replace_rows(vec![entry(ManifestType::HgAugmented, 2, "replacement")?])?;
        updater.refresh_cache().await?;
        assert_eq!(
            cached_paths(&updater.cache)?,
            HashSet::from([NonRootMPath::new("replacement")?])
        );

        store.replace_rows(Vec::new())?;
        updater.refresh_cache().await?;
        assert_eq!(cached_path_count(&updater.cache)?, 0);
        Ok(())
    }

    /// What it tests: a failed store read cannot publish a partial or empty cache replacement.
    /// Expected: refresh returns the injected error and the last successful cache remains readable.
    #[mononoke::fbinit_test]
    async fn test_full_refresh_failure_preserves_cache(fb: FacebookInit) -> Result<()> {
        let store = Arc::new(TestManifestIdStore::new(vec![entry(
            ManifestType::ContentManifest,
            1,
            "retained",
        )?]));
        let updater = updater(fb, store.clone());
        updater.refresh_cache().await?;
        store.replace_rows(Vec::new())?;
        store.fail_reads()?;

        assert!(updater.refresh_cache().await.is_err());
        assert_eq!(
            cached_paths(&updater.cache)?,
            HashSet::from([NonRootMPath::new("retained")?])
        );
        Ok(())
    }

    fn updater(fb: FacebookInit, store: Arc<TestManifestIdStore>) -> CacheUpdater {
        CacheUpdater {
            cache: Arc::new(RwLock::new(HashMap::new())),
            manifest_id_store: store,
            ctx: CoreContext::test_mock(fb),
        }
    }

    fn entry(
        manifest_type: ManifestType,
        manifest_byte: u8,
        path: &str,
    ) -> Result<RestrictedPathManifestIdEntry> {
        RestrictedPathManifestIdEntry::new(
            manifest_type,
            RestrictedManifestId::new(SmallVec::from_slice(&[manifest_byte; 32])),
            RepoPath::dir(NonRootMPath::new(path)?)?,
        )
    }

    fn cached_path_count(cache: &ManifestIdCache) -> Result<usize> {
        Ok(cached_paths(cache)?.len())
    }

    fn cached_manifest_type_count(cache: &ManifestIdCache) -> Result<usize> {
        Ok(cache
            .read()
            .map_err(|error| anyhow::anyhow!("locking cache for assertion: {error}"))?
            .len())
    }

    fn cached_paths(cache: &ManifestIdCache) -> Result<HashSet<NonRootMPath>> {
        Ok(cache
            .read()
            .map_err(|error| anyhow::anyhow!("locking cache for assertion: {error}"))?
            .values()
            .flat_map(HashMap::values)
            .flat_map(HashSet::iter)
            .cloned()
            .collect())
    }
}
