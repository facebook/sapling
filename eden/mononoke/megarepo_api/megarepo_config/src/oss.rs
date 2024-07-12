/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use async_trait::async_trait;
use blobstore_factory::ReadOnlyStorage;
use context::CoreContext;
use fbinit::FacebookInit;
use megarepo_configs::SyncConfigVersion;
use megarepo_configs::SyncTargetConfig;
use megarepo_configs::Target;
use megarepo_error::MegarepoError;
use metaconfig_types::RepoConfig;
use slog::warn;
use slog::Logger;
use sql_ext::facebook::MysqlOptions;

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
        _mysql_options: MysqlOptions,
        _readonly_storage: ReadOnlyStorage,
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
    async fn get_config_by_version(
        &self,
        _ctx: CoreContext,
        _repo_config: Arc<RepoConfig>,
        _target: Target,
        _version: SyncConfigVersion,
    ) -> Result<SyncTargetConfig, MegarepoError> {
        unimplemented!("OSS CfgrMononokeMegarepoConfigs::get_config_by_version")
    }

    async fn add_config_version(
        &self,
        _ctx: CoreContext,
        _repo_config: Arc<RepoConfig>,
        _config: SyncTargetConfig,
    ) -> Result<(), MegarepoError> {
        unimplemented!("OSS CfgrMononokeMegarepoConfigs::add_config_version")
    }
}
