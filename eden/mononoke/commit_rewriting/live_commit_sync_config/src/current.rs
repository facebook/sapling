/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use commitsync::types::RawCommitSyncCurrentVersions;
use mononoke_types::RepositoryId;
use pushredirect_enable::types::MononokePushRedirectEnable;
use vec1::vec1;
use vec1::Vec1;

#[derive(Clone)]
pub struct CfgrCurrentCommitSyncConfig {
    config_handle_for_current_version: ConfigHandle<RawCommitSyncCurrentVersions>,
    config_handle_for_push_redirection: ConfigHandle<MononokePushRedirectEnable>,
}

pub enum RepoGroup {
    // Not on any megarepo
    Single(RepositoryId),
    // Megarepo with a large repo and (possibly) many small
    // More complicated structures not currently supported
    Megarepo {
        large: RepositoryId,
        small: Vec1<RepositoryId>,
    },
}

impl CfgrCurrentCommitSyncConfig {
    pub fn new(config_store: &ConfigStore) -> Result<Self> {
        let config_handle_for_push_redirection =
            config_store.get_config_handle(super::CONFIGERATOR_PUSHREDIRECT_ENABLE.to_string())?;
        let config_handle_for_current_version = config_store
            .get_config_handle(super::CONFIGERATOR_CURRENT_COMMIT_SYNC_CONFIG.to_string())?;
        Ok(Self {
            config_handle_for_current_version,
            config_handle_for_push_redirection,
        })
    }

    pub async fn repo_group(&self, repo_id: RepositoryId) -> Result<RepoGroup> {
        let config = self.config_handle_for_current_version.get();
        let group = config.repos.values().find(|config| {
            // Is large repo?
            config.large_repo_id == repo_id.id()
                // Is small repo?
                    || config
                        .small_repos
                        .iter()
                        .any(|small| small.repoid == repo_id.id())
        });
        if let Some(group) = group {
            Ok(RepoGroup::Megarepo {
                large: RepositoryId::new(group.large_repo_id),
                small: group
                    .small_repos
                    .iter()
                    .map(|small| RepositoryId::new(small.repoid))
                    .collect::<Vec<_>>()
                    .try_into()?,
            })
        } else {
            Ok(RepoGroup::Single(repo_id))
        }
    }
}

impl RepoGroup {
    /// If this repository group is a megarepo group and it has a small repo which has the
    /// pushredirection config set, but that config is set to false, it means we need to
    /// be extra careful that the changes we're doing will not lead to repos diverging.
    pub fn small_repos_with_pushredirection_disabled(
        &self,
        config: &CfgrCurrentCommitSyncConfig,
    ) -> Option<Vec1<RepositoryId>> {
        let mut repos = vec![];
        if let RepoGroup::Megarepo { large: _, small } = self {
            let config = config.config_handle_for_push_redirection.get();
            for small_repo in small {
                if let Some(state) = config.per_repo.get(&(small_repo.id() as i64)) {
                    if !state.public_push {
                        repos.push(small_repo.clone());
                    }
                }
            }
        }
        Vec1::try_from(repos).ok()
    }

    pub fn into_vec(self) -> Vec1<RepositoryId> {
        match self {
            Self::Single(repo) => vec1![repo],
            Self::Megarepo { large, mut small } => {
                small.push(large);
                small
            }
        }
    }
}
