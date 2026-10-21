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
use mononoke_bookmark_event_notification::BookmarkEventNotification;
use repo_update_logger::GitContentRefInfo;
use repo_update_logger::PlainBookmarkInfo;

use crate::RepoName;

/// Trait outlining the interface for providing repo names.
pub trait RepoNameProvider {
    /// Resolve the repository name when this process can route the notification.
    fn repo_name(&self, repo_configs: &RepoConfigs) -> Option<RepoName>;
}

impl RepoNameProvider for PlainBookmarkInfo {
    fn repo_name(&self, _repo_configs: &RepoConfigs) -> Option<RepoName> {
        Some(self.repo_name().to_string())
    }
}

impl RepoNameProvider for GitContentRefInfo {
    fn repo_name(&self, _repo_configs: &RepoConfigs) -> Option<RepoName> {
        Some(self.repo_name.to_string())
    }
}

impl RepoNameProvider for BookmarkEventNotification {
    fn repo_name(&self, repo_configs: &RepoConfigs) -> Option<RepoName> {
        repo_configs
            .get_repo_config_by_raw_id(self.repo_id)
            .map(|(repo_name, _)| repo_name.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use metaconfig_types::CommonConfig;
    use metaconfig_types::RepoConfig;
    use mononoke_macros::mononoke;

    use super::*;

    #[mononoke::test]
    fn resolves_bookmark_event_repository_name() {
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

        assert_eq!(
            notification.repo_name(&repo_configs).as_deref(),
            Some("repo")
        );
    }

    #[mononoke::test]
    fn ignores_bookmark_event_for_unknown_repository() {
        let repo_configs = RepoConfigs::new(HashMap::new(), CommonConfig::default());
        let notification = BookmarkEventNotification {
            repo_id: 123,
            ..Default::default()
        };

        assert_eq!(notification.repo_name(&repo_configs), None);
    }
}
