/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(backtrace)]
#![deny(warnings)]

use async_trait::async_trait;
use context::CoreContext;
pub use megarepo_configs::types::{
    Source, SourceMappingRules, SourceRevision, SyncConfigVersion, SyncTargetConfig, Target,
};
use megarepo_error::MegarepoError;
#[cfg(fbcode_build)]
mod facebook;
#[cfg(not(fbcode_build))]
mod oss;
mod test_impl;

#[cfg(fbcode_build)]
pub use facebook::CfgrMononokeMegarepoConfigs;
#[cfg(not(fbcode_build))]
pub use oss::CfgrMononokeMegarepoConfigs;
pub use test_impl::TestMononokeMegarepoConfigs;

/// An API for Megarepo Configs
#[async_trait]
pub trait MononokeMegarepoConfigs: Send + Sync {
    /// Get all the versions for a given Target
    fn get_target_config_versions(
        &self,
        ctx: CoreContext,
        target: Target,
    ) -> Result<Vec<SyncConfigVersion>, MegarepoError>;

    /// Get a SyncTargetConfig by its version
    fn get_config_by_version(
        &self,
        ctx: CoreContext,
        target: Target,
        version: SyncConfigVersion,
    ) -> Result<SyncTargetConfig, MegarepoError>;

    /// Add a Target with an initial SyncTargetConfig
    /// Note: this method is identical to `add_config_version`, but
    /// it expects the target to not exist. The reason this method
    /// exists instead of a combination of (add_target, add_config_version)
    /// is that I want it to be impossible to create a target without
    /// a single config version associated
    async fn add_target_with_config_version(
        &self,
        ctx: CoreContext,
        config: SyncTargetConfig,
    ) -> Result<(), MegarepoError>;

    /// Add a new unused SyncTargetConfig for an existing Target
    /// Note: this method is identical to `add_target_with_config_version`, but
    /// it expects the target to exist
    async fn add_config_version(
        &self,
        ctx: CoreContext,
        config: SyncTargetConfig,
    ) -> Result<(), MegarepoError>;
}
