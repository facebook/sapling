/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Repo Name Provider.
//!
//! Trait outlining the interface for providing repo names.

use anyhow::Context;
use metaconfig_parser::RepoConfigs;
use mononoke_bookmark_event_notification::BookmarkEventNotification;
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

impl RepoNameProvider for BookmarkEventNotification {
    fn repo_name(&self, repo_configs: &RepoConfigs) -> anyhow::Result<RepoName> {
        let (repo_name, _) = repo_configs
            .get_repo_config_by_raw_id(self.repo_id)
            .with_context(|| format!("repository name not found for repo ID {}", self.repo_id))?;
        Ok(repo_name.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use anyhow::Result;
    use metaconfig_types::CommonConfig;
    use metaconfig_types::RepoConfig;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn resolves_bookmark_event_repository_name() -> Result<()> {
        let repo_config = RepoConfig::default();
        let repo_id = repo_config.repoid.id();
        let repo_configs = RepoConfigs::new(
            HashMap::from([("repo".to_owned(), repo_config)]),
            CommonConfig::default(),
        );
        let notification = BookmarkEventNotification {
            repo_id,
            ..Default::default()
        };

        assert_eq!(notification.repo_name(&repo_configs)?, "repo");
        Ok(())
    }
}
