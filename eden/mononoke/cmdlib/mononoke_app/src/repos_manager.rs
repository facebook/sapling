/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use facet::AsyncBuildable;
use futures::stream;
use futures::stream::StreamExt;
use itertools::Itertools;
use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use metaconfig_types::RepoConfig;
use mononoke_api::Mononoke;
use mononoke_configs::ConfigUpdateReceiver;
use mononoke_configs::MononokeConfigs;
use mononoke_repos::MononokeRepos;
use repo_factory::RepoFactory;
use repo_factory::RepoFactoryBuilder;
use slog::info;
use slog::o;
use slog::Logger;
use stats::prelude::*;

define_stats! {
    prefix = "mononoke.app";
    initialization_time_millisecs: dynamic_timeseries(
        "initialization_time_millisecs.{}",
        (reponame: String);
        Average, Sum, Count
    ),
    completion_duration_secs: timeseries(Average, Sum, Count),
}

/// A manager of a MononokeRepos collection.
///
/// This allows repos to be added or removed from the MononokeRepos
/// collection.
pub struct MononokeReposManager<Repo> {
    repos: Arc<MononokeRepos<Repo>>,
    configs: Arc<MononokeConfigs>,
    repo_factory: Arc<RepoFactory>,
    logger: Logger,
}

impl<Repo> MononokeReposManager<Repo> {
    pub(crate) async fn new<Names>(
        configs: Arc<MononokeConfigs>,
        repo_factory: Arc<RepoFactory>,
        logger: Logger,
        repo_names: Names,
    ) -> Result<Self>
    where
        Names: IntoIterator<Item = String>,
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>
            + Send
            + Sync
            + 'static,
    {
        let repos = Arc::new(MononokeRepos::new());
        let mgr = MononokeReposManager {
            repos,
            configs,
            repo_factory,
            logger,
        };
        mgr.populate_repos(repo_names).await?;
        let update_receiver = MononokeConfigUpdateReceiver::new(
            mgr.repos.clone(),
            mgr.repo_factory.clone(),
            mgr.logger.clone(),
        );
        mgr.configs
            .register_for_update(Arc::new(update_receiver) as Arc<dyn ConfigUpdateReceiver>);
        Ok(mgr)
    }

    /// The repo collection that is being managed.
    pub fn repos(&self) -> &Arc<MononokeRepos<Repo>> {
        &self.repos
    }

    /// The logger for the app.
    pub fn logger(&self) -> &Logger {
        &self.logger
    }

    /// Construct a logger for a specific repo.
    pub fn repo_logger(&self, repo_name: &str) -> Logger {
        self.logger.new(o!("repo" => repo_name.to_string()))
    }

    /// Return a repo config for a named repo.  This reads from the main
    /// configuration, so doesn't need to be a currently managed repo.
    pub fn repo_config(&self, repo_name: &str) -> Result<RepoConfig> {
        self.configs
            .repo_configs()
            .repos
            .get(repo_name)
            .cloned()
            .ok_or_else(|| anyhow!("unknown reponame: {:?}", repo_name))
    }

    /// Construct and add a new repo to the managed repo collection.
    pub async fn add_repo(&self, repo_name: &str) -> Result<()>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
    {
        let repo_config = self.repo_config(repo_name)?;
        let repo_id = repo_config.repoid.id();
        let common_config = self.configs.repo_configs().common.clone();
        let repo = self
            .repo_factory
            .build(repo_name.to_string(), repo_config, common_config)
            .await?;
        self.repos.add(repo_name, repo_id, repo);
        Ok(())
    }

    /// Remove a repo from the managed repo collection.
    pub fn remove_repo(&self, repo_name: &str) {
        self.repos.remove(repo_name);
    }

