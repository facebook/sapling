/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::RwLock;
use std::time::SystemTime;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use cached_config::ConfigUpdateWatcher;
use cloned::cloned;
use futures::future::join_all;
use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use metaconfig_parser::config::configerator_config_handle;
use metaconfig_parser::config::load_configs_from_raw;
use metaconfig_parser::configerator_manifest_handle;
use metaconfig_parser::configerator_repo_spec_handle;
use metaconfig_parser::parse_repo_spec;
use metaconfig_types::CommonConfig;
use metaconfig_types::ConfigInfo;
use metaconfig_types::RepoConfig;
use repos::RawRepoConfigs;
use repos::RepoSpec;
use repos::TierManifest;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use stats::prelude::*;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

const LIVENESS_INTERVAL: u64 = 300;
const CONFIGERATOR_TIER_PREFIX: &str = "configerator://scm/mononoke/repos/tiers/";
type Swappable<T> = Arc<ArcSwap<T>>;

define_stats! {
    prefix = "mononoke.config_refresh";
    refresh_failure_count: timeseries(Average, Sum, Count),
    refresh_success_count: timeseries(Average, Sum, Count),
    liveness_count: timeseries(Average, Sum, Count),
    spurious_reload_suppressed: timeseries(Average, Sum, Count),
    merge_skipped_no_handle: timeseries(Average, Sum, Count),
}

fn content_changed<T: PartialEq>(prev: &Option<Arc<T>>, current: &Option<Arc<T>>) -> bool {
    match (prev, current) {
        (Some(a), Some(b)) => **a != **b,
        (None, None) => false,
        _ => true,
    }
}

async fn wait_for_handle<T: Send + Sync + 'static>(
    watcher: &mut Option<ConfigUpdateWatcher<T>>,
) -> Result<()> {
    match watcher {
        Some(w) => {
            w.wait_for_next().await?;
            Ok(())
        }
        None => std::future::pending().await,
    }
}

/// Configuration provider and update notifier for all of Mononoke services
/// and jobs. The configurations provided by this struct are always up-to-date
/// with its source.
pub struct MononokeConfigs {
    /// Serializes write operations (clone-insert-store) on repo_configs ArcSwap.
    /// Must NOT be held across async operations.
    config_update_lock: Arc<Mutex<()>>,
    repo_configs: Swappable<RepoConfigs>,
    storage_configs: Swappable<StorageConfigs>,
    update_receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>>,
    config_info: Swappable<Option<ConfigInfo>>,
    maybe_config_updater: Option<JoinHandle<()>>,
    maybe_liveness_updater: Option<JoinHandle<()>>,
    maybe_config_handle: Option<ConfigHandle<RawRepoConfigs>>,
    // Per-repo split-loading fields
    maybe_manifest_handle: Option<ConfigHandle<TierManifest>>,
    repo_handles: Arc<RwLock<HashMap<String, ConfigHandle<RepoSpec>>>>,
    config_store: Option<ConfigStore>,
    /// Tier name derived from the configerator config path.
    /// Used for resolving tier_overrides in RepoSpec configs during split-loading.
    tier_name: Option<String>,
}

