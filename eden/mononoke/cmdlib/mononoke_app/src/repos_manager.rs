/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::future::Future;
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
    completion_duration_secs: timeseries(Average, Sum, Count),
    // Deep-shard repo load failures (config load or facet build) via add_repo,
    // the chokepoint every deep-shard load funnels through.
    add_repo_failed: timeseries(Sum, Count),
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
        // Retained for the app.rs open_named_managed_repos API; reconcile derives
        // deep-sharding from the manifest, so the receiver no longer needs it.
        _service_name: Option<ShardedService>,
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
            repo_names_in_tier: repo_names_in_tier.clone(),
            reconcile_driver,
            reconcile_loop_handle: None,
        };
        mgr.populate_repos(repo_names).await?;
        let update_receiver = MononokeConfigUpdateReceiver::new(repo_names_in_tier);
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
        let repo_config = self
            .repo_config(repo_name)
            .inspect_err(|_| STATS::add_repo_failed.add_value(1))?;
        let repo_id = repo_config.repoid.id();
        let common_config = self.configs.repo_configs().common.clone();
        let repo = self
            .repo_factory
            .build(repo_name.to_string(), repo_config, common_config)
            .await
            .inspect_err(|_| STATS::add_repo_failed.add_value(1))?;
        self.repos.add(repo_name, repo_id, repo);
        self.repos
            .get_by_name(repo_name)
            .ok_or_else(|| anyhow!("Couldn't retrieve added repo {repo_name}"))
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
                        .with_context(|| format!("Failed to initialize repo '{repo_name}'"))?;
                    let n = completed.fetch_add(1, Ordering::Relaxed) + 1;
                    info!("Initialized repo: {} ({}/{})", &repo_name, n, total);
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

/// Memoize a per-name hash of an `Arc<T>`. Returns the cached hash only when the
/// live `Arc` is pointer-identical to the one hashed last time — a re-serialize
/// optimization; `compute` stays the drift signal, `ptr_eq` never is. On a pointer
/// miss, recompute via `compute`; on `Ok` cache `(spec.clone(), hash)` and return
/// it, on `Err` return `None` (matching the caller's `.ok()?`). Stores the `Arc`
/// itself (not a raw pointer) so a reused address can't false-match via `ptr_eq`
/// (ABA).
fn memoized_spec_hash<T>(
    cache: &Mutex<HashMap<String, (Arc<T>, u64)>>,
    name: &str,
    spec: &Arc<T>,
    compute: impl FnOnce(&T) -> Result<u64>,
) -> Option<u64> {
    let mut cache = cache.lock().expect("spec_hash_cache poisoned");
    if let Some((cached_spec, hash)) = cache.get(name) {
        if Arc::ptr_eq(cached_spec, spec) {
            return Some(*hash);
        }
    }
    let hash = compute(spec).ok()?;
    cache.insert(name.to_string(), (spec.clone(), hash));
    Some(hash)
}

