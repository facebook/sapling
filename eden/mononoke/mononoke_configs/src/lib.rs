/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use cached_config::ConfigStore;
use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use tokio::task::JoinHandle;

/// Configuration provider and config update notifier for all of Mononoke
/// services and jobs. The configurations provided by this struct are always
/// up-to-date with its source.
pub struct MononokeConfigs {
    repo_configs: Arc<ArcSwap<RepoConfigs>>,
    storage_configs: Arc<ArcSwap<StorageConfigs>>,
    update_receivers: Arc<ArcSwap<Vec<Arc<dyn ConfigUpdateReceiver>>>>,
    config_updater: Option<JoinHandle<()>>,
}

impl MononokeConfigs {
    /// Create a new instance of MononokeConfigs with configurations backed via ConfigStore.
    /// If the config path points to a dynamic config source (e.g. configerator), this enables
    /// auto-refresh of those configurations.
    pub fn new(config_path: impl AsRef<Path>, config_store: &ConfigStore) -> Self {
        todo!()
    }

    /// The latest repo configs fetched from the underlying configuration store.
    pub fn repo_configs(&self) -> Arc<RepoConfigs> {
        todo!()
    }

    /// The latest storage configs fetched from the underlying configuration store.
    pub fn storage_configs(&self) -> Arc<StorageConfigs> {
        todo!()
    }

    /// Register an instance of ConfigUpdateReceiver to receive notifications of updates to
    /// the underlying configs which can then be used to perform further actions.
    pub fn register_for_update(&self, update_receiver: Arc<dyn ConfigUpdateReceiver>) {
        todo!()
    }
}

/// Trait defining methods related to config update notification. A struct implementing
/// this trait can be configured to receive the most updated config value everytime the
/// underlying config changes.
#[async_trait]
pub trait ConfigUpdateReceiver {
    /// Method containing the logic to be executed when the configuration is updated. This
    /// should not be too long running since the config updates will wait for all update
    /// receivers before checking for the next config update.
    async fn apply_update(
        &self,
        repo_configs: Arc<RepoConfigs>,
        storage_configs: Arc<StorageConfigs>,
    ) -> Result<()>;
}
