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
use arc_swap::ArcSwap;
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
use metaconfig_types::CommitIdentityScheme;
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
use tracing::debug;
use tracing::info;
use tracing::warn;

fn repos_manager_concurrency() -> Result<usize> {
    Ok(justknobs::get_as::<usize>(
        "scm/mononoke:repos_manager_concurrency",
        None,
    ))
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
    // Tracks the RepoConfig last applied to each managed repo. Used to skip
    // redundant per-repo reloads when a tier-manifest content change does not
    // change a given repo's config (the common case when a sibling repo is
    // added or modified).
    applied_configs: Arc<ArcSwap<HashMap<String, RepoConfig>>>,
    // Tier-wide list of enabled repos (name -> default identity scheme).
    // Shared with Mononoke<R> (read by list_repos) and with
    // MononokeConfigUpdateReceiver (which refreshes it on each config update).
    repo_names_in_tier: Arc<ArcSwap<HashMap<String, CommitIdentityScheme>>>,
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
        let applied_configs = Arc::new(ArcSwap::from_pointee(HashMap::new()));
        let repo_names_in_tier = Arc::new(ArcSwap::from_pointee(HashMap::new()));
        let mgr = MononokeReposManager {
            repos,
            configs,
            repo_factory,
            redaction_disabled,
            applied_configs: applied_configs.clone(),
            repo_names_in_tier: repo_names_in_tier.clone(),
        };
        mgr.populate_repos(repo_names).await?;
        let update_receiver = MononokeConfigUpdateReceiver::new(
            mgr.repos.clone(),
            mgr.repo_factory.clone(),
            service_name,
            mgr.configs.clone(),
            applied_configs,
            repo_names_in_tier,
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
        let mut repo_config = self.configs.get_or_load_repo_config(repo_name)?;
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
        // get_or_load_repo_config (called via repo_config) handles
        // ConfigHandle subscription internally — no separate
        // load_repo_config_handle call needed.
        let repo_config = self.repo_config(repo_name)?;
        let repo_id = repo_config.repoid.id();
        let common_config = self.configs.repo_configs().common.clone();
        let tracked_config = repo_config.clone();
        let repo = self
            .repo_factory
            .build(repo_name.to_string(), repo_config, common_config)
            .await?;
        self.repos.add(repo_name, repo_id, repo);
        self.record_applied_configs(std::iter::once((repo_name.to_string(), tracked_config)));
        self.repos
            .get_by_name(repo_name)
            .ok_or_else(|| anyhow!("Couldn't retrieve added repo {}", repo_name))
    }

    /// Merge the given (repo_name, RepoConfig) entries into the applied-config
    /// cache. This is the source of truth for "which config is currently active
    /// in MononokeRepos for each repo" and drives per-repo reload dedup in
    /// MononokeConfigUpdateReceiver.
    fn record_applied_configs<I>(&self, entries: I)
    where
        I: IntoIterator<Item = (String, RepoConfig)>,
    {
        let mut new_applied = (**self.applied_configs.load()).clone();
        new_applied.extend(entries);
        self.applied_configs.store(Arc::new(new_applied));
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
        let tracked_configs: Vec<(String, RepoConfig)> = repo_configs
            .iter()
            .map(|(name, config)| (name.clone(), config.clone()))
            .collect();
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
        self.record_applied_configs(tracked_configs);
        Ok(())
    }

    pub fn add_stats_handle_for_repo(&self, repo_name: &str, handle: AbortHandle) {
        self.repos.add_stats_handle_for_repo(repo_name, handle)
    }

    pub fn remove_stats_handle_for_repo(&self, repo_name: &str) {
        self.repos.remove_stats_handle_for_repo(repo_name)
    }
}

impl<R> MononokeReposManager<R> {
    pub fn make_mononoke_api(&self) -> Result<Mononoke<R>> {
        // Note: the watcher receiver is already registered by the time we
        // run, so in principle a configerator update fired between
        // registration and this call could land a fresher snapshot that
        // this store() overwrites. In practice make_mononoke_api runs
        // milliseconds after registration during startup, before any
        // notification is plausible; subsequent apply_update calls will
        // correct any drift within one config refresh cycle.
        let configs = self.configs.load_all_repo_configs()?;
        self.repo_names_in_tier
            .store(Arc::new(build_repo_names_in_tier(
                configs.iter().map(|(name, config)| (name, config)),
            )));
        Mononoke::new(self.repos.clone(), self.repo_names_in_tier.clone())
    }
}

