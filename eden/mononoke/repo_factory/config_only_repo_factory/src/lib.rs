/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Very, very lightweight repository factory
//!
//! Repo factory meant to be used by short-lived binaries where binary size and startup time is
//! crucial. In those cases we won't be connecting to mononoke blobstores and databases so we don't
//! need them but it's useful to be able to have those binaries able to use normal Mononoke
//! configuration.

use std::sync::Arc;

pub use blobstore_factory::BlobstoreOptions;
pub use blobstore_factory::ReadOnlyStorage;
use metaconfig_types::ArcCommonConfig;
use metaconfig_types::ArcRepoConfig;
use metaconfig_types::CommonConfig;
use metaconfig_types::RepoConfig;
use repo_identity::ArcRepoIdentity;
use repo_identity::RepoIdentity;

#[derive(Clone)]
pub struct ConfigOnlyRepoFactory {}

impl ConfigOnlyRepoFactory {
    pub fn new() -> ConfigOnlyRepoFactory {
        ConfigOnlyRepoFactory {}
    }
}

#[facet::factory(name: String, repo_config_param: RepoConfig, common_config_param: CommonConfig)]
impl ConfigOnlyRepoFactory {
    pub fn repo_config(&self, repo_config_param: &RepoConfig) -> ArcRepoConfig {
        Arc::new(repo_config_param.clone())
    }

    pub fn common_config(&self, common_config_param: &CommonConfig) -> ArcCommonConfig {
        Arc::new(common_config_param.clone())
    }

    pub fn repo_identity(&self, name: &str, repo_config: &ArcRepoConfig) -> ArcRepoIdentity {
        Arc::new(RepoIdentity::new(repo_config.repoid, name.to_string()))
    }
}
