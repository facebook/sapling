/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Context;
use async_trait::async_trait;
use executor_lib::RepoShardedProcess;
use executor_lib::RepoShardedProcessExecutor;
use metaconfig_types::ShardedService;
use mononoke_app::MononokeReposManager;
use sharding_ext::RepoShard;
use slog::info;

use crate::Repo;

/// Struct representing the Mononoke Git Server process when sharding by
/// repo.
pub struct MononokeGitServerProcess {
    repos_mgr: Arc<MononokeReposManager<Repo>>,
}

impl MononokeGitServerProcess {
    pub fn new(repos_mgr: Arc<MononokeReposManager<Repo>>) -> Self {
        Self { repos_mgr }
    }
}

#[async_trait]
impl RepoShardedProcess for MononokeGitServerProcess {
    async fn setup(&self, repo: &RepoShard) -> anyhow::Result<Arc<dyn RepoShardedProcessExecutor>> {
        let repo_name = repo.repo_name.as_str();
        let logger = self.repos_mgr.repo_logger(repo_name);
        info!(
            &logger,
            "Setting up repo {} in Mononoke Git Server", repo_name
        );
        // Check if the input repo is already initialized. This can happen if the repo is a
        // shallow-sharded repo, in which case it would already be initialized during service startup.
        if self.repos_mgr.repos().get_by_name(repo_name).is_none() {
            // The input repo is a deep-sharded repo, so it needs to be added now.
            self.repos_mgr.add_repo(repo_name).await.with_context(|| {
                format!(
                    "Failure in setting up repo {} in Mononoke Git Server",
                    repo_name
                )
            })?;
            info!(
                &logger,
                "Completed repo {} setup in Mononoke Git Server", repo_name
            );
        } else {
            info!(
                &logger,
                "Repo {} is already setup in Mononoke Git Server", repo_name
            );
        }
        Ok(Arc::new(MononokeGitServerExecutor {
            repo_name: repo_name.to_string(),
            repos_mgr: self.repos_mgr.clone(),
        }))
    }
}

/// Struct representing the execution of the Mononoke Git Server for a
/// particular repo when sharding by repo.
pub struct MononokeGitServerExecutor {
    repo_name: String,
    repos_mgr: Arc<MononokeReposManager<Repo>>,
}

#[async_trait]
impl RepoShardedProcessExecutor for MononokeGitServerExecutor {
    async fn execute(&self) -> anyhow::Result<()> {
        info!(
            self.repos_mgr.logger(),
            "Serving repo {} in Mononoke Git Server", &self.repo_name,
        );
        Ok(())
    }

    async fn stop(&self) -> anyhow::Result<()> {
        let config = self
            .repos_mgr
            .repo_config(&self.repo_name)
            .with_context(|| {
                format!(
                    "Failure in stopping repo {}. The config for repo doesn't exist",
                    &self.repo_name
                )
            })?;
        // Check if the current repo is a deep-sharded or shallow-sharded repo. If the
        // repo is deep-sharded, then remove it since SM wants some other host to serve it.
        // If repo is shallow-sharded, then keep it since regardless of SM sharding, shallow
        // sharded repos need to be present on each host.
        let is_deep_sharded = config
            .deep_sharding_config
            .and_then(|c| c.status.get(&ShardedService::MononokeGitServer).copied())
            .unwrap_or(false);
        if is_deep_sharded {
            self.repos_mgr.remove_repo(&self.repo_name);
            info!(
                self.repos_mgr.logger(),
                "No longer serving repo {} in Mononoke Git Server", &self.repo_name,
            );
        } else {
            info!(
                self.repos_mgr.logger(),
                "Continuing serving repo {} in Mononoke Git Server because it's shallow-sharded.",
                &self.repo_name,
            );
        }
        Ok(())
    }
}
