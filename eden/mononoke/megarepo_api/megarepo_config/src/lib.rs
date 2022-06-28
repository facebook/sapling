/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(backtrace)]
#![cfg_attr(not(fbcode_build), allow(unused_crate_dependencies))]

use async_trait::async_trait;
use clap::Args;
use context::CoreContext;
pub use megarepo_configs::types::MergeMode;
pub use megarepo_configs::types::Source;
pub use megarepo_configs::types::SourceMappingRules;
pub use megarepo_configs::types::SourceRevision;
pub use megarepo_configs::types::Squashed;
pub use megarepo_configs::types::SyncConfigVersion;
pub use megarepo_configs::types::SyncTargetConfig;
pub use megarepo_configs::types::Target;
pub use megarepo_configs::types::WithExtraMoveCommit;
use megarepo_error::MegarepoError;
use std::path::Path;
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

/// Command line arguments for controlling Megarepo configs
#[derive(Args, Debug)]
pub struct MegarepoConfigsArgs {
    /// Whether to instantiate test-style MononokeMegarepoConfigs
    ///
    /// Prod-style instance reads/writes from/to configerator and
    /// requires the FB environment to work properly.
    // For compatibility with existing usage, this arg takes value
    // for example `--with-test-megarepo-configs-client=true`.
    #[clap(
        long,
        parse(try_from_str),
        default_value_t = false,
        value_name = "BOOL"
    )]
    pub with_test_megarepo_configs_client: bool,
}

impl MononokeMegarepoConfigsOptions {
    pub fn from_args(
        local_configerator_path: Option<&Path>,
        megarepo_configs_args: &MegarepoConfigsArgs,
    ) -> Self {
        if megarepo_configs_args.with_test_megarepo_configs_client {
            if let Some(path) = local_configerator_path {
                MononokeMegarepoConfigsOptions::IntegrationTest(path.to_path_buf())
            } else {
                MononokeMegarepoConfigsOptions::UnitTest
            }
        } else {
            MononokeMegarepoConfigsOptions::Prod
        }
    }
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