/// Drop cache entries whose name is absent from `live`. Called once per pass (there
/// is no end-of-pass hook on `ConfigSource`) to evict repos removed from the
/// manifest.
fn retain_live_cache_entries<T>(
    cache: &Mutex<HashMap<String, (Arc<T>, u64)>>,
    live: &HashSet<&str>,
) {
    let mut cache = cache.lock().expect("spec_hash_cache poisoned");
    cache.retain(|name, _| live.contains(name.as_str()));
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
        let hash = memoized_spec_hash(&self.spec_hash_cache, name, &spec, spec_hash)?;

        Some(DesiredRepo {
            enabled: spec.enabled,
            spec_hash: hash,
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

/// Whether a completed build should record a generation. A deep-sharded repo that
/// was not already present is skipped (`reload_if_present` returned false) → `None`;
/// every other case records the generation.
fn apply_generation(
    deep: bool,
    applied: bool,
    generation: RepoGeneration,
) -> Option<RepoGeneration> {
    if deep && !applied {
        None
    } else {
        Some(generation)
    }
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
        Ok(apply_generation(
            deep,
            applied,
            RepoGeneration {
                spec_hash,
                storage_gen,
            },
        ))
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

/// Single-flight guard: run `body` under `lock` iff it is free (`try_lock`), else
/// skip (never queue) and return `None`. The tokio Mutex is await-safe, so the
/// guard is intentionally held across `body`'s await.
async fn run_exclusive<F, Fut, T>(lock: &tokio::sync::Mutex<()>, body: F) -> Option<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = T>,
{
    let Ok(_guard) = lock.try_lock() else {
        return None;
    };
    Some(body().await)
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
        // A held lock means "skip this pass", which is a success (Ok(())).
        run_exclusive(&self.reconcile_lock, move || async move {
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
            let outcome = config_reconcile::reconcile(
                &config,
                &manager,
                &current,
                repos_manager_concurrency()?,
            )
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

            // Evict spec-hash cache entries for repos no longer in the manifest.
            // Done here (once per pass) because ConfigSource has no end-of-pass
            // hook. The std Mutex is acquired after all awaits, not across one.
            if let Some(manifest) = self.configs.manifest() {
                let live: HashSet<&str> = manifest
                    .repos
                    .iter()
                    .map(|e| e.repo_name.as_str())
                    .collect();
                retain_live_cache_entries(&self.spec_hash_cache, &live);
            }

            Ok(())
        })
        .await
        .unwrap_or(Ok(()))
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

/// Loop mechanics for the reconcile background task: run `pass` immediately, then
/// forever wake on either a `trigger` notification or the backstop
/// `sleep(next_interval())` and run `pass` again. `next_interval` is injected (not
/// read from justknobs here) so tests can drive the loop without registered knobs.
async fn reconcile_loop<F, Fut>(
    pass: F,
    trigger: Arc<Notify>,
    mut next_interval: impl FnMut() -> Duration,
) where
    F: Fn() -> Fut,
    Fut: Future<Output = ()>,
{
    loop {
        pass().await;
        tokio::select! {
            _ = trigger.notified() => {}
            _ = tokio::time::sleep(next_interval()) => {}
        }
    }
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
    mononoke::spawn_task(reconcile_loop(
        move || {
            let driver = driver.clone();
            async move {
                // Time the whole pass; keep the metric emission adjacent to the pass.
                let start = Instant::now();
                if let Err(e) = driver.pass().await {
                    warn!("reconcile pass failed: {e:#}");
                }
                STATS::reconcile_tick_duration_ms
                    .add_value(start.elapsed().as_millis().try_into().unwrap_or(i64::MAX));
            }
        },
        trigger,
        reconcile_tick_interval,
    ))
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

/// A `ConfigUpdateReceiver` that keeps the tier-wide repo names map fresh on
/// every config change. Repo (re)building is owned by the reconcile loop;
/// reconcile does not maintain this map, so it lives here.
pub struct MononokeConfigUpdateReceiver {
    // Shared with MononokeReposManager and Mononoke<R>. Updated on every
    // config change so `list_repos` sees newly-added repos without waiting
    // for a process restart.
    repo_names_in_tier: Arc<ArcSwap<HashMap<String, CommitIdentityScheme>>>,
}

impl MononokeConfigUpdateReceiver {
    fn new(repo_names_in_tier: Arc<ArcSwap<HashMap<String, CommitIdentityScheme>>>) -> Self {
        Self { repo_names_in_tier }
    }

    /// Rebuild the tier-wide repo names map from `repo_configs` (the full
    /// tier config, not the per-task subset) and atomically swap it in.
    fn refresh_repo_names_in_tier(&self, repo_configs: &RepoConfigs) {
        let names =
            build_repo_names_in_tier(repo_configs.repos.iter().map(|(k, v)| (k, v.as_ref())));
        self.repo_names_in_tier.store(Arc::new(names));
    }
}

#[async_trait]
impl ConfigUpdateReceiver for MononokeConfigUpdateReceiver {
    async fn apply_update(
        &self,
        repo_configs: Arc<RepoConfigs>,
        _: Arc<StorageConfigs>,
    ) -> Result<()> {
        // Keep the tier-wide names map fresh so `list_repos` reflects the latest
        // tier config. Repo (re)building is owned by the reconcile loop.
        self.refresh_repo_names_in_tier(&repo_configs);
        Ok(())
    }

    async fn apply_repo_update(&self, repo_name: &str, repo_config: &RepoConfig) -> Result<()> {
        // Patch the names map from the passed-in arg (authoritative for THIS
        // repo). rcu() makes the load-mutate-store atomic against concurrent
        // writers (e.g. apply_update's bulk refresh); idempotent for this shape.
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
        Ok(())
    }
}

#[cfg(test)]
mod tests;
