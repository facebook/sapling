/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(arc_unwrap_or_clone)]

use std::path::Path;
use std::sync::Arc;

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
use repos::RawRepoConfigs;
use slog::error;
use slog::info;
use slog::Logger;
use stats::prelude::*;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

type Swappable<T> = Arc<ArcSwap<T>>;

define_stats! {
    prefix = "mononoke.config_refresh";
    refresh_failure_count: timeseries(Average, Sum, Count),
}

/// Configuration provider and update notifier for all of Mononoke services
/// and jobs. The configurations provided by this struct are always up-to-date
/// with its source.
pub struct MononokeConfigs {
    repo_configs: Swappable<RepoConfigs>,
    storage_configs: Swappable<StorageConfigs>,
    update_receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>>,
    maybe_config_updater: Option<JoinHandle<()>>,
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
        let maybe_config_watcher = maybe_config_handle
            .as_ref()
            .map(|config_handle| config_handle.watcher())
            .transpose()?;
        // If the configuration is backed by a static source, the config update watcher
        // and the config updater handle will be None.
        let maybe_config_updater = maybe_config_watcher.map(|config_watcher| {
            cloned!(storage_configs, repo_configs, update_receivers);
            runtime_handle.spawn(watch_and_update(
                repo_configs,
                storage_configs,
                update_receivers,
                config_watcher,
                logger,
            ))
        });
        Ok(Self {
            repo_configs,
            storage_configs,
            update_receivers,
            maybe_config_updater,
            maybe_config_handle,
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
    }
}

async fn watch_and_update(
    repo_configs: Swappable<RepoConfigs>,
    storage_configs: Swappable<StorageConfigs>,
    update_receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>>,
    mut config_watcher: ConfigUpdateWatcher<RawRepoConfigs>,
    logger: Logger,
) {
    loop {
        match config_watcher.wait_for_next().await {
            Ok(raw_repo_configs) => {
                info!(
                    logger,
                    "Raw Repo Configs changed in config store, applying update: {:?}",
                    raw_repo_configs
                );
                match load_configs_from_raw(Arc::unwrap_or_clone(raw_repo_configs)) {
                    Ok((new_repo_configs, new_storage_configs)) => {
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