impl MononokeConfigs {
    /// Create a new instance of MononokeConfigs with configurations backed via ConfigStore.
    /// If the config path points to a dynamic config source (e.g. configerator), this enables
    /// auto-refresh of those configurations.
    pub fn new(
        config_path: impl AsRef<Path>,
        config_store: &ConfigStore,
        manifest_path: Option<&str>,
        runtime_handle: Handle,
    ) -> Result<Self> {
        let storage_configs = metaconfig_parser::load_storage_configs(&config_path, config_store)?;
        let storage_configs = Arc::new(ArcSwap::from_pointee(storage_configs));
        let repo_configs = metaconfig_parser::load_repo_configs(&config_path, config_store)?;
        let repo_configs = Arc::new(ArcSwap::from_pointee(repo_configs));

        // Derive tier name from the configerator config path.
        // Configerator paths follow the pattern:
        //   configerator://scm/mononoke/repos/tiers/{tier_name}
        let tier_name = config_path
            .as_ref()
            .to_str()
            .and_then(|p| p.strip_prefix(CONFIGERATOR_TIER_PREFIX))
            .filter(|t| !t.is_empty())
            .map(|t| t.to_owned());
        let update_receivers = Arc::new(ArcSwap::from_pointee(vec![]));
        let maybe_config_handle = configerator_config_handle(config_path.as_ref(), config_store)?;
        let config_info = if let Some(config_handle) = maybe_config_handle.as_ref() {
            match build_config_info(config_handle.get()) {
                Ok(new_config_info) => Some(new_config_info),
                Err(e) => {
                    warn!("Could not compute new config_info: {:?}", e);
                    None
                }
            }
        } else {
            None
        };
        let config_info = Arc::new(ArcSwap::from_pointee(config_info));
        let maybe_manifest_handle = manifest_path
            .map(|path| configerator_manifest_handle(path, config_store))
            .transpose()?;

        // Only log split-loading status for configerator-backed configs (where
        // tier_name is set). File-backed configs used in tests always have
        // tier_name=None and manifest_path=None, so logging would be noise.
        if tier_name.is_some() {
            if let Some(manifest_path) = manifest_path {
                debug!(
                    "Split-loading enabled: config_path={}, manifest_path={}, tier_name={:?}",
                    config_path.as_ref().to_string_lossy(),
                    manifest_path,
                    tier_name.as_deref().unwrap_or("<none>"),
                );
            } else {
                debug!(
                    "Split-loading disabled: config_path={}, tier_name={:?}",
                    config_path.as_ref().to_string_lossy(),
                    tier_name.as_deref().unwrap_or("<none>"),
                );
            }
        }

        // Validate: split-loading (manifest) requires a tier name for resolving
        // tier_overrides in RepoSpec configs.
        if maybe_manifest_handle.is_some() && tier_name.is_none() {
            anyhow::bail!(
                "tier_name is required when split-loading is enabled (manifest_path is set)"
            );
        }

        let repo_handles = Arc::new(RwLock::new(HashMap::new()));

        // If manifest is available, pre-load handles for non-deep-sharded repos.
        // Collect all handles first, then insert in bulk under a single write lock.
        if let Some(ref manifest_handle) = maybe_manifest_handle {
            let manifest = manifest_handle.get();
            let handles_to_add: Vec<_> = manifest
                .repos
                .iter()
                .filter(|e| !e.is_deep_sharded)
                .map(|entry| {
                    let handle = configerator_repo_spec_handle(&entry.config_path, config_store)?;
                    Ok((entry.repo_name.clone(), handle))
                })
                .collect::<Result<Vec<_>>>()?;

            info!(
                "Split-loading: pre-loaded {} repo handles from manifest ({} total repos, {} deep-sharded skipped)",
                handles_to_add.len(),
                manifest.repos.len(),
                manifest.repos.iter().filter(|e| e.is_deep_sharded).count(),
            );

            repo_handles
                .write()
                .map_err(|e| anyhow!("repo_handles lock poisoned: {}", e))?
                .extend(handles_to_add);
        }

        let maybe_liveness_updater =
            if maybe_config_handle.is_some() || maybe_manifest_handle.is_some() {
                Some(runtime_handle.spawn(liveness_updater()))
            } else {
                None
            };

        let maybe_config_updater =
            if maybe_config_handle.is_some() || maybe_manifest_handle.is_some() {
                cloned!(
                    repo_handles,
                    repo_configs,
                    storage_configs,
                    config_info,
                    update_receivers,
                );
                let config_store_clone = config_store.clone();
                let tier = tier_name.clone();
                Some(runtime_handle.spawn(unified_config_watcher(
                    maybe_config_handle.clone(),
                    maybe_manifest_handle.clone(),
                    repo_handles,
                    config_store_clone,
                    tier,
                    repo_configs,
                    storage_configs,
                    config_info,
                    update_receivers,
                )))
            } else {
                None
            };

        Ok(Self {
            config_update_lock: Arc::new(Mutex::new(())),
            repo_configs,
            storage_configs,
            update_receivers,
            config_info,
            maybe_config_updater,
            maybe_config_handle,
            maybe_liveness_updater,
            maybe_manifest_handle,
            repo_handles,
            config_store: Some(config_store.clone()),
            tier_name,
        })
    }

    /// The latest repo configs fetched from the underlying configuration store.
    pub fn repo_configs(&self) -> Arc<RepoConfigs> {
        // Load full since there could be lots of calls to repo_configs.
        self.repo_configs.load_full()
    }

    /// The latest storage configs fetched from the underlying configuration store.
    pub fn storage_configs(&self) -> Arc<StorageConfigs> {
        // Load full since there could be lots of calls to storage_configs.
        self.storage_configs.load_full()
    }

    /// The info on the latest config fetched from the underlying configuration store.
    pub fn config_info(&self) -> Arc<Option<ConfigInfo>> {
        self.config_info.load_full()
    }

