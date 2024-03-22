/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::Result;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use cached_config::ConfigUpdateWatcher;
use cloned::cloned;
use futures::future::join_all;
use metaconfig_parser::config::configerator_config_handle;
use metaconfig_parser::config::load_configs_from_raw;
use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use metaconfig_types::ConfigInfo;
use repos::RawRepoConfigs;
use serde::Serialize;
use sha2::Digest;
use sha2::Sha256;
use slog::error;
use slog::info;
use slog::trace;
use slog::warn;
use slog::Logger;
use stats::prelude::*;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

const LIVENESS_INTERVAL: u64 = 300;
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
    maybe_config_handle: Option<ConfigHandle<RawRepoConfigs>>,
}

impl MononokeConfigs {
    /// Create a new instance of MononokeConfigs with configurations backed via ConfigStore.
    /// If the config path points to a dynamic config source (e.g. configerator), this enables
    /// auto-refresh of those configurations.
    pub fn new(
        config_path: impl AsRef<Path>,
        config_store: &ConfigStore,
        runtime_handle: Handle,
        logger: Logger,
    ) -> Result<Self> {
        let storage_configs = metaconfig_parser::load_storage_configs(&config_path, config_store)?;
        let storage_configs = Arc::new(ArcSwap::from_pointee(storage_configs));
        let repo_configs = metaconfig_parser::load_repo_configs(&config_path, config_store)?;
        let repo_configs = Arc::new(ArcSwap::from_pointee(repo_configs));
        let update_receivers = Arc::new(ArcSwap::from_pointee(vec![]));
        let maybe_config_handle = configerator_config_handle(config_path.as_ref(), config_store)?;
        let config_info = if let Some(config_handle) = maybe_config_handle.as_ref() {
            if let Ok(new_config_info) = build_config_info(config_handle.get()) {
                Some(new_config_info)
            } else {
                warn!(logger, "Could not compute new config_info");
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
                logger,
            ))
        });
        Ok(Self {
            repo_configs,
            storage_configs,
            update_receivers,
            config_info,
            maybe_config_updater,
            maybe_config_handle,
            maybe_liveness_updater,
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

async fn watch_and_update(
    repo_configs: Swappable<RepoConfigs>,
    storage_configs: Swappable<StorageConfigs>,
    config_info: Swappable<Option<ConfigInfo>>,
    update_receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>>,
    mut config_watcher: ConfigUpdateWatcher<RawRepoConfigs>,
    logger: Logger,
) {
    loop {
        match config_watcher.wait_for_next().await {
            Ok(raw_repo_configs) => {
                info!(
                    logger,
                    "Raw Repo Configs changed in config store, applying update"
                );
                trace!(logger, "Applied configs: {:?}", raw_repo_configs);
                let original_raw_repo_configs = raw_repo_configs.clone();
                match load_configs_from_raw(Arc::unwrap_or_clone(raw_repo_configs)) {
                    Ok((new_repo_configs, new_storage_configs)) => {
                        if let Ok(new_config_info) = build_config_info(original_raw_repo_configs) {
                            let new_config_info = Arc::new(Some(new_config_info));
                            config_info.store(new_config_info);
                        } else {
                            warn!(logger, "Could not compute new config_info");
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
                                logger,
                                "Failure in sending config update to receivers. Error: {:?}", e
                            );
                            STATS::refresh_failure_count.add_value(1);
                        } else {
                            info!(logger, "Successfully applied config update");
                            // Need to publish a value of 0 to keep the counter alive
                            STATS::refresh_failure_count.add_value(0);
                        }
                    }
                    Err(e) => {
                        error!(
                            logger,
                            "Failure in parsing config from raw config. Error: {:?}", e
                        );
                        STATS::refresh_failure_count.add_value(1);
                    }
                }
            }
            Err(e) => {
                error!(
                    logger,
                    "Failure in fetching latest config change. Error: {:?}", e
                );
                STATS::refresh_failure_count.add_value(1);
            }
        }
    }
}

/// Trait defining methods related to config update notification. A struct implementing
/// this trait can be configured to receive the most updated config value everytime the
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

    use repos::RawRepoConfig;
    use repos::RawRepoConfigs;

    use super::*;

    #[test]
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

    #[test]
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

    #[test]
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

    // The smallest fixture that did *not* demostrate non-deterministic behavior
    // with the old implementation.
    #[test]
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

    // The smallest fixture that *did* demostrate non-deterministic behavior
    // with the old implementation.
    #[test]
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
