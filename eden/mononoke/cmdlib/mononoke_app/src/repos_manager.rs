/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::time::Duration;
use std::time::Instant;

#[cfg(fbcode_build)]
use MononokeAppStats_ods3::Instrument_MononokeAppStats;
#[cfg(fbcode_build)]
use MononokeAppStats_ods3_types::MononokeAppStats;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use async_trait::async_trait;
use facet::AsyncBuildable;
use futures::stream;
use futures::stream::AbortHandle;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_retry::retry;
use itertools::Itertools;
use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use metaconfig_types::Redaction;
use metaconfig_types::RepoConfig;
use metaconfig_types::ShardedService;
use mononoke_api::Mononoke;
use mononoke_api::MononokeRepo;
use mononoke_configs::ConfigUpdateReceiver;
use mononoke_configs::MononokeConfigs;
use mononoke_macros::mononoke;
use mononoke_repos::MononokeRepos;
use repo_factory::RepoFactory;
use repo_factory::RepoFactoryBuilder;
use stats::prelude::*;
use tracing::info;

fn repos_manager_concurrency() -> Result<usize> {
    justknobs::get_as::<usize>("scm/mononoke:repos_manager_concurrency", None)
        .context("Failed to read scm/mononoke:repos_manager_concurrency JustKnob")
}

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
        Self::new_with_redaction_disabled(configs, repo_factory, service_name, repo_names, false)
            .await
    }

    pub(crate) async fn new_with_redaction_disabled<Names>(
        configs: Arc<MononokeConfigs>,
        repo_factory: Arc<RepoFactory>,
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
            redaction_disabled,
        };
        mgr.populate_repos(repo_names).await?;
        let update_receiver = MononokeConfigUpdateReceiver::new(
            mgr.repos.clone(),
            mgr.repo_factory.clone(),
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

    pub fn configs(&self) -> Arc<MononokeConfigs> {
        self.configs.clone()
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
            .ok_or_else(|| anyhow!("Couldn't retrieve added repo {}", repo_name))
    }

    /// Remove a repo from the managed repo collection.
    pub fn remove_repo(&self, repo_name: &str) {
        self.repos.remove(repo_name);
    }

    async fn populate_repos<Names>(&self, repo_names: Names) -> Result<()>
    where
        Names: IntoIterator<Item = String>,
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>
            + Send
            + Sync
            + 'static,
    {
        let repo_configs = repo_names
            .into_iter()
            .unique()
            .map(|repo_name| {
                self.repo_config(&repo_name)
                    .map(|repo_config| (repo_name, repo_config))
            })
            .collect::<Result<Vec<_>>>()?;
        let total = repo_configs.len();
        let completed = Arc::new(AtomicUsize::new(0));
        let repos_input = stream::iter(repo_configs)
            .map(|(repo_name, repo_config)| {
                let repo_factory = self.repo_factory.clone();
                let name = repo_name.clone();
                let common_config = self.configs.repo_configs().common.clone();
                let repo_id = repo_config.repoid.id();
                let completed = completed.clone();
                mononoke::spawn_task(async move {
                    let start = Instant::now();
                    info!("Initializing repo: {}", &repo_name);
                    let repo = repo_factory
                        .build(name, repo_config, common_config)
                        .await
                        .with_context(|| format!("Failed to initialize repo '{}'", &repo_name))?;
                    let n = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    info!("Initialized repo: {} ({}/{})", &repo_name, n, total);
                    STATS::initialization_time_millisecs.add_value(
                        start.elapsed().as_millis().try_into().unwrap_or(i64::MAX),
                        (repo_name.to_string(),),
                    );

                    #[cfg(fbcode_build)]
                    let instrument = Instrument_MononokeAppStats::new();
                    #[cfg(fbcode_build)]
                    instrument.observe(MononokeAppStats {
                        repo_name: Some(repo_name.to_string()),
                        initialization_time_millisecs: Some(start.elapsed().as_millis() as f64),
                        ..Default::default()
                    });

                    anyhow::Ok((repo_id, repo_name, repo))
                })
            })
            // Repo construction can be heavy, limit concurrency via JK.
            .buffer_unordered(repos_manager_concurrency()?)
            .map(|r| anyhow::Ok(r??))
            .try_collect::<Vec<_>>()
            .await?;
        self.repos.populate(repos_input);
        Ok(())
    }

    pub fn add_stats_handle_for_repo(&self, repo_name: &str, handle: AbortHandle) {
        self.repos.add_stats_handle_for_repo(repo_name, handle)
    }

    pub fn remove_stats_handle_for_repo(&self, repo_name: &str) {
        self.repos.remove_stats_handle_for_repo(repo_name)
    }
}

impl<R: MononokeRepo> MononokeReposManager<R> {
    pub fn make_mononoke_api(&self) -> Result<Mononoke<R>> {
        let repo_names_in_tier =
            HashMap::from_iter(self.configs.repo_configs().repos.iter().filter_map(
                |(name, config)| {
                    if config.enabled {
                        Some((
                            name.to_string(),
                            config.default_commit_identity_scheme.clone(),
                        ))
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
    service_name: Option<ShardedService>,
}

impl<Repo> MononokeConfigUpdateReceiver<Repo> {
    fn new(
        repos: Arc<MononokeRepos<Repo>>,
        repo_factory: Arc<RepoFactory>,
        service_name: Option<ShardedService>,
    ) -> Self {
        Self {
            repos,
            repo_factory,
            service_name,
        }
    }

    /// Method for determining the set of repos to be reloaded with the new config
    fn reloadable_repo(&self, repo_configs: Arc<RepoConfigs>) -> Vec<(String, RepoConfig)> {
        let mut repos_to_load = vec![];
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
        repos_to_load
    }
}

#[async_trait]
impl<Repo> ConfigUpdateReceiver for MononokeConfigUpdateReceiver<Repo>
where
    Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>
        + Send
        + Sync
        + 'static,
{
    async fn apply_update(
        &self,
        repo_configs: Arc<RepoConfigs>,
        _: Arc<StorageConfigs>,
    ) -> Result<()> {
        let repos_to_load = self.reloadable_repo(repo_configs.clone());
        let total = repos_to_load.len();
        let completed = Arc::new(AtomicUsize::new(0));

        let repos_input = stream::iter(repos_to_load)
            .map(|(repo_name, repo_config)| {
                let repo_factory = self.repo_factory.clone();
                let name = repo_name.clone();
                let common_config = repo_configs.common.clone();
                let repo_id = repo_config.repoid.id();
                let completed = completed.clone();
                mononoke::spawn_task(async move {
                    info!("Reloading repo: {}", &repo_name);
                    let repo = retry(
                        |_| {
                            repo_factory.build(
                                name.clone(),
                                repo_config.clone(),
                                common_config.clone(),
                            )
                        },
                        Duration::from_millis(100),
                    )
                    .binary_exponential_backoff()
                    .max_attempts(5)
                    .await
                    .with_context(|| format!("Failed to reload repo '{}'", &repo_name))?
                    .0;
                    let n = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    info!("Reloaded repo: {} ({}/{})", &repo_name, n, total);

                    anyhow::Ok((repo_id, repo_name, repo))
                })
            })
            // Repo construction can be heavy, limit concurrency via JK.
            .buffer_unordered(repos_manager_concurrency()?)
            .map(|r| anyhow::Ok(r??))
            .try_collect::<Vec<_>>()
            .await?;
        // Ensure that we only add or replace repos and NEVER remove them
        self.repos.reload(repos_input);
        Ok(())
    }
}