    /// Returns the ConfigStore, if available (configerator-backed configs only).
    pub fn config_store(&self) -> Option<&ConfigStore> {
        self.config_store.as_ref()
    }

    /// Returns the current TierManifest, if split-loading is enabled.
    pub fn manifest(&self) -> Option<Arc<TierManifest>> {
        self.maybe_manifest_handle.as_ref().map(|h| h.get())
    }

    /// Is automatic update of the underlying configuration enabled?
    pub fn auto_update_enabled(&self) -> bool {
        // If the config updater handle is none, configs won't be updated.
        self.maybe_config_updater.is_some()
    }

    // Config watcher that can be used to get notified of the latest
    // changes in the underlying config and to act on it. This is useful
    // if the processing to be performed is long running which is not supported
    // via ConfigUpdateReceivers
    pub fn config_watcher(&self) -> Option<ConfigUpdateWatcher<RawRepoConfigs>> {
        self.maybe_config_handle
            .as_ref()
            .and_then(|config_handle| config_handle.watcher().ok())
    }

    /// Register an instance of ConfigUpdateReceiver to receive notifications of updates to
    /// the underlying configs which can then be used to perform further actions. Note that
    /// the operation performed by the ConfigUpdateReceiver should not be too long running.
    /// If that's the case, use config_watcher method instead.
    pub fn register_for_update(&self, update_receiver: Arc<dyn ConfigUpdateReceiver>) {
        let mut update_receivers = Vec::from_iter(self.update_receivers.load().iter().cloned());
        update_receivers.push(update_receiver);
        self.update_receivers.store(Arc::new(update_receivers));
    }

    /// Drop the per-repo ConfigHandle (called by ShardManager on_drop_shard via
    /// repos_manager::remove_repo). Symmetric counterpart to load_repo_config_handle.
    /// No-op if no handle is held.
    pub fn remove_repo_config_handle(&self, repo_name: &str) {
        match self.repo_handles.write() {
            Ok(mut handles) => {
                if handles.remove(repo_name).is_some() {
                    info!("Removed config handle for repo: {}", repo_name);
                }
            }
            Err(e) => {
                error!(
                    "repo_handles lock poisoned while removing {}: {}",
                    repo_name, e
                );
            }
        }
    }

    /// Create a per-repo ConfigHandle on-demand (called by ShardManager on_add_shard).
    pub fn load_repo_config_handle(&self, repo_name: &str) -> Result<()> {
        // Fast path: already loaded
        if self
            .repo_handles
            .read()
            .map_err(|e| anyhow!("repo_handles lock poisoned: {}", e))?
            .contains_key(repo_name)
        {
            return Ok(());
        }

        let manifest = self
            .maybe_manifest_handle
            .as_ref()
            .context("No manifest handle available")?
            .get();

        let entry = manifest
            .repos
            .iter()
            .find(|e| e.repo_name == repo_name)
            .ok_or_else(|| anyhow!("Repo {} not found in manifest", repo_name))?;

        let config_store = self
            .config_store
            .as_ref()
            .context("No config store available")?;

        let handle = configerator_repo_spec_handle(&entry.config_path, config_store)?;
        self.repo_handles
            .write()
            .map_err(|e| anyhow!("repo_handles lock poisoned: {}", e))?
            .insert(repo_name.to_owned(), handle);
        Ok(())
    }

    /// Load a repo config on-demand. Checks the repo_configs cache first,
    /// falls back to loading from the TierManifest via ConfigHandle.
    ///
    /// Thread-safe: uses config_update_lock for write serialization.
    /// The lock only guards the synchronous clone-insert-store; expensive
    /// work (ConfigHandle subscription, RepoSpec parsing) happens outside it.
    pub fn get_or_load_repo_config(&self, repo_name: &str) -> Result<RepoConfig> {
        // Fast path: lock-free read from cache (covers both legacy blob
        // and previously loaded split-config repos)
        if let Some(config) = self.repo_configs.load_full().repos.get(repo_name) {
            return Ok(config.clone());
        }

        // Slow path: try loading from manifest. If split-loading infrastructure
        // is unavailable (no manifest, no config store), this will fail and
        // we return the error — the fast path above already checked the legacy blob.
        let repo_config = self.load_and_parse_repo_config(repo_name)?;

        // Insert into cache (INSIDE the lock, sync only)
        {
            let _guard = self
                .config_update_lock
                .lock()
                .map_err(|e| anyhow!("config_update_lock poisoned: {}", e))?;

            // Double-check: another thread may have loaded it while we were parsing
            let current = self.repo_configs.load_full();
            if let Some(config) = current.repos.get(repo_name) {
                return Ok(config.clone());
            }

            let mut new_configs = (*current).clone();
            new_configs.insert_repo(repo_name.to_owned(), repo_config.clone());
            self.repo_configs.store(Arc::new(new_configs));
        }

        Ok(repo_config)
    }

