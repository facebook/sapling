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
use std::time::Instant;

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
        let mut updater = CacheUpdater {
            ctx: ctx.clone(),
            cache: cache.clone(),
            manifest_id_store: manifest_id_store.clone(),
            use_incremental_updates: config.use_incremental_cache_updates,
            incremental_lookback_ids: config.incremental_cache_update_lookback_ids,
            full_refresh_interval: Duration::from_millis(config.cache_full_refresh_interval_ms),
            max_seen_id: 0,
            last_full_refresh: Instant::now(),
        };

        tracing::debug!("Starting restricted paths cache updater");

        // Do initial refresh
        updater.full_refresh().await?;

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
    use_incremental_updates: bool,
    incremental_lookback_ids: u64,
    full_refresh_interval: Duration,
    max_seen_id: u64,
    last_full_refresh: Instant,
}

impl CacheUpdater {
    async fn refresh_cache(&mut self) -> Result<()> {
        if !self.use_incremental_updates
            || self.last_full_refresh.elapsed() >= self.full_refresh_interval
        {
            self.full_refresh().await
        } else {
            self.incremental_refresh().await
        }
    }

    /// Replace the cache from a full repository query.
    async fn full_refresh(&mut self) -> Result<()> {
        let entries = self.manifest_id_store.get_all_entries(&self.ctx).await?;
        let next_max_seen_id = if self.use_incremental_updates {
            entries.iter().try_fold(0, |max_seen_id, entry| {
                Ok::<_, anyhow::Error>(max_seen_id.max(entry.id.ok_or_else(|| {
                    anyhow::anyhow!("ID-aware manifest cache query returned an entry without an ID")
                })?))
            })?
        } else {
            0
        };

        let new_cache = entries_to_cache(entries)?;

        // Atomically update the cache
        let mut cache = self
            .cache
            .write()
            .map_err(|e| anyhow::anyhow!("Failed to acquire cache write lock: {e}"))?;
        *cache = new_cache;
        if self.use_incremental_updates {
            self.max_seen_id = next_max_seen_id;
        }
        self.last_full_refresh = Instant::now();

        Ok(())
    }

    /// Merge rows from an overlapping ID window into the cache.
    async fn incremental_refresh(&mut self) -> Result<()> {
        let min_id = self
            .max_seen_id
            .saturating_sub(self.incremental_lookback_ids);
        let entries = self
            .manifest_id_store
            .get_entries_by_id(&self.ctx, min_id)
            .await?;
        let next_max_seen_id =
            entries
                .iter()
                .try_fold(self.max_seen_id, |max_seen_id, entry| {
                    Ok::<_, anyhow::Error>(max_seen_id.max(entry.id.ok_or_else(|| {
                        anyhow::anyhow!(
                            "incremental manifest cache query returned an entry without an ID"
                        )
                    })?))
                })?;

        let delta = entries_to_cache(entries)?;

        let mut cache = self
            .cache
            .write()
            .map_err(|e| anyhow::anyhow!("Failed to acquire cache write lock: {e}"))?;
        delta.into_iter().for_each(|(manifest_type, manifests)| {
            let cached_manifests = cache.entry(manifest_type).or_default();
            manifests.into_iter().for_each(|(manifest_id, paths)| {
                cached_manifests
                    .entry(manifest_id)
                    .or_default()
                    .extend(paths);
            });
        });
        self.max_seen_id = next_max_seen_id;

        Ok(())
    }