/// Build the tier-wide (name -> default identity scheme) map from an iterator
/// of borrowed (repo_name, RepoConfig) pairs, dropping disabled repos. Takes
/// borrows to avoid cloning the heavy RepoConfig struct just to read two
/// fields.
fn build_repo_names_in_tier<'a, I>(configs: I) -> HashMap<String, CommitIdentityScheme>
where
    I: IntoIterator<Item = (&'a String, &'a RepoConfig)>,
{
    configs
        .into_iter()
        .filter(|(_, config)| config.enabled)
        .map(|(name, config)| (name.clone(), config.default_commit_identity_scheme.clone()))
        .collect()
}

/// Struct responsible for receiving updated configurations from MononokeConfigs
/// and refreshing repos (and related entities) based on the update.
pub struct MononokeConfigUpdateReceiver<Repo> {
    repos: Arc<MononokeRepos<Repo>>,
    repo_factory: Arc<RepoFactory>,
    service_name: Option<ShardedService>,
    mononoke_configs: Arc<MononokeConfigs>,
    // Shared with the owning MononokeReposManager. See MononokeReposManager.
    applied_configs: Arc<ArcSwap<HashMap<String, RepoConfig>>>,
    // Shared with MononokeReposManager and Mononoke<R>. Updated on every
    // config change so `list_repos` sees newly-added repos without waiting
    // for a process restart.
    repo_names_in_tier: Arc<ArcSwap<HashMap<String, CommitIdentityScheme>>>,
}