    /// Subscribe to a repo's ConfigHandle and parse its RepoSpec into a RepoConfig.
    /// This is the shared helper for get_or_load_repo_config and batch_load_repo_configs.
    fn load_and_parse_repo_config(&self, repo_name: &str) -> Result<RepoConfig> {
        self.load_repo_config_handle(repo_name)?;
        let handle = self
            .repo_handles
            .read()
            .map_err(|e| anyhow!("repo_handles lock poisoned: {}", e))?
            .get(repo_name)
            .context("handle not found after load")?
            .clone();
        let repo_spec = handle.get();
        let tier = self
            .tier_name
            .as_deref()
            .context("tier_name required for split-loading")?;
        let manifest = self
            .maybe_manifest_handle
            .as_ref()
            .context("manifest handle required for split-loading")?
            .get();
        parse_repo_spec(Arc::unwrap_or_clone(repo_spec), tier, &manifest.storage)
    }

    /// Batch-load repo configs. Single lock acquisition, single HashMap clone,
    /// single ArcSwap store regardless of how many repos are loaded.
    /// This is the default path for startup (`open_managed_repos`).
    pub fn batch_load_repo_configs(
        &self,
        repo_names: &[String],
    ) -> Result<Vec<(String, RepoConfig)>> {
        // Step 1: Separate cached from missing (no lock)
        let current = self.repo_configs.load_full();
        let mut results: Vec<(String, RepoConfig)> = Vec::new();
        let mut missing: Vec<String> = Vec::new();

        for name in repo_names {
            if let Some(config) = current.repos.get(name.as_str()) {
                results.push((name.clone(), config.clone()));
            } else {
                missing.push(name.clone());
            }
        }

        if missing.is_empty() {
            return Ok(results);
        }

        // Step 2: Subscribe to ConfigHandles + parse OUTSIDE the lock
        let mut loaded: Vec<(String, RepoConfig)> = Vec::new();
        for name in &missing {
            match self.load_and_parse_repo_config(name) {
                Ok(config) => loaded.push((name.clone(), config)),
                Err(e) => {
                    warn!("batch_load: failed to load config for {}: {:#}", name, e);
                }
            }
        }

        // Step 3: Single lock, single clone, bulk insert, single store
        if !loaded.is_empty() {
            let _guard = self
                .config_update_lock
                .lock()
                .map_err(|e| anyhow!("config_update_lock poisoned: {}", e))?;
            let mut new = (*self.repo_configs.load_full()).clone();
            for (name, config) in &loaded {
                new.insert_repo(name.clone(), config.clone());
            }
            self.repo_configs.store(Arc::new(new));
        }

        results.extend(loaded);
        Ok(results)
    }

    /// Load configs for all repos discovered from both the legacy blob and
    /// the manifest. Uses batch loading (single lock, single clone).
    pub fn load_all_repo_configs(&self) -> Result<Vec<(String, RepoConfig)>> {
        let mut all_names: HashSet<String> = self
            .repo_configs
            .load_full()
            .repos
            .keys()
            .cloned()
            .collect();
        if let Some(manifest) = self.manifest() {
            for entry in &manifest.repos {
                all_names.insert(entry.repo_name.clone());
            }
        }
        let names: Vec<String> = all_names.into_iter().collect();
        self.batch_load_repo_configs(&names)
    }

    /// Load a repo config by repository ID. O(1) cache lookup via repos_by_id
    /// index, falls back to searching the manifest by repo_id.
    pub fn get_or_load_repo_config_by_id(&self, repo_id: i32) -> Result<(String, RepoConfig)> {
        // Fast path: O(1) lookup via repos_by_id index
        let current = self.repo_configs.load_full();
        if let Some((name, config)) = current.get_repo_config_by_raw_id(repo_id) {
            return Ok((name.clone(), config.clone()));
        }

        // Slow path: search manifest for repo_id.
        // If no manifest is available (e.g. integration tests, file-backed configs),
        // fall back to the original "unknown repoid" error.
        let manifest = match self.maybe_manifest_handle.as_ref() {
            Some(handle) => handle.get(),
            None => anyhow::bail!("unknown repoid: RepositoryId({})", repo_id),
        };
        let entry = manifest
            .repos
            .iter()
            .find(|e| e.repo_id == repo_id)
            .ok_or_else(|| anyhow!("unknown repoid: RepositoryId({})", repo_id))?;
        let config = self.get_or_load_repo_config(&entry.repo_name)?;
        Ok((entry.repo_name.clone(), config))
    }
}