    /// Spawn a background thread that periodically refreshes the cache.
    pub async fn spawn(mut self, terminate: oneshot::Receiver<()>, refresh_interval: Duration) {
        let loop_fut = async move {
            loop {
                // Construction performs the initial refresh, so wait before the next one.
                // Jitter prevents repository updater tasks from querying the database in lockstep.
                let sleep_interval = refresh_interval.mul_f64(0.9 + fastrand::f64() * 0.2);
                tokio::time::sleep(sleep_interval).await;

                // Refresh the cache
                let refresh_result = self.refresh_cache().await;

                if let Err(err) = refresh_result {
                    tracing::error!("Failed to refresh restricted paths cache: {:#}", err);
                }
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

fn entries_to_cache(
    entries: Vec<RestrictedPathManifestIdEntry>,
) -> Result<HashMap<ManifestType, HashMap<RestrictedManifestId, HashSet<NonRootMPath>>>> {
    entries.into_iter().try_fold(
        HashMap::<ManifestType, HashMap<RestrictedManifestId, HashSet<NonRootMPath>>>::new(),
        |mut cache,
         RestrictedPathManifestIdEntry {
             manifest_type,
             manifest_id,
             path,
             ..
         }| {
            let repo_path = RepoPath::dir(NonRootMPath::new(path.0)?)?;
            if let RepoPath::DirectoryPath(path) = repo_path {
                cache
                    .entry(manifest_type)
                    .or_default()
                    .entry(manifest_id)
                    .or_default()
                    .insert(path);
            }
            anyhow::Ok(cache)
        },
    )
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
        last_min_id: RwLock<Option<u64>>,
    }

    impl TestManifestIdStore {
        fn new(rows: Vec<RestrictedPathManifestIdEntry>) -> Self {
            Self {
                rows: RwLock::new(rows),
                fail_reads: RwLock::new(false),
                last_min_id: RwLock::new(None),
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

        fn last_min_id(&self) -> Result<Option<u64>> {
            Ok(*self
                .last_min_id
                .read()
                .map_err(|error| anyhow::anyhow!("locking last test lower bound: {error}"))?)
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

        async fn get_entries_by_id(
            &self,
            ctx: &CoreContext,
            min_id: u64,
        ) -> Result<Vec<RestrictedPathManifestIdEntry>> {
            *self.last_min_id.write().map_err(|error| {
                anyhow::anyhow!("locking last test lower bound for writing: {error}")
            })? = Some(min_id);
            Ok(self
                .get_all_entries(ctx)
                .await?
                .into_iter()
                .filter(|entry| entry.id.is_none_or(|id| id >= min_id))
                .collect())
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
        let mut updater = updater(fb, store);

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
        let mut updater = updater(fb, store.clone());
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
        let mut updater = updater(fb, store.clone());
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

    /// What it tests: overlapping incremental reads merge delayed rows idempotently.
    /// Expected: a delayed lower-ID row is added, the watermark advances, and repeated overlap rows do not duplicate.
    #[mononoke::fbinit_test]
    async fn test_incremental_refresh_merges_overlap_without_duplicates(
        fb: FacebookInit,
    ) -> Result<()> {
        let first = entry_with_id(100, 1, "first")?;
        let store = Arc::new(TestManifestIdStore::new(vec![first.clone()]));
        let mut updater = incremental_updater(fb, store.clone(), 10);
        updater.full_refresh().await?;

        store.replace_rows(vec![
            entry_with_id(95, 2, "delayed")?,
            first,
            entry_with_id(101, 3, "latest")?,
        ])?;
        updater.incremental_refresh().await?;
        updater.incremental_refresh().await?;

        assert_eq!(updater.max_seen_id, 101);
        assert_eq!(store.last_min_id()?, Some(91));
        assert_eq!(cached_path_count(&updater.cache)?, 3);
        Ok(())
    }

    /// What it tests: the incremental lower bound saturates and the watermark never regresses.
    /// Expected: a large lookback queries from zero and older rows leave the watermark unchanged.
    #[mononoke::fbinit_test]
    async fn test_incremental_refresh_saturates_and_preserves_watermark(
        fb: FacebookInit,
    ) -> Result<()> {
        let store = Arc::new(TestManifestIdStore::new(vec![entry_with_id(
            5, 1, "first",
        )?]));
        let mut updater = incremental_updater(fb, store.clone(), 10);
        updater.full_refresh().await?;

        store.replace_rows(vec![entry_with_id(3, 2, "older")?])?;
        updater.incremental_refresh().await?;

        assert_eq!(store.last_min_id()?, Some(0));
        assert_eq!(updater.max_seen_id, 5);
        Ok(())
    }

    /// What it tests: incremental refreshes cannot infer deletions or recover rows beyond the overlap.
    /// Expected: periodic full reconciliation removes deleted rows and recovers an older delayed row.
    #[mononoke::fbinit_test]
    async fn test_full_refresh_reconciles_incremental_gaps_and_deletions(
        fb: FacebookInit,
    ) -> Result<()> {
        let deleted = entry_with_id(100, 1, "deleted")?;
        let retained = entry_with_id(200, 2, "retained")?;
        let store = Arc::new(TestManifestIdStore::new(vec![deleted, retained.clone()]));
        let mut updater = incremental_updater(fb, store.clone(), 10);
        updater.full_refresh().await?;

        store.replace_rows(vec![entry_with_id(50, 3, "delayed")?, retained])?;
        updater.incremental_refresh().await?;
        assert_eq!(cached_path_count(&updater.cache)?, 2);

        updater.last_full_refresh = Instant::now() - Duration::from_secs(61);
        updater.refresh_cache().await?;
        assert_eq!(
            cached_paths(&updater.cache)?,
            HashSet::from([
                NonRootMPath::new("delayed")?,
                NonRootMPath::new("retained")?,
            ])
        );
        Ok(())
    }

    /// What it tests: ID-aware refreshes reject rows that do not contain a database ID.
    /// Expected: the refresh fails without changing the last good cache or watermark.
    #[mononoke::fbinit_test]
    async fn test_incremental_refresh_rejects_missing_id(fb: FacebookInit) -> Result<()> {
        let first = entry_with_id(10, 1, "retained")?;
        let store = Arc::new(TestManifestIdStore::new(vec![first]));
        let mut updater = incremental_updater(fb, store.clone(), 5);
        updater.full_refresh().await?;
        store.replace_rows(vec![entry(ManifestType::Fsnode, 2, "missing-id")?])?;

        assert!(updater.incremental_refresh().await.is_err());
        assert_eq!(updater.max_seen_id, 10);
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
            use_incremental_updates: false,
            incremental_lookback_ids: 0,
            full_refresh_interval: Duration::from_secs(60),
            max_seen_id: 0,
            last_full_refresh: Instant::now(),
        }
    }

    fn incremental_updater(
        fb: FacebookInit,
        store: Arc<TestManifestIdStore>,
        lookback_ids: u64,
    ) -> CacheUpdater {
        CacheUpdater {
            use_incremental_updates: true,
            incremental_lookback_ids: lookback_ids,
            ..updater(fb, store)
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

    fn entry_with_id(
        id: u64,
        manifest_byte: u8,
        path: &str,
    ) -> Result<RestrictedPathManifestIdEntry> {
        Ok(RestrictedPathManifestIdEntry {
            id: Some(id),
            ..entry(ManifestType::Fsnode, manifest_byte, path)?
        })
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
