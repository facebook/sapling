/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
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
use config_reconcile::ConfigSource;
use config_reconcile::DesiredRepo;
use config_reconcile::ManifestEntry;
use config_reconcile::RepoGeneration;
use config_reconcile::RepoManager;
use config_reconcile::RepoState;
use facet::AsyncBuildable;
use futures::stream;
use futures::stream::AbortHandle;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use futures_retry::retry;
use itertools::Itertools;
use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use metaconfig_parser::parse_repo_spec;
use metaconfig_parser::spec_hash;
use metaconfig_parser::storage_generation;
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
use repos::RepoSpec;
use stats::prelude::*;
use tokio::sync::Notify;
use tokio::task::JoinHandle;
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
    reconcile_applied: timeseries(Average, Sum, Count),
    reconcile_dropped: timeseries(Average, Sum, Count),
    reconcile_failed_repos: timeseries(Average, Sum, Count),
    reconcile_tick_duration_ms: timeseries(Average, Sum, Count),
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
    // Holds all state a reconcile pass needs (per-repo state, spec-hash cache,
    // single-flight lock). Shared with the background loop.
    reconcile_driver: Arc<ReconcileDriver<Repo>>,
    // Background reconcile loop; aborted on Drop. None without split-loading.
    reconcile_loop_handle: Option<JoinHandle<()>>,
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
        let reconcile_driver = Arc::new(ReconcileDriver {
            configs: configs.clone(),
            repo_factory: repo_factory.clone(),
            repos: repos.clone(),
            redaction_disabled,
            reconcile_state: Arc::new(ArcSwap::from_pointee(HashMap::new())),
            spec_hash_cache: Arc::new(Mutex::new(HashMap::new())),
            reconcile_lock: Arc::new(tokio::sync::Mutex::new(())),
        });
        let mut mgr = MononokeReposManager {
            repos,
            configs,
            repo_factory,
            redaction_disabled,
            applied_configs: applied_configs.clone(),
            repo_names_in_tier: repo_names_in_tier.clone(),
            reconcile_driver,
            reconcile_loop_handle: None,
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

        // Split-loaded services drive reconcile from a background loop, woken by
        // a receiver on every config change plus a periodic backstop. The loop
        // is unconditional; the killswitch is checked per-pass.
        if mgr.configs.manifest().is_some() {
            let trigger = Arc::new(Notify::new());
            mgr.configs.register_for_update(Arc::new(ReconcileTrigger {
                notify: trigger.clone(),
            }) as Arc<dyn ConfigUpdateReceiver>);
            mgr.reconcile_loop_handle =
                Some(spawn_reconcile_loop(mgr.reconcile_driver.clone(), trigger));
        }

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
        // get_or_load_repo_config (called via repo_config) subscribes the
        // per-repo ConfigHandle internally.
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
            .ok_or_else(|| anyhow!("Couldn't retrieve added repo {repo_name}"))
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
        self.configs.remove_repo_config_handle(repo_name);
    }

    /// Run one reconciliation pass now. Delegates to the driver. No-op unless the
    /// `use_config_reconcile` killswitch is on (read every call).
    pub async fn reconcile(&self) -> Result<()>
    where
        Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>
            + Send
            + Sync
            + 'static,
    {
        self.reconcile_driver.pass().await
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

/// Adapts `MononokeConfigs` to the `config_reconcile::ConfigSource` trait.
struct ReconcileConfigSource {
    configs: Arc<MononokeConfigs>,
    // Memoizes spec_hash keyed by the RepoSpec Arc identity. See ReconcileDriver.
    spec_hash_cache: Arc<Mutex<HashMap<String, (Arc<RepoSpec>, u64)>>>,
}

impl ConfigSource for ReconcileConfigSource {
    fn manifest(&self) -> Vec<ManifestEntry> {
        self.configs.manifest().map_or_else(Vec::new, |m| {
            m.repos
                .iter()
                .map(|e| ManifestEntry {
                    name: e.repo_name.clone(),
                    is_deep_sharded: e.is_deep_sharded,
                })
                .collect()
        })
    }

    fn desired(&self, name: &str) -> Option<DesiredRepo> {
        let spec = self.configs.live_repo_spec(name)?;

        // Reuse the cached hash when the live RepoSpec Arc is pointer-identical to
        // the one we hashed last time (ConfigHandle::get() is pointer-stable while
        // unchanged), else recompute. spec_hash stays the drift signal — pointer
        // identity only decides whether to re-serialize.
        let mut cache = self
            .spec_hash_cache
            .lock()
            .expect("spec_hash_cache poisoned");
        let cached_hash = match cache.get(name) {
            Some((cached_spec, hash)) if Arc::ptr_eq(cached_spec, &spec) => Some(*hash),
            _ => None,
        };
        let spec_hash = match cached_hash {
            Some(hash) => hash,
            None => {
                let hash = spec_hash(&spec).ok()?;
                cache.insert(name.to_string(), (spec.clone(), hash));
                hash
            }
        };
        drop(cache);

        Some(DesiredRepo {
            enabled: spec.enabled,
            spec_hash,
        })
    }

    fn storage_generation(&self) -> Result<u64> {
        let manifest = self
            .configs
            .manifest()
            .context("reconcile: no manifest for storage generation")?;
        storage_generation(&manifest.storage)
    }
}

/// Adapts `MononokeConfigs` + `RepoFactory` + `MononokeRepos` to the
/// `config_reconcile::RepoManager` trait: builds a repo from its live config
/// (async), then inserts it under `MononokeRepos`' update lock (sync).
struct ReconcileRepoManager<Repo> {
    configs: Arc<MononokeConfigs>,
    repo_factory: Arc<RepoFactory>,
    repos: Arc<MononokeRepos<Repo>>,
    tier: String,
    redaction_disabled: bool,
}

#[async_trait]
impl<Repo> RepoManager for ReconcileRepoManager<Repo>
where
    Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>
        + Send
        + Sync
        + 'static,
{
    fn loaded_names(&self) -> HashSet<String> {
        self.repos.iter_names().collect()
    }

    async fn build_and_apply(
        &self,
        name: &str,
        deep: bool,
        storage_gen: u64,
    ) -> Result<Option<RepoGeneration>> {
        let spec = self
            .configs
            .live_repo_spec(name)
            .context("reconcile: no live RepoSpec (unsubscribed or unreadable)")?;
        let spec_hash = spec_hash(&spec)?;
        let manifest = self.configs.manifest().context("reconcile: no manifest")?;
        let mut repo_config =
            parse_repo_spec(Arc::unwrap_or_clone(spec), &self.tier, &manifest.storage)?;
        if self.redaction_disabled {
            repo_config.redaction = Redaction::Disabled;
        }
        let repo_id = repo_config.repoid.id();
        let common_config = self.configs.repo_configs().common.clone();
        let repo = self
            .repo_factory
            .build(name.to_string(), repo_config, common_config)
            .await?;
        // The build (async) is done; the insert below is synchronous and takes
        // the MononokeRepos update lock internally — no lock across an await.
        let applied = if deep {
            self.repos
                .reload_if_present(repo_id, name.to_string(), repo)
        } else {
            self.repos.reload(vec![(repo_id, name.to_string(), repo)]);
            true
        };
        Ok(applied.then_some(RepoGeneration {
            spec_hash,
            storage_gen,
        }))
    }

    fn drop_repo(&self, name: &str) {
        self.repos.remove(name);
        self.configs.remove_repo_config_handle(name);
    }
}

/// Holds all state a reconciliation pass needs and runs one pass via `pass()`.
/// Shared (via `Arc`) between the background loop and the public `reconcile`
/// entry point. Owns the per-repo reconcile state, the spec-hash cache (perf),
/// and the single-flight lock.
struct ReconcileDriver<Repo> {
    configs: Arc<MononokeConfigs>,
    repo_factory: Arc<RepoFactory>,
    repos: Arc<MononokeRepos<Repo>>,
    redaction_disabled: bool,
    // Per-repo reconcile state (loaded generation or last failure), keyed by
    // name. Empty until reconcile runs; driven by the config_reconcile crate.
    reconcile_state: Arc<ArcSwap<HashMap<String, RepoState>>>,
    // Memoizes spec_hash keyed by RepoSpec Arc identity so an unchanged repo is
    // not re-serialized every pass. Stores the Arc itself (not a raw pointer) to
    // pin the allocation, so Arc::ptr_eq can't false-match a reused address (ABA).
    spec_hash_cache: Arc<Mutex<HashMap<String, (Arc<RepoSpec>, u64)>>>,
    // Single-flight guard: only one pass runs at a time.
    reconcile_lock: Arc<tokio::sync::Mutex<()>>,
}

impl<Repo> ReconcileDriver<Repo>
where
    Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>
        + Send
        + Sync
        + 'static,
{
    /// One reconciliation pass, shared by the background loop and `reconcile`.
    /// No-op when `use_config_reconcile` is off or there is no tier/manifest.
    async fn pass(&self) -> Result<()> {
        if !justknobs::eval("scm/mononoke:use_config_reconcile", None, None) {
            return Ok(());
        }
        // Needs a tier + manifest to parse specs and know membership.
        let Some(tier) = self.configs.tier_name().map(str::to_owned) else {
            return Ok(());
        };
        if self.configs.manifest().is_none() {
            return Ok(());
        }

        // Single-flight: skip if a pass is already running. The loop is the only
        // caller today and is sequential, so this can't currently contend; it
        // keeps the pub reconcile() entry safe if ever driven concurrently. The
        // tokio Mutex is await-safe, so the guard is held across the whole pass.
        let Ok(_guard) = self.reconcile_lock.try_lock() else {
            return Ok(());
        };

        let config = ReconcileConfigSource {
            configs: self.configs.clone(),
            spec_hash_cache: self.spec_hash_cache.clone(),
        };
        let manager = ReconcileRepoManager {
            configs: self.configs.clone(),
            repo_factory: self.repo_factory.clone(),
            repos: self.repos.clone(),
            tier,
            redaction_disabled: self.redaction_disabled,
        };
        let current = self.reconcile_state.load_full();
        let outcome =
            config_reconcile::reconcile(&config, &manager, &current, repos_manager_concurrency()?)
                .await?;

        if outcome.built + outcome.rebuilt + outcome.dropped > 0 || !outcome.failed.is_empty() {
            info!(
                "reconcile: built={} rebuilt={} dropped={} failed={}",
                outcome.built,
                outcome.rebuilt,
                outcome.dropped,
                outcome.failed.len(),
            );
        }
        STATS::reconcile_applied.add_value((outcome.built + outcome.rebuilt) as i64);
        STATS::reconcile_dropped.add_value(outcome.dropped as i64);
        STATS::reconcile_failed_repos.add_value(outcome.failed.len() as i64);
        self.reconcile_state.store(Arc::new(outcome.next_state));

        // Evict spec-hash cache entries for repos no longer in the manifest. Done
        // here (once per pass) because ConfigSource has no end-of-pass hook. The
        // std Mutex is acquired after all awaits and not held across one.
        if let Some(manifest) = self.configs.manifest() {
            let live: HashSet<&str> = manifest
                .repos
                .iter()
                .map(|e| e.repo_name.as_str())
                .collect();
            let mut cache = self
                .spec_hash_cache
                .lock()
                .expect("spec_hash_cache poisoned");
            cache.retain(|name, _| live.contains(name.as_str()));
        }

        Ok(())
    }
}

/// Interval policy (pure): fixed 60s backstop when off; the tunable value floored
/// at 1s when on, so a 0 can't spin the loop.
fn tick_interval_secs(reconcile_on: bool, knob_secs: u64) -> u64 {
    if reconcile_on { knob_secs.max(1) } else { 60 }
}

/// Backstop interval. Only reads the tunable knob when reconcile is on (it may be
/// unregistered otherwise, and missing-knob reads are expensive).
fn reconcile_tick_interval() -> Duration {
    let on = justknobs::eval("scm/mononoke:use_config_reconcile", None, None);
    let knob_secs = if on {
        justknobs::get_as::<u64>("scm/mononoke:config_reconcile_tick_interval_secs", None)
    } else {
        0
    };
    Duration::from_secs(tick_interval_secs(on, knob_secs))
}

/// Spawn the background reconcile loop. Owns an Arc to the driver (task is
/// `'static`); aborted on Drop. Runs one pass immediately, then again on each
/// wake — a config-change trigger or the backstop.
fn spawn_reconcile_loop<Repo>(
    driver: Arc<ReconcileDriver<Repo>>,
    trigger: Arc<Notify>,
) -> JoinHandle<()>
where
    Repo: for<'builder> AsyncBuildable<'builder, RepoFactoryBuilder<'builder>>
        + Send
        + Sync
        + 'static,
{
    mononoke::spawn_task(async move {
        loop {
            let start = Instant::now();
            if let Err(e) = driver.pass().await {
                warn!("reconcile pass failed: {e:#}");
            }
            STATS::reconcile_tick_duration_ms
                .add_value(start.elapsed().as_millis().try_into().unwrap_or(i64::MAX));
            let period = reconcile_tick_interval();
            tokio::select! {
                _ = trigger.notified() => {}
                _ = tokio::time::sleep(period) => {}
            }
        }
    })
}

/// A `ConfigUpdateReceiver` that wakes the reconcile loop on any config change
/// (bulk or per-repo). `notify_one` coalesces changes into one wake.
struct ReconcileTrigger {
    notify: Arc<Notify>,
}

#[async_trait]
impl ConfigUpdateReceiver for ReconcileTrigger {
    async fn apply_update(
        &self,
        _repo_configs: Arc<RepoConfigs>,
        _storage_configs: Arc<StorageConfigs>,
    ) -> Result<()> {
        self.notify.notify_one();
        Ok(())
    }

    async fn apply_repo_update(&self, _repo_name: &str, _repo_config: &RepoConfig) -> Result<()> {
        self.notify.notify_one();
        Ok(())
    }
}

impl<Repo> Drop for MononokeReposManager<Repo> {
    fn drop(&mut self) {
        // Stop the loop; otherwise it would run forever on a torn-down manager.
        if let Some(handle) = self.reconcile_loop_handle.as_ref() {
            handle.abort();
        }
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
///
/// This is the edge-triggered reload path. While use_config_reconcile is on it
/// coexists with the reconcile loop (both may rebuild a repo on one change);
/// retiring it is the reconcile cutover.
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

/// Whether a single-repo config update should rebuild the repo on this host.
/// Skips disabled repos and repos not currently served here — per-repo watchers
/// fire for every manifest repo, but a host only builds its own shard's repos
/// (a newly-assigned shard is built by `add_repo`).
fn should_reload_single_repo(enabled: bool, currently_served: bool) -> bool {
    enabled && currently_served
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

        // Skip disabled repos, and repos not served on this host: an unserved
        // repo has no applied_configs entry, so it would fall through the dedup
        // below and rebuild a repo we don't serve. Mirrors compute_reloadable_repos.
        if !should_reload_single_repo(
            repo_config.enabled,
            self.repos.get_by_name(repo_name).is_some(),
        ) {
            debug!(
                "Skipping single-repo reload for {} (disabled or unserved)",
                repo_name
            );
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
        .with_context(|| format!("Failed to reload repo '{repo_name}'"))?
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
mod tests;