impl Drop for MononokeConfigs {
    // If MononokeConfigs is getting dropped, then we need to terminate the updater
    // process as well.
    fn drop(&mut self) {
        // If the updater process exists, abort it.
        if let Some(updater_handle) = self.maybe_config_updater.as_ref() {
            updater_handle.abort();
        }
        // If the liveness updater process exists, abort it.
        if let Some(liveness_updater) = self.maybe_liveness_updater.as_ref() {
            liveness_updater.abort();
        }
    }
}

async fn liveness_updater() {
    loop {
        STATS::liveness_count.add_value(1);
        tokio::time::sleep(tokio::time::Duration::from_secs(LIVENESS_INTERVAL)).await;
    }
}

/// Unified config watcher: monitors both the legacy blob ConfigHandle and the
/// TierManifest ConfigHandle via tokio::select!, applying changes exactly once.
///
/// Fixes two production bugs:
/// 1. Spurious reloads: PartialEq on thrift structs skips no-op version bumps.
/// 2. Double reloads: single apply_update call per real change (was two).
async fn unified_config_watcher(
    blob_handle: Option<ConfigHandle<RawRepoConfigs>>,
    manifest_handle: Option<ConfigHandle<TierManifest>>,
    repo_handles: Arc<RwLock<HashMap<String, ConfigHandle<RepoSpec>>>>,
    config_store: ConfigStore,
    tier_name: Option<String>,
    repo_configs: Swappable<RepoConfigs>,
    storage_configs: Swappable<StorageConfigs>,
    config_info: Swappable<Option<ConfigInfo>>,
    update_receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>>,
) {
    let mut blob_watcher = blob_handle
        .as_ref()
        .map(|h| h.watcher())
        .transpose()
        .unwrap_or_else(|e| {
            error!("Failed to create blob config watcher: {:?}", e);
            None
        });
    let mut manifest_watcher = manifest_handle
        .as_ref()
        .map(|h| h.watcher())
        .transpose()
        .unwrap_or_else(|e| {
            error!("Failed to create manifest watcher: {:?}", e);
            None
        });

    if blob_watcher.is_none() && manifest_watcher.is_none() {
        warn!("No config watchers available, unified_config_watcher exiting");
        return;
    }

    let mut prev_blob: Option<Arc<RawRepoConfigs>> = None;
    let mut prev_manifest: Option<Arc<TierManifest>> = None;
    let mut cached_parsed: Option<RepoConfigs> = None;

    loop {
        tokio::select! {
            result = wait_for_handle(&mut blob_watcher) => {
                if let Err(e) = result {
                    error!("Error waiting for blob config update: {:?}", e);
                    continue;
                }
            }
            result = wait_for_handle(&mut manifest_watcher) => {
                if let Err(e) = result {
                    error!("Error waiting for manifest config update: {:?}", e);
                    continue;
                }
            }
        }

        let current_blob = blob_handle.as_ref().map(|h| h.get());
        let current_manifest = manifest_handle.as_ref().map(|h| h.get());

        let blob_changed = content_changed(&prev_blob, &current_blob);
        let manifest_changed = content_changed(&prev_manifest, &current_manifest);

        if !blob_changed && !manifest_changed {
            STATS::spurious_reload_suppressed.add_value(1);
            debug!("Config version bumped but content identical, skipping reload");
            continue;
        }

        info!(
            "Config content changed (blob={}, manifest={}), applying update",
            blob_changed, manifest_changed,
        );

        if blob_changed {
            if let Some(ref raw) = current_blob {
                match load_configs_from_raw(Arc::unwrap_or_clone(raw.clone())) {
                    Ok((configs, new_storage)) => {
                        storage_configs.store(Arc::new(new_storage));
                        match build_config_info(raw.clone()) {
                            Ok(info) => config_info.store(Arc::new(Some(info))),
                            Err(e) => warn!("Could not compute new config_info: {:?}", e),
                        }
                        cached_parsed = Some(configs);
                    }
                    Err(e) => {
                        error!("Failed to parse blob config: {:?}", e);
                        STATS::refresh_failure_count.add_value(1);
                        continue;
                    }
                }
            } else {
                cached_parsed = None;
            }
            prev_blob = current_blob;
        }

        if manifest_changed {
            if let Some(ref manifest) = current_manifest {
                if let Err(e) = sync_repo_handles(manifest, &repo_handles, &config_store) {
                    // Don't update prev_manifest so we retry on the next watcher cycle.
                    // Transient failures (e.g., configerator timeout for a new repo handle)
                    // will self-heal on the next notification.
                    error!("Failed to sync repo handles: {:?}", e);
                    STATS::refresh_failure_count.add_value(1);
                    continue;
                }
            }
            prev_manifest = current_manifest;
        }

        let base = cached_parsed
            .clone()
            .unwrap_or_else(|| RepoConfigs::new(HashMap::new(), CommonConfig::default()));

        let merged = match (&prev_manifest, tier_name.as_deref()) {
            (Some(manifest), Some(tier)) => {
                let handles = match repo_handles.read() {
                    Ok(h) => h,
                    Err(e) => {
                        error!("Failed to read repo handles lock: {:?}", e);
                        STATS::refresh_failure_count.add_value(1);
                        continue;
                    }
                };
                let mut repos = base.repos.clone();
                for entry in &manifest.repos {
                    if let Some(handle) = handles.get(&entry.repo_name) {
                        let spec = handle.get();
                        match parse_repo_spec(Arc::unwrap_or_clone(spec), tier, &manifest.storage) {
                            Ok(config) => {
                                repos.insert(entry.repo_name.clone(), config);
                            }
                            Err(e) => {
                                error!(
                                    "Failed to parse RepoSpec for repo '{}', skipping: {:?}",
                                    entry.repo_name, e,
                                );
                            }
                        }
                    } else {
                        STATS::merge_skipped_no_handle.add_value(1);
                    }
                }
                RepoConfigs::new(repos, base.common.clone())
            }
            _ => base,
        };

        let new_configs = Arc::new(merged);
        repo_configs.store(new_configs.clone());
        let current_storage = storage_configs.load_full();
        let receivers = update_receivers.load();
        let results = join_all(
            receivers
                .iter()
                .map(|r| r.apply_update(new_configs.clone(), current_storage.clone())),
        )
        .await;
        let had_failure = results.iter().any(|r| r.is_err());
        for (i, result) in results.iter().enumerate() {
            if let Err(e) = result {
                error!("Config update receiver {} failed: {:?}", i, e);
            }
        }
        if had_failure {
            STATS::refresh_failure_count.add_value(1);
        } else {
            info!("Successfully applied config update");
            STATS::refresh_success_count.add_value(1);
            // Keep the timeseries alive for OneDetection alerting
            STATS::refresh_failure_count.add_value(0);
        }
    }
}