/// Determines which repos should be loaded/reloaded based on config.
///
/// A repo should be loaded if:
/// 1. It already exists on the server (always reload to pick up config changes), OR
/// 2. It's a new repo that is:
///    - enabled in config, AND
///    - either no service_name is configured, OR
///    - the repo is shallow-sharded for the given service (not deep-sharded)
fn compute_reloadable_repos<F>(
    repo_configs: &RepoConfigs,
    service_name: Option<&ShardedService>,
    repo_exists: F,
) -> Vec<(String, RepoConfig)>
where
    F: Fn(&str) -> bool,
{
    let mut repos_to_load = vec![];
    for (repo_name, repo_config) in repo_configs.repos.clone().into_iter() {
        if repo_exists(repo_name.as_str()) {
            // Repo was already present on the server. Need to reload it.
            repos_to_load.push((repo_name, repo_config))
        }
        // Only reload repos that are enabled in config
        else if repo_config.enabled {
            match (service_name, &repo_config.deep_sharding_config) {
                (Some(service_name), Some(config)) => {
                    // Service name is provided AND Repo is shallow sharded for this service, so should be loaded.
                    if !config.status.get(service_name).cloned().unwrap_or(false) {
                        repos_to_load.push((repo_name, repo_config));
                    }
                }
                (Some(_), None) => {
                    // Service name is provided but sharding config doesn't exist for repo. In this case it should
                    // be considered as shallow-sharded.
                    repos_to_load.push((repo_name, repo_config));
                }
                (None, _) => {
                    // Service name is not provided so regardless of whether the sharding config
                    // exists or not, the repo should be considered as shallow-sharded.
                    repos_to_load.push((repo_name, repo_config));
                }
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

/// Filter a list of reload candidates down to only those whose `RepoConfig`
/// actually differs from the previously-applied config. A candidate not present
/// in `applied` is treated as never-loaded and passed through.
///
/// This avoids the cost of rebuilding repos whose config did not change — the
/// common case when a tier manifest content-hash bumps due to an unrelated repo
/// being added or modified.
fn filter_repos_with_changed_config(
    candidates: Vec<(String, RepoConfig)>,
    applied: &HashMap<String, RepoConfig>,
) -> Vec<(String, RepoConfig)> {
    candidates
        .into_iter()
        .filter(|(name, new_config)| match applied.get(name) {
            Some(existing) => existing != new_config,
            None => true,
        })
        .collect()
}

impl<Repo> MononokeConfigUpdateReceiver<Repo> {
    fn new(
        repos: Arc<MononokeRepos<Repo>>,
        repo_factory: Arc<RepoFactory>,
        service_name: Option<ShardedService>,
        mononoke_configs: Arc<MononokeConfigs>,
        applied_configs: Arc<ArcSwap<HashMap<String, RepoConfig>>>,
        repo_names_in_tier: Arc<ArcSwap<HashMap<String, CommitIdentityScheme>>>,
    ) -> Self {
        Self {
            repos,
            repo_factory,
            service_name,
            mononoke_configs,
            applied_configs,
            repo_names_in_tier,
        }
    }

    /// Rebuild the tier-wide repo names map from `repo_configs` (the full
    /// tier config, not the per-task subset) and atomically swap it in.
    fn refresh_repo_names_in_tier(&self, repo_configs: &RepoConfigs) {
        let names = build_repo_names_in_tier(repo_configs.repos.iter());
        self.repo_names_in_tier.store(Arc::new(names));
    }

    /// Merge the given (repo_name, RepoConfig) entries into the applied-config
    /// cache after a successful reload.
    fn record_applied_configs<I>(&self, entries: I)
    where
        I: IntoIterator<Item = (String, RepoConfig)>,
    {
        let mut new_applied = (**self.applied_configs.load()).clone();
        new_applied.extend(entries);
        self.applied_configs.store(Arc::new(new_applied));
    }

    /// Method for determining the set of repos to be reloaded with the new config
    fn reloadable_repo(&self, repo_configs: Arc<RepoConfigs>) -> Vec<(String, RepoConfig)> {
        // Check if manifest has repos not yet in repo_configs
        let manifest = self.mononoke_configs.manifest();
        let has_new_manifest_repos = manifest.as_ref().is_some_and(|m| {
            m.repos
                .iter()
                .any(|e| !repo_configs.repos.contains_key(&e.repo_name))
        });

        if !has_new_manifest_repos {
            // Common case: no new manifest repos, avoid cloning
            return compute_reloadable_repos(&repo_configs, self.service_name.as_ref(), |name| {
                self.repos.get_by_name(name).is_some()
            });
        }

        // Clone and enrich with manifest repos
        let mut enriched = (*repo_configs).clone();
        if let Some(manifest) = manifest {
            for entry in &manifest.repos {
                if !enriched.repos.contains_key(&entry.repo_name) {
                    match self
                        .mononoke_configs
                        .get_or_load_repo_config(&entry.repo_name)
                    {
                        Ok(config) => {
                            enriched.insert_repo(entry.repo_name.clone(), config);
                        }
                        Err(e) => {
                            warn!(
                                "reloadable_repo: failed to load manifest repo {}: {:#}",
                                entry.repo_name, e
                            );
                        }
                    }
                }
            }
        }
        compute_reloadable_repos(&enriched, self.service_name.as_ref(), |name| {
            self.repos.get_by_name(name).is_some()
        })
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
        // Refresh the tier-wide names list first so `list_repos` reflects the
        // latest tier config independent of (and not blocked by) the heavy
        // per-task repo rebuild below.
        self.refresh_repo_names_in_tier(&repo_configs);

        let candidates = self.reloadable_repo(repo_configs.clone());
        let candidate_count = candidates.len();
        let applied_snapshot = self.applied_configs.load_full();
        let repos_to_load = filter_repos_with_changed_config(candidates, &applied_snapshot);
        let suppressed = candidate_count - repos_to_load.len();
        if suppressed > 0 {
            info!(
                "Skipping reload of {} repos with unchanged config (reloading {})",
                suppressed,
                repos_to_load.len(),
            );
        }
        if repos_to_load.is_empty() {
            return Ok(());
        }
        let tracked_configs: Vec<(String, RepoConfig)> = repos_to_load
            .iter()
            .map(|(name, config)| (name.clone(), config.clone()))
            .collect();

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
        self.record_applied_configs(tracked_configs);
        Ok(())
    }

    async fn apply_repo_update(&self, repo_name: &str, repo_config: &RepoConfig) -> Result<()> {
        // Surgically patch the tier-wide names map from the passed-in
        // arg, not from self.mononoke_configs.repo_configs(). The arg is
        // authoritative for THIS repo; self.mononoke_configs depends on
        // an ordering invariant (the caller must have already swapped in
        // the new config) which isn't documented on the trait. Using the
        // arg eliminates that coupling.
        //
        // rcu() makes the load-mutate-store atomic against concurrent writers:
        // if anything else (another apply_repo_update, or apply_update's bulk
        // refresh) stores during the closure, the CAS fails and the closure
        // re-runs on the fresher snapshot. Idempotent for our patch shape.
        self.repo_names_in_tier.rcu(|current| {
            let mut snapshot = (**current).clone();
            if repo_config.enabled {
                snapshot.insert(
                    repo_name.to_string(),
                    repo_config.default_commit_identity_scheme.clone(),
                );
            } else {
                snapshot.remove(repo_name);
            }
            Arc::new(snapshot)
        });

        // Skip disabled or non-reloadable repos
        if !repo_config.enabled {
            return Ok(());
        }

        // Skip if the config has not actually changed since the last apply.
        if let Some(existing) = self.applied_configs.load().get(repo_name) {
            if existing == repo_config {
                debug!(
                    "Skipping single-repo reload for {} (config unchanged)",
                    repo_name,
                );
                return Ok(());
            }
        }

        // Get the common config from the current repo_configs
        let common_config = self.mononoke_configs.repo_configs().common.clone();

        let repo_id = repo_config.repoid.id();
        info!("Reloading single repo config: {}", repo_name);

        let repo = retry(
            |_| {
                self.repo_factory.build(
                    repo_name.to_string(),
                    repo_config.clone(),
                    common_config.clone(),
                )
            },
            Duration::from_millis(100),
        )
        .binary_exponential_backoff()
        .max_attempts(5)
        .await
        .with_context(|| format!("Failed to reload repo '{}'", repo_name))?
        .0;

        info!("Reloaded single repo: {}", repo_name);
        self.repos
            .reload(vec![(repo_id, repo_name.to_string(), repo)]);
        self.record_applied_configs(std::iter::once((
            repo_name.to_string(),
            repo_config.clone(),
        )));
        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;
    use std::collections::HashSet;

    use metaconfig_parser::RepoConfigs;
    use metaconfig_types::CommonConfig;
    use metaconfig_types::RepoConfig;
    use metaconfig_types::ShardedService;
    use metaconfig_types::ShardingModeConfig;
    use mononoke_macros::mononoke;

    use super::compute_reloadable_repos;
    use super::filter_repos_with_changed_config;

    /// Helper to create a RepoConfig with the specified enabled state and sharding config
    fn make_repo_config(
        enabled: bool,
        deep_sharding_config: Option<ShardingModeConfig>,
    ) -> RepoConfig {
        RepoConfig {
            enabled,
            deep_sharding_config,
            ..Default::default()
        }
    }

    /// Helper to create a ShardingModeConfig with the given service marked as deep-sharded or not
    fn make_sharding_config(service: ShardedService, is_deep_sharded: bool) -> ShardingModeConfig {
        let mut status = HashMap::new();
        status.insert(service, is_deep_sharded);
        ShardingModeConfig { status }
    }

    /// Helper to create RepoConfigs from a list of (name, config) pairs
    fn make_repo_configs(repos: Vec<(String, RepoConfig)>) -> RepoConfigs {
        RepoConfigs::new(repos.into_iter().collect(), CommonConfig::default())
    }

    /// Helper to get repo names from result
    fn get_repo_names(result: &[(String, RepoConfig)]) -> Vec<&str> {
        let mut names: Vec<_> = result.iter().map(|(name, _)| name.as_str()).collect();
        names.sort();
        names
    }

    /// Helper to create a repo_exists function from a set of existing repo names
    fn existing_repos(names: &[&str]) -> impl Fn(&str) -> bool {
        let set: HashSet<String> = names.iter().map(|s| s.to_string()).collect();
        move |name: &str| set.contains(name)
    }

    #[mononoke::test]
    fn test_existing_repo_always_reloaded() {
        // Repos already present on the server should always be reloaded,
        // regardless of service_name or deep_sharding_config
        let repo_configs = make_repo_configs(vec![(
            "existing_repo".to_string(),
            make_repo_config(true, None),
        )]);

        let result =
            compute_reloadable_repos(&repo_configs, None, existing_repos(&["existing_repo"]));
        assert_eq!(get_repo_names(&result), vec!["existing_repo"]);
    }

    #[mononoke::test]
    fn test_existing_disabled_repo_still_reloaded() {
        // Even disabled repos should be reloaded if they're already on the server
        let repo_configs = make_repo_configs(vec![(
            "existing_repo".to_string(),
            make_repo_config(false, None),
        )]);

        let result =
            compute_reloadable_repos(&repo_configs, None, existing_repos(&["existing_repo"]));
        assert_eq!(get_repo_names(&result), vec!["existing_repo"]);
    }

    #[mononoke::test]
    fn test_new_repo_no_service_name() {
        // New repos should be loaded when no service_name is provided
        // This is the key bug fix: previously these repos were not loaded
        let repo_configs =
            make_repo_configs(vec![("new_repo".to_string(), make_repo_config(true, None))]);

        let result = compute_reloadable_repos(&repo_configs, None, existing_repos(&[]));
        assert_eq!(get_repo_names(&result), vec!["new_repo"]);
    }

    #[mononoke::test]
    fn test_new_repo_no_service_name_with_sharding_config() {
        // New repos with sharding config should still be loaded when no service_name is provided
        let sharding_config = make_sharding_config(ShardedService::SaplingRemoteApi, true);
        let repo_configs = make_repo_configs(vec![(
            "new_repo".to_string(),
            make_repo_config(true, Some(sharding_config)),
        )]);

        let result = compute_reloadable_repos(&repo_configs, None, existing_repos(&[]));
        assert_eq!(get_repo_names(&result), vec!["new_repo"]);
    }

    #[mononoke::test]
    fn test_new_repo_with_service_name_no_sharding_config() {
        // New repos without sharding config should be loaded (shallow-sharded by default)
        let repo_configs =
            make_repo_configs(vec![("new_repo".to_string(), make_repo_config(true, None))]);

        let result = compute_reloadable_repos(
            &repo_configs,
            Some(&ShardedService::SaplingRemoteApi),
            existing_repos(&[]),
        );
        assert_eq!(get_repo_names(&result), vec!["new_repo"]);
    }

    #[mononoke::test]
    fn test_new_repo_shallow_sharded_for_service() {
        // New repos explicitly marked as shallow-sharded (false) should be loaded
        let sharding_config = make_sharding_config(ShardedService::SaplingRemoteApi, false);
        let repo_configs = make_repo_configs(vec![(
            "new_repo".to_string(),
            make_repo_config(true, Some(sharding_config)),
        )]);

        let result = compute_reloadable_repos(
            &repo_configs,
            Some(&ShardedService::SaplingRemoteApi),
            existing_repos(&[]),
        );
        assert_eq!(get_repo_names(&result), vec!["new_repo"]);
    }

    #[mononoke::test]
    fn test_new_repo_deep_sharded_for_service() {
        // New repos marked as deep-sharded (true) for the service should NOT be loaded
        let sharding_config = make_sharding_config(ShardedService::SaplingRemoteApi, true);
        let repo_configs = make_repo_configs(vec![(
            "new_repo".to_string(),
            make_repo_config(true, Some(sharding_config)),
        )]);

        let result = compute_reloadable_repos(
            &repo_configs,
            Some(&ShardedService::SaplingRemoteApi),
            existing_repos(&[]),
        );
        assert!(result.is_empty(), "Deep-sharded repos should not be loaded");
    }

    #[mononoke::test]
    fn test_new_repo_deep_sharded_for_different_service() {
        // Repos deep-sharded for a different service should be loaded
        // Repo is deep-sharded for SourceControlService, but we're SaplingRemoteApi
        let sharding_config = make_sharding_config(ShardedService::SourceControlService, true);
        let repo_configs = make_repo_configs(vec![(
            "new_repo".to_string(),
            make_repo_config(true, Some(sharding_config)),
        )]);

        let result = compute_reloadable_repos(
            &repo_configs,
            Some(&ShardedService::SaplingRemoteApi),
            existing_repos(&[]),
        );
        assert_eq!(get_repo_names(&result), vec!["new_repo"]);
    }

    #[mononoke::test]
    fn test_disabled_new_repo_not_loaded() {
        // Disabled new repos should not be loaded
        let repo_configs = make_repo_configs(vec![(
            "disabled_repo".to_string(),
            make_repo_config(false, None),
        )]);

        let result = compute_reloadable_repos(&repo_configs, None, existing_repos(&[]));
        assert!(result.is_empty(), "Disabled new repos should not be loaded");
    }

    #[mononoke::test]
    fn test_mixed_repos() {
        // Test a mix of existing, new, enabled, disabled, and sharded repos
        let deep_sharded = make_sharding_config(ShardedService::SaplingRemoteApi, true);
        let shallow_sharded = make_sharding_config(ShardedService::SaplingRemoteApi, false);

        let repo_configs = make_repo_configs(vec![
            ("existing_enabled".to_string(), make_repo_config(true, None)),
            (
                "existing_disabled".to_string(),
                make_repo_config(false, None),
            ),
            (
                "new_enabled_no_sharding".to_string(),
                make_repo_config(true, None),
            ),
            ("new_disabled".to_string(), make_repo_config(false, None)),
            (
                "new_shallow_sharded".to_string(),
                make_repo_config(true, Some(shallow_sharded)),
            ),
            (
                "new_deep_sharded".to_string(),
                make_repo_config(true, Some(deep_sharded)),
            ),
        ]);

        let result = compute_reloadable_repos(
            &repo_configs,
            Some(&ShardedService::SaplingRemoteApi),
            existing_repos(&["existing_enabled", "existing_disabled"]),
        );
        let names = get_repo_names(&result);

        // Should include: existing repos (both), new enabled repos that are not deep-sharded
        assert!(names.contains(&"existing_enabled"));
        assert!(names.contains(&"existing_disabled"));
        assert!(names.contains(&"new_enabled_no_sharding"));
        assert!(names.contains(&"new_shallow_sharded"));

        // Should NOT include: new disabled repos, new deep-sharded repos
        assert!(!names.contains(&"new_disabled"));
        assert!(!names.contains(&"new_deep_sharded"));
    }

    #[mononoke::test]
    fn test_filter_skips_repo_with_unchanged_config() {
        // Repos whose RepoConfig is byte-identical to the applied config should be
        // filtered out — no reload needed.
        let config = make_repo_config(true, None);
        let candidates = vec![("repo".to_string(), config.clone())];
        let mut applied = HashMap::new();
        applied.insert("repo".to_string(), config);

        let result = filter_repos_with_changed_config(candidates, &applied);
        assert!(
            result.is_empty(),
            "Repo with unchanged config should not be reloaded, got {:?}",
            get_repo_names(&result),
        );
    }

    #[mononoke::test]
    fn test_filter_keeps_repo_with_changed_config() {
        // Repo whose RepoConfig differs from the applied config must be reloaded.
        let old_config = make_repo_config(true, None);
        let new_config = make_repo_config(false, None);
        let candidates = vec![("repo".to_string(), new_config)];
        let mut applied = HashMap::new();
        applied.insert("repo".to_string(), old_config);

        let result = filter_repos_with_changed_config(candidates, &applied);
        assert_eq!(get_repo_names(&result), vec!["repo"]);
    }

    #[mononoke::test]
    fn test_filter_keeps_repo_not_in_applied_map() {
        // A repo absent from the applied map (e.g., never loaded before) must be
        // passed through so it gets loaded.
        let config = make_repo_config(true, None);
        let candidates = vec![("new_repo".to_string(), config)];
        let applied = HashMap::new();

        let result = filter_repos_with_changed_config(candidates, &applied);
        assert_eq!(get_repo_names(&result), vec!["new_repo"]);
    }

    #[mononoke::test]
    fn test_filter_mixed_candidates() {
        // Mix of unchanged, changed, and brand-new repos.
        let config_a = make_repo_config(true, None);
        let config_b = make_repo_config(false, None);

        let candidates = vec![
            ("unchanged".to_string(), config_a.clone()),
            ("changed".to_string(), config_b.clone()),
            ("brand_new".to_string(), config_a.clone()),
        ];
        let mut applied = HashMap::new();
        applied.insert("unchanged".to_string(), config_a);
        applied.insert("changed".to_string(), make_repo_config(true, None));

        let result = filter_repos_with_changed_config(candidates, &applied);
        let names = get_repo_names(&result);
        assert!(!names.contains(&"unchanged"));
        assert!(names.contains(&"changed"));
        assert!(names.contains(&"brand_new"));
    }
}
