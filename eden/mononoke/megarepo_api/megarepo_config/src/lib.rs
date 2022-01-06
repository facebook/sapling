/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
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
use std::path::PathBuf;
#[cfg(fbcode_build)]
mod facebook;
#[cfg(not(fbcode_build))]
mod oss;
mod test_impl;
mod verification;

pub use verification::verify_config;

#[cfg(fbcode_build)]
pub use facebook::CfgrMononokeMegarepoConfigs;
#[cfg(not(fbcode_build))]
pub use oss::CfgrMononokeMegarepoConfigs;
pub use test_impl::TestMononokeMegarepoConfigs;

/// Options for instantiating MononokeMegarepoConfigs
#[derive(Clone, PartialEq, Eq)]
pub enum MononokeMegarepoConfigsOptions {
    /// Create prod-style `MononokeMegarepoConfigs` implementation
    /// (requires fb infra to function correctly, although will
    /// successfully instantiate with `unimplemented!` methods
    /// when built outside of fbcode)
    Prod,
    /// Create a config implementation that writes JSON to disk at the
    /// given path instead of calling FB infra.
    /// Used with a testing config store, this gives you a good basis
    /// for integration tests
    IntegrationTest(PathBuf),
    /// Create test-style `MononokeMegarepoConfigs` implementation
    UnitTest,
}

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

    /// Add a new unused SyncTargetConfig for an existing Target
    async fn add_config_version(
        &self,
        ctx: CoreContext,
        config: SyncTargetConfig,
    ) -> Result<(), MegarepoError>;
}