/// Adds preload handles for new non-deep-sharded entries; removes handles
/// whose repo is gone from the manifest. The add/remove filters are
/// asymmetric on purpose: deep-sharded repos are loaded on-demand by
/// ShardManager via `load_repo_config_handle` and must survive manifest
/// refreshes.
fn sync_repo_handles(
    manifest: &TierManifest,
    repo_handles: &RwLock<HashMap<String, ConfigHandle<RepoSpec>>>,
    config_store: &ConfigStore,
) -> Result<()> {
    let current_repos: HashSet<String> = repo_handles
        .read()
        .map_err(|e| anyhow!("repo_handles lock poisoned: {}", e))?
        .keys()
        .cloned()
        .collect();

    let to_remove = compute_handles_to_remove(&current_repos, manifest);

    let new_handles: Vec<_> = manifest
        .repos
        .iter()
        .filter(|entry| !entry.is_deep_sharded && !current_repos.contains(&entry.repo_name))
        .filter_map(
            |entry| match configerator_repo_spec_handle(&entry.config_path, config_store) {
                Ok(handle) => {
                    info!("Added config handle for new repo: {}", entry.repo_name);
                    Some((entry.repo_name.clone(), handle))
                }
                Err(e) => {
                    error!("Failed to load config for {}: {:?}", entry.repo_name, e);
                    STATS::refresh_failure_count.add_value(1);
                    None
                }
            },
        )
        .collect();

    if !new_handles.is_empty() || !to_remove.is_empty() {
        let mut handles = repo_handles
            .write()
            .map_err(|e| anyhow!("repo_handles lock poisoned: {}", e))?;
        handles.extend(new_handles);
        for repo_name in &to_remove {
            handles.remove(repo_name);
            info!("Removed config handle for repo: {}", repo_name);
        }
    }

    Ok(())
}

