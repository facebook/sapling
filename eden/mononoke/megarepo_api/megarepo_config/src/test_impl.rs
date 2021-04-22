/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use context::CoreContext;
use megarepo_configs::types::{SyncConfigVersion, SyncTargetConfig, Target};
use megarepo_error::MegarepoError;
use slog::{info, Logger};

use crate::MononokeMegarepoConfigs;

pub struct TestMononokeMegarepoConfigs;

impl TestMononokeMegarepoConfigs {
    pub fn new(logger: &Logger) -> Self {
        info!(logger, "Creating a new TestMononokeMegarepoConfigs");
        Self
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
        _target: Target,
        _version: SyncConfigVersion,
    ) -> Result<SyncTargetConfig, MegarepoError> {
        unimplemented!("TestMononokeMegarepoConfigs::get_config_by_version")
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
