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
use metaconfig_types::Redaction;
use metaconfig_types::RepoConfig;
use metaconfig_types::ShardedService;
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
    redaction_disabled: bool,
}

impl<Repo> MononokeReposManager<Repo> {
    // Create a new `MononokeReposManager`.
    // Unlike `new_with_redaction_disabled`, we don't expose the mechanism to access redacted blobs
    // through this API.
    // This should be your goto constructor for this struct except if you have a specific reason
    // for needing to disable redaction.
    #[allow(unused)]
    pub(crate) async fn new<Names>(
        configs: Arc<MononokeConfigs>,
        repo_factory: Arc<RepoFactory>,
        logger: Logger,
        service_name: Option<ShardedService>,
        repo_names: Names,
    ) -> Result<Self>
    where
        Names: IntoIterator<Item = String>,
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>
            + Send
            + Sync
            + 'static,
    {
        Self::new_with_redaction_disabled(
            configs,
            repo_factory,
            logger,
            service_name,
            repo_names,
            false,
        )
        .await
    }

    pub(crate) async fn new_with_redaction_disabled<Names>(
        configs: Arc<MononokeConfigs>,
        repo_factory: Arc<RepoFactory>,
        logger: Logger,
        service_name: Option<ShardedService>,
        repo_names: Names,
        redaction_disabled: bool,
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
            redaction_disabled,
        };
        mgr.populate_repos(repo_names).await?;
        let update_receiver = MononokeConfigUpdateReceiver::new(
            mgr.repos.clone(),
            mgr.repo_factory.clone(),
            mgr.logger.clone(),
            service_name,
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
        let mut repo_config = self
            .configs
            .repo_configs()
            .repos
            .get(repo_name)
            .cloned()
            .ok_or_else(|| anyhow!("unknown reponame: {:?}", repo_name))?;
        if self.redaction_disabled {
            repo_config.redaction = Redaction::Disabled;
        }
        Ok(repo_config)
    }

    /// Construct and add a new repo to the managed repo collection.
    pub async fn add_repo(&self, repo_name: &str) -> Result<Arc<Repo>>
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
        self.repos
            .get_by_name(repo_name)
            .ok_or_else(|| anyhow!("Couldn't retrive added repo {}", repo_name))
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
    service_name: Option<ShardedService>,
}

impl<Repo> MononokeConfigUpdateReceiver<Repo> {
    fn new(
        repos: Arc<MononokeRepos<Repo>>,
        repo_factory: Arc<RepoFactory>,
        logger: Logger,
        service_name: Option<ShardedService>,
    ) -> Self {
        Self {
            repos,
            repo_factory,
            logger,
            service_name,
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
        let mut repos_to_load = Vec::new();
        for (repo_name, repo_config) in repo_configs.repos.clone().into_iter() {
            if self.repos.get_by_name(repo_name.as_str()).is_some() {
                // Repo was already present on the server. Need to reload it.
                repos_to_load.push((repo_name, repo_config))
            }
            // If the service name is known, then by default we need to reload or add all repos
            // that are in RepoConfig AND are shallow-sharded (i.e. NOT deep-sharded).
            else if repo_config.enabled
                && let Some(ref service_name) = self.service_name
            {
                if let Some(ref config) = repo_config.deep_sharding_config {
                    // Repo is shallow sharded for this service AND enabled, so should be loaded.
                    if !config.status.get(service_name).cloned().unwrap_or(false) {
                        repos_to_load.push((repo_name, repo_config));
                    }
                } else {
                    // Service specific sharding config doesn't exist for repo but the repo is
                    // enabled so should be considered as shallow-sharded.
                    repos_to_load.push((repo_name, repo_config));
                }
            }
            // The repos present on the server but not part of RepoConfigs are ignored by
            // default. This situation can happen when the name of the repo changes
            // (e.g. whatsapp/server.mirror renamed to whatsapp/server) or when a repo is
            // added or removed. In such a case, reloading of the repo with the old name
            // would not be possible based on the new configs.
        }

        let repos_input = stream::iter(repos_to_load)
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