/// Names in `current_repos` no longer present in the manifest. Pure helper
/// extracted to make the diff testable without a ConfigStore.
fn compute_handles_to_remove(
    current_repos: &HashSet<String>,
    manifest: &TierManifest,
) -> Vec<String> {
    let manifest_repo_names: HashSet<&str> = manifest
        .repos
        .iter()
        .map(|e| e.repo_name.as_str())
        .collect();
    current_repos
        .iter()
        .filter(|name| !manifest_repo_names.contains(name.as_str()))
        .cloned()
        .collect()
}

/// Trait defining methods related to config update notification. A struct implementing
/// this trait can be configured to receive the most updated config value every time the
/// underlying config changes.
#[async_trait]
pub trait ConfigUpdateReceiver: Send + Sync {
    /// Method containing the logic to be executed when the configuration is updated. This
    /// should not be too long running since the config updates will wait for all update
    /// receivers before checking for the next config update.
    async fn apply_update(
        &self,
        repo_configs: Arc<RepoConfigs>,
        storage_configs: Arc<StorageConfigs>,
    ) -> Result<()>;

    /// Called when a single repo's config changes in the per-repo split-loading path.
    /// Default implementation is a no-op — existing receivers that only handle bulk
    /// updates don't need to implement this.
    async fn apply_repo_update(&self, _repo_name: &str, _repo_config: &RepoConfig) -> Result<()> {
        Ok(())
    }
}

fn serialize_to_value<T: Serialize, S: serde::Serializer>(
    value: &T,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let value = serde_json::to_value(value).map_err(serde::ser::Error::custom)?;
    value.serialize(serializer)
}

