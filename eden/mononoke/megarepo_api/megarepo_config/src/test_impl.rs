/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use async_trait::async_trait;
use context::CoreContext;
use megarepo_configs::types::{SyncConfigVersion, SyncTargetConfig, Target};
use megarepo_error::MegarepoError;
use slog::{info, Logger};
use std::collections::HashMap;

use crate::MononokeMegarepoConfigs;

pub struct TestMononokeMegarepoConfigs {
    config_versions: HashMap<(Target, SyncConfigVersion), SyncTargetConfig>,
}

impl TestMononokeMegarepoConfigs {
    pub fn new(logger: &Logger) -> Self {
        info!(logger, "Creating a new TestMononokeMegarepoConfigs");
        Self {
            config_versions: HashMap::new(),
        }
    }

    pub fn add(&mut self, key: (Target, SyncConfigVersion), target: SyncTargetConfig) {
        self.config_versions.insert(key, target);
    }
}

#[async_trait]
impl MononokeMegarepoConfigs for TestMononokeMegarepoConfigs {
    fn get_target_config_versions(
        &self,
        _ctx: CoreContext,
        _target: Target,
    ) -> Result<Vec<SyncConfigVersion>, MegarepoError> {
        unimplemented!("TestMononokeMegarepoConfigs::get_target_config_versions")
    }

    fn get_config_by_version(
        &self,
        _ctx: CoreContext,
        target: Target,
        version: SyncConfigVersion,
    ) -> Result<SyncTargetConfig, MegarepoError> {
        self.config_versions
            .get(&(target.clone(), version.clone()))
            .cloned()
            .ok_or_else(|| anyhow!("{:?} not found", (target, version)))
            .map_err(MegarepoError::internal)
    }

    async fn add_target_with_config_version(
        &self,
        _ctx: CoreContext,
        _config: SyncTargetConfig,
    ) -> Result<(), MegarepoError> {
        unimplemented!("TestMononokeMegarepoConfigs::add_target_with_config_version")
    }

    async fn add_config_version(
        &self,
        _ctx: CoreContext,
        _config: SyncTargetConfig,
    ) -> Result<(), MegarepoError> {
        unimplemented!("TestMononokeMegarepoConfigs::add_config_version")
    }
}