    async fn populate_repos<Names>(&self, repo_names: Names) -> Result<()>
    where
        Names: IntoIterator<Item = String>,
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>,
    {
        let repos_input = stream::iter(repo_names.into_iter().unique())
            .map(|repo_name| {
                let repo_factory = self.repo_factory.clone();
                let name = repo_name.clone();
                async move {
                    let start = Instant::now();
                    let logger = self.logger();
                    let repo_config = self.repo_config(&repo_name)?;
                    let common_config = self.configs.repo_configs().common.clone();
                    let repo_id = repo_config.repoid.id();
                    info!(logger, "Initializing repo: {}", &repo_name);
                    let repo = repo_factory
                        .build(name, repo_config, common_config)
                        .await
                        .with_context(|| format!("Failed to initialize repo '{}'", &repo_name))?;
                    info!(logger, "Initialized repo: {}", &repo_name);
                    STATS::initialization_time_millisecs.add_value(
                        start.elapsed().as_millis().try_into().unwrap_or(i64::MAX),
                        (repo_name.to_string(),),
                    );
                    anyhow::Ok((repo_id, repo_name, repo))
                }
            })
            // Repo construction can be heavy, 30 at a time is sufficient.
            .buffered(30)
            .collect::<Vec<_>>();
        // There are lots of deep FuturesUnordered here that have caused inefficient polling with
        // Tokio coop in the past.
        let repos_input = tokio::task::unconstrained(repos_input)
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        self.repos.populate(repos_input);
        Ok(())
    }
}

// This has a concrete type until we make `mononoke_api::Mononoke` generic
// also.
impl MononokeReposManager<mononoke_api::Repo> {
    pub fn make_mononoke_api(&self) -> Result<Mononoke> {
        let repo_names_in_tier =
            Vec::from_iter(self.configs.repo_configs().repos.iter().filter_map(
                |(name, config)| {
                    if config.enabled {
                        Some(name.to_string())
                    } else {
                        None
                    }
                },
            ));
        Mononoke::new(self.repos.clone(), repo_names_in_tier)
    }
}

/// Struct responsible for receiving updated configurations from MononokeConfigs
/// and refreshing repos (and related entities) based on the update.
pub struct MononokeConfigUpdateReceiver<Repo> {
    repos: Arc<MononokeRepos<Repo>>,
    repo_factory: Arc<RepoFactory>,
    logger: Logger,
}

impl<Repo> MononokeConfigUpdateReceiver<Repo> {
    fn new(
        repos: Arc<MononokeRepos<Repo>>,
        repo_factory: Arc<RepoFactory>,
        logger: Logger,
    ) -> Self {
        Self {
            repos,
            repo_factory,
            logger,
        }
    }
}

#[async_trait]
impl<Repo> ConfigUpdateReceiver for MononokeConfigUpdateReceiver<Repo>
where
    Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>> + Send + Sync,
{
    async fn apply_update(
        &self,
        repo_configs: Arc<RepoConfigs>,
        _: Arc<StorageConfigs>,
    ) -> Result<()> {
        // We need to filter out the name of repos that are present in MononokeRepos (i.e.
        // currently served by the server) but not in RepoConfigs. This situation can happen
        // when the name of the repo changes (e.g. whatsapp/server.mirror renamed to whatsapp/server)
        // or when a repo is added or removed. In such a case, reloading of the repo with the old name
        // would not be possible based on the new configs.
        let repos_input = stream::iter(self.repos.iter_names().filter_map(|repo_name| {
            repo_configs
                .repos
                .get(&repo_name)
                .cloned()
                .map(|repo_config| (repo_name, repo_config))
        }))
        .map(|(repo_name, repo_config)| {
            let repo_factory = self.repo_factory.clone();
            let name = repo_name.clone();
            let logger = self.logger.clone();
            let common_config = repo_configs.common.clone();
            async move {
                let repo_id = repo_config.repoid.id();
                info!(logger, "Reloading repo: {}", &repo_name);
                let repo = repo_factory
                    .build(name, repo_config, common_config)
                    .await
                    .with_context(|| format!("Failed to reload repo '{}'", &repo_name))?;
                info!(logger, "Reloaded repo: {}", &repo_name);

                anyhow::Ok((repo_id, repo_name, repo))
            }
        })
        // Repo construction can be heavy, 30 at a time is sufficient.
        .buffered(30)
        .collect::<Vec<_>>();
        // There are lots of deep FuturesUnordered here that have caused inefficient polling with
        // Tokio coop in the past.
        let repos_input = tokio::task::unconstrained(repos_input)
            .await
            .into_iter()
            .collect::<Result<Vec<_>>>()?;
        self.repos.populate(repos_input);
        Ok(())
    }
}