#[derive(Serialize)]
struct SortKeys<T: Serialize>(#[serde(serialize_with = "serialize_to_value")] T);

fn build_config_info(raw_repo_configs: Arc<RawRepoConfigs>) -> Result<ConfigInfo> {
    let content_hash = {
        let serialized = serde_json::to_string(&SortKeys(raw_repo_configs))
            .expect("RawRepoConfigs serialization should never fail");
        let mut hasher = Sha256::new();
        hasher.update(serialized);
        let hash = hasher.finalize();
        hex::encode(hash)
    };

    let last_updated_at = {
        let now = SystemTime::now();
        now.duration_since(SystemTime::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs()
    };

    Ok(ConfigInfo {
        content_hash,
        last_updated_at,
    })
}

#[cfg(test)]
mod test {
    use std::collections::HashSet;
    use std::sync::Arc;

    use mononoke_macros::mononoke;
    use repos::RawRepoConfig;
    use repos::RawRepoConfigs;
    use repos::TierManifest;
    use repos::TierRepoEntry;

    use super::*;

    #[mononoke::test]
    fn test_build_config_info_empty() {
        let results = (1..10)
            .map(|_i| {
                let raw_repo_configs = RawRepoConfigs::default();
                let res = build_config_info(Arc::new(raw_repo_configs));
                assert!(res.is_ok());

                let info = res.unwrap().to_owned();
                assert!(info.last_updated_at > 0);

                info.content_hash
            })
            .fold(HashSet::new(), |mut h, i| {
                h.insert(i);
                h
            });
        assert_eq!(results.len(), 1);
    }

    #[mononoke::test]
    fn test_build_config_info_one_repo() {
        let results = (1..10)
            .map(|_| {
                let mut raw_repo_configs = RawRepoConfigs::default();
                raw_repo_configs
                    .repos
                    .insert("repo1".to_string(), RawRepoConfig::default());

                let res = build_config_info(Arc::new(raw_repo_configs));
                assert!(res.is_ok());

                let info = res.unwrap().to_owned();
                assert!(info.last_updated_at > 0);

                info.content_hash
            })
            .fold(HashSet::new(), |mut h, i| {
                h.insert(i);
                h
            });
        assert_eq!(results.len(), 1);
    }

    #[mononoke::test]
    fn test_build_config_info_two_repos() {
        let results = (1..10)
            .flat_map(|_| {
                let mut ret = Vec::new();

                let mut raw_repo_configs = RawRepoConfigs::default();
                raw_repo_configs
                    .repos
                    .insert("repo1".to_string(), RawRepoConfig::default());
                raw_repo_configs
                    .repos
                    .insert("repo2".to_string(), RawRepoConfig::default());

                let res = build_config_info(Arc::new(raw_repo_configs));
                assert!(res.is_ok());

                let info = res.unwrap().to_owned();
                assert!(info.last_updated_at > 0);
                ret.push(info.content_hash);

                // Test that the hash is different if the order of the repos is different
                let mut raw_repo_configs = RawRepoConfigs::default();
                raw_repo_configs
                    .repos
                    .insert("repo2".to_string(), RawRepoConfig::default());
                raw_repo_configs
                    .repos
                    .insert("repo1".to_string(), RawRepoConfig::default());

                let res = build_config_info(Arc::new(raw_repo_configs));
                assert!(res.is_ok());

                let info = res.unwrap().to_owned();
                assert!(info.last_updated_at > 0);
                ret.push(info.content_hash);

                ret
            })
            .fold(HashSet::new(), |mut h, i| {
                h.insert(i);
                h
            });
        assert_eq!(results.len(), 1);
    }

    // The smallest fixture that did *not* demonstrate non-deterministic behavior
    // with the old implementation.
    #[mononoke::test]
    fn test_build_config_info_minimal() {
        let results = (1..10)
            .map(|_| {
                let json = fixtures::json_config_minimal();
                let raw_repo_configs =
                    serde_json::from_str::<RawRepoConfigs>(&json).expect("Unable to parse");

                let res = build_config_info(Arc::new(raw_repo_configs));
                assert!(res.is_ok());

                let info = res.unwrap().to_owned();
                assert!(info.last_updated_at > 0);

                info.content_hash
            })
            .fold(HashSet::new(), |mut h, i| {
                h.insert(i);
                h
            });

        assert_eq!(results.len(), 1);
    }

    // The smallest fixture that *did* demonstrate non-deterministic behavior
    // with the old implementation.
    #[mononoke::test]
    fn test_build_config_info_small() {
        let results = (1..10)
            .map(|_| {
                let json = fixtures::json_config_small();
                let raw_repo_configs =
                    serde_json::from_str::<RawRepoConfigs>(&json).expect("Unable to parse");

                let res = build_config_info(Arc::new(raw_repo_configs));
                assert!(res.is_ok());

                let info = res.unwrap().to_owned();
                assert!(info.last_updated_at > 0);

                info.content_hash
            })
            .fold(HashSet::new(), |mut h, i| {
                h.insert(i);
                h
            });
        assert_eq!(results.len(), 1);
    }

    fn tier_entry(name: &str, is_deep_sharded: bool) -> TierRepoEntry {
        TierRepoEntry {
            repo_name: name.to_owned(),
            is_deep_sharded,
            ..Default::default()
        }
    }

    fn manifest_with(entries: Vec<TierRepoEntry>) -> TierManifest {
        TierManifest {
            repos: entries,
            ..Default::default()
        }
    }

    // Regression: deep-sharded handles inserted on-demand by ShardManager
    // must survive manifest refresh.
    #[mononoke::test]
    fn test_compute_handles_to_remove_preserves_deep_sharded() {
        let manifest = manifest_with(vec![
            tier_entry("non_sharded_repo", false),
            tier_entry("deep_sharded_repo", true),
        ]);
        let current: HashSet<String> = ["non_sharded_repo", "deep_sharded_repo"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let to_remove = compute_handles_to_remove(&current, &manifest);
        assert!(
            to_remove.is_empty(),
            "deep-sharded repo present in manifest must not be removed, got {:?}",
            to_remove,
        );
    }

    #[mononoke::test]
    fn test_compute_handles_to_remove_drops_repos_missing_from_manifest() {
        let manifest = manifest_with(vec![tier_entry("still_present", true)]);
        let current: HashSet<String> = ["still_present", "gone_from_manifest"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let to_remove = compute_handles_to_remove(&current, &manifest);
        assert_eq!(
            to_remove,
            vec!["gone_from_manifest".to_string()],
            "only entries absent from manifest should be removed",
        );
    }

    #[mononoke::test]
    fn test_compute_handles_to_remove_empty_manifest() {
        let manifest = manifest_with(vec![]);
        let current: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let mut to_remove = compute_handles_to_remove(&current, &manifest);
        to_remove.sort();
        assert_eq!(to_remove, vec!["a".to_string(), "b".to_string()]);
    }

    #[mononoke::test]
    fn test_compute_handles_to_remove_empty_current() {
        let manifest = manifest_with(vec![tier_entry("a", false), tier_entry("b", true)]);
        let current: HashSet<String> = HashSet::new();
        let to_remove = compute_handles_to_remove(&current, &manifest);
        assert!(to_remove.is_empty());
    }
}
