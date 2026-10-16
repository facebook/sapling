/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repo Name Provider.
//!
//! Trait outlining the interface for providing repo names.

use metaconfig_parser::RepoConfigs;
use repo_update_logger::GitContentRefInfo;
use repo_update_logger::PlainBookmarkInfo;

use crate::RepoName;

/// Trait outlining the interface for providing repo names.
pub trait RepoNameProvider {
    /// Get the name of the repo.
    fn repo_name(&self, repo_configs: &RepoConfigs) -> anyhow::Result<RepoName>;
}

impl RepoNameProvider for PlainBookmarkInfo {
    fn repo_name(&self, _repo_configs: &RepoConfigs) -> anyhow::Result<RepoName> {
        Ok(self.repo_name().to_string())
    }
}

impl RepoNameProvider for GitContentRefInfo {
    fn repo_name(&self, _repo_configs: &RepoConfigs) -> anyhow::Result<RepoName> {
        Ok(self.repo_name.to_string())
    }
}
