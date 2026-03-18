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
use tracing::error;
use tracing::info;
use tracing::trace;
use tracing::warn;

const LIVENESS_INTERVAL: u64 = 300;
const CONFIGERATOR_TIER_PREFIX: &str = "configerator://scm/mononoke/repos/tiers/";
type Swappable<T> = Arc<ArcSwap<T>>;

define_stats! {
    prefix = "mononoke.config_refresh";
    refresh_failure_count: timeseries(Average, Sum, Count),
    liveness_count: timeseries(Average, Sum, Count),
}

/// Configuration provider and update notifier for all of Mononoke services
/// and jobs. The configurations provided by this struct are always up-to-date
/// with its source.
pub struct MononokeConfigs {
    repo_configs: Swappable<RepoConfigs>,
    storage_configs: Swappable<StorageConfigs>,
    update_receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>>,
    config_info: Swappable<Option<ConfigInfo>>,
    maybe_config_updater: Option<JoinHandle<()>>,
    maybe_liveness_updater: Option<JoinHandle<()>>,
    maybe_manifest_updater: Option<JoinHandle<()>>,
    maybe_config_handle: Option<ConfigHandle<RawRepoConfigs>>,
    // Per-repo split-loading fields
    maybe_manifest_handle: Option<ConfigHandle<TierManifest>>,
    repo_handles: Arc<RwLock<HashMap<String, ConfigHandle<RepoSpec>>>>,
    config_store: Option<ConfigStore>,
    /// Tier name derived from the configerator config path.
    /// Used for resolving tier_overrides in RepoSpec configs during split-loading.
    #[allow(dead_code)] // Stored for potential future use in load_repo_config_handle
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
            if let Ok(new_config_info) = build_config_info(config_handle.get()) {
                Some(new_config_info)
            } else {
                warn!("Could not compute new config_info");
                None
            }
        } else {
            None
        };
        let config_info = Arc::new(ArcSwap::from_pointee(config_info));
        let maybe_config_watcher = maybe_config_handle
            .as_ref()
            .map(|config_handle| config_handle.watcher())
            .transpose()?;
        // If we are dynamically updating the config, we need to have a liveness updater process in place.
        let maybe_liveness_updater = maybe_config_watcher
            .as_ref()
            .map(|_| runtime_handle.spawn(liveness_updater()));
        // If the configuration is backed by a static source, the config update watcher
        // and the config updater handle will be None.
        let maybe_config_updater = maybe_config_watcher.map(|config_watcher| {
            cloned!(storage_configs, repo_configs, config_info, update_receivers);
            runtime_handle.spawn(watch_and_update(
                repo_configs,
                storage_configs,
                config_info,
                update_receivers,
                config_watcher,
            ))
        });
        let maybe_manifest_handle = manifest_path
            .map(|path| configerator_manifest_handle(path, config_store))
            .transpose()?;

        // Only log split-loading status for configerator-backed configs (where
        // tier_name is set). File-backed configs used in tests always have
        // tier_name=None and manifest_path=None, so logging would be noise.
        if tier_name.is_some() {
            if let Some(manifest_path) = manifest_path {
                info!(
                    "Split-loading enabled: config_path={}, manifest_path={}, tier_name={:?}",
                    config_path.as_ref().to_string_lossy(),
                    manifest_path,
                    tier_name.as_deref().unwrap_or("<none>"),
                );
            } else {
                info!(
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

        let maybe_manifest_updater = if let Some(ref manifest_handle) = maybe_manifest_handle {
            cloned!(
                repo_handles,
                repo_configs,
                storage_configs,
                update_receivers,
                config_store,
            );
            let tier = tier_name.clone();
            Some(runtime_handle.spawn(watch_manifest_and_repos(
                manifest_handle.clone(),
                repo_handles,
                config_store,
                tier,
                repo_configs,
                storage_configs,
                update_receivers,
            )))
        } else {
            None
        };

        Ok(Self {
            repo_configs,
            storage_configs,
            update_receivers,
            config_info,
            maybe_config_updater,
            maybe_config_handle,
            maybe_liveness_updater,
            maybe_manifest_updater,
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
        // If the manifest updater process exists, abort it.
        if let Some(manifest_updater) = self.maybe_manifest_updater.as_ref() {
            manifest_updater.abort();
        }
    }
}

async fn liveness_updater() {
    loop {
        STATS::liveness_count.add_value(1);
        tokio::time::sleep(tokio::time::Duration::from_secs(LIVENESS_INTERVAL)).await;
    }
}

async fn watch_and_update(
    repo_configs: Swappable<RepoConfigs>,
    storage_configs: Swappable<StorageConfigs>,
    config_info: Swappable<Option<ConfigInfo>>,
    update_receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>>,
    mut config_watcher: ConfigUpdateWatcher<RawRepoConfigs>,
) {
    loop {
        match config_watcher.wait_for_next().await {
            Ok(raw_repo_configs) => {
                info!("Raw Repo Configs changed in config store, applying update");
                trace!("Applied configs: {:?}", raw_repo_configs);
                let original_raw_repo_configs = raw_repo_configs.clone();
                match load_configs_from_raw(Arc::unwrap_or_clone(raw_repo_configs)) {
                    Ok((new_repo_configs, new_storage_configs)) => {
                        if let Ok(new_config_info) = build_config_info(original_raw_repo_configs) {
                            let new_config_info = Arc::new(Some(new_config_info));
                            config_info.store(new_config_info);
                        } else {
                            warn!("Could not compute new config_info");
                        }
                        let new_repo_configs = Arc::new(new_repo_configs);
                        let new_storage_configs = Arc::new(new_storage_configs);
                        repo_configs.store(new_repo_configs.clone());
                        storage_configs.store(new_storage_configs.clone());
                        let receivers = update_receivers.load();
                        let update_tasks = receivers.iter().map(|receiver| {
                            receiver
                                .apply_update(new_repo_configs.clone(), new_storage_configs.clone())
                        });
                        if let Err(e) = join_all(update_tasks)
                            .await
                            .into_iter()
                            .collect::<Result<Vec<_>>>()
                        {
                            error!(
                                "Failure in sending config update to receivers. Error: {:?}",
                                e
                            );
                            STATS::refresh_failure_count.add_value(1);
                        } else {
                            info!("Successfully applied config update");
                        }
                    }
                    Err(e) => {
                        error!("Failure in parsing config from raw config. Error: {:?}", e);
                        STATS::refresh_failure_count.add_value(1);
                    }
                }
            }
            Err(e) => {
                error!("Failure in fetching latest config change. Error: {:?}", e);
                STATS::refresh_failure_count.add_value(1);
            }
        }
    }
}

/// Result of syncing repo handles with the manifest.
struct RepoHandleSyncResult {
    /// Repo names that were tracked before the sync.
    previously_tracked: HashSet<String>,
    /// Repo names currently in the manifest (non-deep-sharded only).
    manifest_repos: HashSet<String>,
}

/// Syncs repo_handles with the manifest: adds handles for new repos, removes
/// handles for repos no longer in the manifest.
fn sync_repo_handles(
    manifest: &TierManifest,
    repo_handles: &RwLock<HashMap<String, ConfigHandle<RepoSpec>>>,
    config_store: &ConfigStore,
) -> Result<RepoHandleSyncResult> {
    let current_repos: HashSet<String> = repo_handles
        .read()
        .map_err(|e| anyhow!("repo_handles lock poisoned: {}", e))?
        .keys()
        .cloned()
        .collect();

    let manifest_repos: HashSet<String> = manifest
        .repos
        .iter()
        .filter(|e| !e.is_deep_sharded)
        .map(|e| e.repo_name.clone())
        .collect();

    // Collect all new handles first, then insert in bulk under one lock
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

    // Repos to remove
    let to_remove: Vec<&String> = current_repos.difference(&manifest_repos).collect();

    // Single write lock acquisition for both adds and removes
    if !new_handles.is_empty() || !to_remove.is_empty() {
        let mut handles = repo_handles
            .write()
            .map_err(|e| anyhow!("repo_handles lock poisoned: {}", e))?;
        handles.extend(new_handles);
        for repo_name in &to_remove {
            handles.remove(*repo_name);
            info!("Removed config handle for repo: {}", repo_name);
        }
    }

    Ok(RepoHandleSyncResult {
        previously_tracked: current_repos,
        manifest_repos,
    })
}

/// Re-parses all per-repo configs and merges them into the existing repo_configs.
/// Legacy repos not managed by split-loading are preserved. Repos that were
/// previously managed but are no longer in the manifest are removed.
fn parse_and_merge_repo_configs(
    manifest: &TierManifest,
    repo_handles: &RwLock<HashMap<String, ConfigHandle<RepoSpec>>>,
    tier_name: &str,
    current_configs: &RepoConfigs,
    previously_tracked: &HashSet<String>,
    manifest_repos: &HashSet<String>,
) -> Result<RepoConfigs> {
    if tier_name.is_empty() {
        anyhow::bail!("tier_name must not be empty for split-loading config resolution");
    }

    let handles = repo_handles
        .read()
        .map_err(|e| anyhow!("repo_handles lock poisoned: {}", e))?
        .clone();
    let mut merged_repos = current_configs.repos.clone();

    for entry in &manifest.repos {
        if let Some(handle) = handles.get(&entry.repo_name) {
            let repo_spec = handle.get();
            match parse_repo_spec(
                Arc::unwrap_or_clone(repo_spec),
                tier_name,
                &manifest.storage,
            ) {
                Ok(repo_config) => {
                    merged_repos.insert(entry.repo_name.clone(), repo_config);
                }
                Err(e) => {
                    error!(
                        "Failed to parse config for repo {}: {:?}",
                        entry.repo_name, e
                    );
                    STATS::refresh_failure_count.add_value(1);
                }
            }
        }
    }

    // Remove repos that were previously managed by split-loading
    // but are no longer in the manifest
    for repo_name in previously_tracked.difference(manifest_repos) {
        merged_repos.remove(repo_name);
    }

    Ok(RepoConfigs {
        repos: merged_repos,
        common: current_configs.common.clone(),
    })
}

/// Watches the TierManifest for structural changes (repos added/removed) and
/// per-repo ConfigHandles for config value changes. On any change, re-parses
/// affected repos and updates the `repo_configs` ArcSwap.
///
/// This is the split-loading equivalent of `watch_and_update`.
async fn watch_manifest_and_repos(
    manifest_handle: ConfigHandle<TierManifest>,
    repo_handles: Arc<RwLock<HashMap<String, ConfigHandle<RepoSpec>>>>,
    config_store: ConfigStore,
    tier_name: Option<String>,
    repo_configs: Swappable<RepoConfigs>,
    storage_configs: Swappable<StorageConfigs>,
    update_receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>>,
) {
    let mut manifest_watcher = match manifest_handle.watcher() {
        Ok(w) => w,
        Err(e) => {
            error!("Failed to create manifest watcher: {:?}", e);
            return;
        }
    };

    loop {
        match manifest_watcher.wait_for_next().await {
            Ok(new_manifest) => {
                info!("TierManifest changed, syncing repo handles and configs");

                let sync_result =
                    match sync_repo_handles(&new_manifest, &repo_handles, &config_store) {
                        Ok(result) => result,
                        Err(e) => {
                            error!("Failed to sync repo handles: {:?}", e);
                            STATS::refresh_failure_count.add_value(1);
                            continue;
                        }
                    };

                let tier = match tier_name.as_deref() {
                    Some(t) => t,
                    None => {
                        error!("tier_name is required for split-loading but was not provided");
                        STATS::refresh_failure_count.add_value(1);
                        continue;
                    }
                };
                let current = repo_configs.load();
                let new_repo_configs = match parse_and_merge_repo_configs(
                    &new_manifest,
                    &repo_handles,
                    tier,
                    &current,
                    &sync_result.previously_tracked,
                    &sync_result.manifest_repos,
                ) {
                    Ok(configs) => Arc::new(configs),
                    Err(e) => {
                        error!("Failed to parse and merge repo configs: {:?}", e);
                        STATS::refresh_failure_count.add_value(1);
                        continue;
                    }
                };
                repo_configs.store(new_repo_configs.clone());

                // Notify update receivers
                let current_storage = storage_configs.load_full();
                let receivers = update_receivers.load();
                let update_tasks = receivers.iter().map(|receiver| {
                    receiver.apply_update(new_repo_configs.clone(), current_storage.clone())
                });
                if let Err(e) = join_all(update_tasks)
                    .await
                    .into_iter()
                    .collect::<Result<Vec<_>>>()
                {
                    error!(
                        "Failure in sending config update to receivers. Error: {:?}",
                        e
                    );
                    STATS::refresh_failure_count.add_value(1);
                } else {
                    info!("Successfully applied per-repo config update");
                }
            }
            Err(e) => {
                error!("Manifest watch error: {:?}", e);
                STATS::refresh_failure_count.add_value(1);
            }
        }
    }
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
        let serialized = serde_json::to_string(&SortKeys(raw_repo_configs)).unwrap();
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
}
