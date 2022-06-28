/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use cached_config::ConfigStore;
use context::CoreContext;
use fbinit::FacebookInit;
use megarepo_configs::types::SyncConfigVersion;
use megarepo_configs::types::SyncTargetConfig;
use megarepo_configs::types::Target;
use megarepo_error::MegarepoError;
use slog::warn;
use slog::Logger;
use std::path::PathBuf;

use crate::MononokeMegarepoConfigs;

/// This struct is a no-op stub, to allow for successful builds
/// outside of fbcode. While it is allowed to instantiate this struct,
/// (to allow creating of dependent structs that instantiate
/// MononokeMegarepoConfigs as part of their setup) it is illegal
/// to call any of the methods.
pub struct CfgrMononokeMegarepoConfigs;

impl CfgrMononokeMegarepoConfigs {
    pub fn new(
        _fb: FacebookInit,
        logger: &Logger,
        _config_store: ConfigStore,
        _test_write_path: Option<PathBuf>,
    ) -> Result<Self, MegarepoError> {
        warn!(
            logger,
            "CfgrMononokeMegarepoConfigs is not implemented for non-fbcode builds"
        );
        // While this struct should never be used in practice outside of fbcode, I think
        // it shouldn't be an error to simply instantiate it in non-fbcode
        // builds, so let's return `Ok` here.
        Ok(Self)
    }
}

#[async_trait]
impl MononokeMegarepoConfigs for CfgrMononokeMegarepoConfigs {
    fn get_target_config_versions(
        &self,
        _ctx: CoreContext,
        _target: Target,
    ) -> Result<Vec<SyncConfigVersion>, MegarepoError> {
        unimplemented!("OSS CfgrMononokeMegarepoConfigs::get_target_config_versions")
    }

    fn get_config_by_version(
        &self,
        _ctx: CoreContext,
        _target: Target,
        _version: SyncConfigVersion,
    ) -> Result<SyncTargetConfig, MegarepoError> {
        unimplemented!("OSS CfgrMononokeMegarepoConfigs::get_config_by_version")
    }

    async fn add_config_version(
        &self,
        _ctx: CoreContext,
        _config: SyncTargetConfig,
    ) -> Result<(), MegarepoError> {
        unimplemented!("OSS CfgrMononokeMegarepoConfigs::add_config_version")
    }
}
