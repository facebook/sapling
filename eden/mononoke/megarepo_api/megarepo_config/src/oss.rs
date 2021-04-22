/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;
use context::CoreContext;
use fbinit::FacebookInit;
use megarepo_configs::types::{SyncConfigVersion, SyncTargetConfig, Target};
use megarepo_error::MegarepoError;
use slog::{warn, Logger};

use crate::MononokeMegarepoConfigs;

/// This struct is a no-op stub, to allow for successful builds
/// outside of fbcode. While it is allowed to instantiate this struct,
/// (to allow creating of dependent structs that instantiate
/// MononokeMegarepoConfigs as part of their setup) it is illegal
/// to call any of the methods.
pub struct CfgrMononokeMegarepoConfigs;

impl CfgrMononokeMegarepoConfigs {
    pub fn new(_fb: FacebookInit, logger: &Logger) -> Result<Self, MegarepoError> {
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
