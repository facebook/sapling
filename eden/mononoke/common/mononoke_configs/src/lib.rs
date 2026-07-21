/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! `MononokeConfigs` — the per-service entry point for fetching repo configs
//! and registering for config-update notifications.
//!
//! Internal organization:
//! - [`ConfigUpdateReceiver`][receiver::ConfigUpdateReceiver] trait is in
//!   [`receiver`]
//! - The unified watcher task (single tokio task that owns the blob, manifest,
//!   per-repo control channel, and per-repo wait fan-in) is in [`watcher`]
//! - The deterministic content-hash + last-updated-at helper used to expose
//!   stable config identity is in [`config_info`]
//!
//! `MononokeConfigs` itself owns the `ArcSwap` state and the task handles.

mod config_info;
mod receiver;
mod watcher;

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use std::sync::RwLock;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use arc_swap::ArcSwap;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use cached_config::ConfigUpdateWatcher;
use cloned::cloned;
use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use metaconfig_parser::config::configerator_config_handle;
use metaconfig_parser::configerator_manifest_handle;
use metaconfig_parser::configerator_repo_spec_handle;
use metaconfig_parser::parse_repo_spec;
use metaconfig_types::ConfigInfo;
use metaconfig_types::RepoConfig;
use repos::RawRepoConfigs;
use repos::RepoSpec;
use repos::TierManifest;
use stats::prelude::*;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::config_info::build_config_info;
pub use crate::receiver::ConfigUpdateReceiver;
use crate::watcher::RepoHandleEvent;
use crate::watcher::liveness_updater;
use crate::watcher::unified_config_watcher;

const CONFIGERATOR_TIER_PREFIX: &str = "configerator://scm/mononoke/repos/tiers/";

/// Shorthand for a value swapped atomically via `arc_swap`. Used for the
/// stateful slots inside `MononokeConfigs` that receivers read.
pub(crate) type Swappable<T> = Arc<ArcSwap<T>>;

define_stats! {
    prefix = "mononoke.config_refresh";
    refresh_failure_count: timeseries(Average, Sum, Count),
    refresh_success_count: timeseries(Average, Sum, Count),
    liveness_count: timeseries(Average, Sum, Count),
    spurious_reload_suppressed: timeseries(Average, Sum, Count),
    merge_skipped_no_handle: timeseries(Average, Sum, Count),
    per_repo_refresh_count: timeseries(Average, Sum, Count),
    per_repo_refresh_failure_count: timeseries(Average, Sum, Count),
    ensure_repo_handle_success_count: timeseries(Average, Sum, Count),
    ensure_repo_handle_failure_count: timeseries(Average, Sum, Count),
}

/// Outcome of a batch config load. Separates the configs that loaded/parsed
/// successfully (`loaded`) from the per-repo parse failures (`failed`), so
/// callers can fail closed instead of silently serving fewer repos.
pub struct BatchLoadOutcome {
    /// Repos that loaded successfully (cached hits + freshly parsed).
    pub loaded: Vec<(String, RepoConfig)>,
    /// Repos whose `RepoSpec` failed to parse, with the parse error.
    pub failed: Vec<(String, anyhow::Error)>,
}

/// Configuration provider and update notifier for all of Mononoke services
/// and jobs. The configurations provided by this struct are always up-to-date
/// with its source.
pub struct MononokeConfigs {
    repo_configs: Swappable<RepoConfigs>,
    storage_configs: Swappable<StorageConfigs>,
    update_receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>>,
    config_info: Swappable<Option<ConfigInfo>>,
    maybe_config_updater: Option<JoinHandle<()>>,
    maybe_liveness_updater: Option<JoinHandle<()>>,
    maybe_config_handle: Option<ConfigHandle<RawRepoConfigs>>,
    // Per-repo split-loading fields
    maybe_manifest_handle: Option<ConfigHandle<TierManifest>>,
    repo_handles: Arc<RwLock<HashMap<String, ConfigHandle<RepoSpec>>>>,
    config_store: Option<ConfigStore>,
    /// Tier name derived from the configerator config path.
    /// Used for resolving tier_overrides in RepoSpec configs during split-loading.
    tier_name: Option<String>,
    /// Sender side of the control channel that notifies `unified_config_watcher`
    /// when a new per-repo `ConfigUpdateWatcher<RepoSpec>` has been created
    /// (via sync_repo_handles or load_repo_config_handle) so it can be added
    /// to the watcher's `FuturesUnordered` set. `None` when split-loading is
    /// disabled.
    repo_handle_event_tx: Option<mpsc::UnboundedSender<RepoHandleEvent>>,
}

impl MononokeConfigs {
    /// Create a new instance of MononokeConfigs with configurations backed via ConfigStore.
    /// If the config path points to a dynamic config source (e.g. configerator), this enables
    /// auto-refresh of those configurations.
    pub fn new(
        config_path: impl AsRef<Path>,
        config_store: &ConfigStore,
        manifest_path: Option<&str>,
        runtime_handle: Handle,
    ) -> Result<Self> {
        let storage_configs = metaconfig_parser::load_storage_configs(&config_path, config_store)?;
        let storage_configs = Arc::new(ArcSwap::from_pointee(storage_configs));
        let repo_configs = metaconfig_parser::load_repo_configs(&config_path, config_store)?;
        let repo_configs = Arc::new(ArcSwap::from_pointee(repo_configs));

        // Derive tier name from the configerator config path.
        // Configerator paths follow the pattern:
        //   configerator://scm/mononoke/repos/tiers/{tier_name}
        let tier_name = config_path
            .as_ref()
            .to_str()
            .and_then(|p| p.strip_prefix(CONFIGERATOR_TIER_PREFIX))
            .filter(|t| !t.is_empty())
            .map(|t| t.to_owned());
        let update_receivers = Arc::new(ArcSwap::from_pointee(vec![]));
        let maybe_config_handle = configerator_config_handle(config_path.as_ref(), config_store)?;
        let config_info = if let Some(config_handle) = maybe_config_handle.as_ref() {
            match build_config_info(config_handle.get()) {
                Ok(new_config_info) => Some(new_config_info),
                Err(e) => {
                    warn!("Could not compute new config_info: {e:?}");
                    None
                }
            }
        } else {
            None
        };
        let config_info = Arc::new(ArcSwap::from_pointee(config_info));
        let maybe_manifest_handle = manifest_path
            .map(|path| configerator_manifest_handle(path, config_store))
            .transpose()?;

        // Only log split-loading status for configerator-backed configs (where
        // tier_name is set). File-backed configs used in tests always have
        // tier_name=None and manifest_path=None, so logging would be noise.
        if tier_name.is_some() {
            if let Some(manifest_path) = manifest_path {
                debug!(
                    "Split-loading enabled: config_path={}, manifest_path={manifest_path}, tier_name={:?}",
                    config_path.as_ref().to_string_lossy(),
                    tier_name.as_deref().unwrap_or("<none>"),
                );
            } else {
                debug!(
                    "Split-loading disabled: config_path={}, tier_name={:?}",
                    config_path.as_ref().to_string_lossy(),
                    tier_name.as_deref().unwrap_or("<none>"),
                );
            }
        }

        // Validate: split-loading (manifest) requires a tier name for resolving
        // tier_overrides in RepoSpec configs.
        if maybe_manifest_handle.is_some() && tier_name.is_none() {
            anyhow::bail!(
                "tier_name is required when split-loading is enabled (manifest_path is set)"
            );
        }

        let repo_handles = Arc::new(RwLock::new(HashMap::new()));

        // Control channel for the per-repo arm of unified_config_watcher.
        // Only created when split-loading is active (manifest is set), since
        // that's the only case where per-repo handles exist. Created BEFORE
        // the pre-load so we can enqueue `Added` events for pre-loaded handles
        // — the watcher will process those queued events on its first iteration
        // via the control arm, which avoids running a separate seed loop that
        // could race with a concurrent `load_repo_config_handle` call from
        // ShardManager and double-register a handle.
        let (repo_handle_event_tx, repo_handle_event_rx) = if maybe_manifest_handle.is_some() {
            let (tx, rx) = mpsc::unbounded_channel();
            (Some(tx), Some(rx))
        } else {
            (None, None)
        };

        // If manifest is available, pre-load handles for non-deep-sharded repos.
        // Collect all handles first, then insert in bulk under a single write lock.
        if let Some(ref manifest_handle) = maybe_manifest_handle {
            let manifest = manifest_handle.get();
            let handles_to_add: Vec<_> = manifest
                .repos
                .iter()
                .filter(|e| !e.is_deep_sharded)
                .map(|entry| {
                    let handle = configerator_repo_spec_handle(&entry.config_path, config_store)?;
                    Ok((entry.repo_name.clone(), handle))
                })
                .collect::<Result<Vec<_>>>()?;

            info!(
                "Split-loading: pre-loaded {} repo handles from manifest ({} total repos, {} deep-sharded skipped)",
                handles_to_add.len(),
                manifest.repos.len(),
                manifest.repos.iter().filter(|e| e.is_deep_sharded).count(),
            );

            // Derive watchers BEFORE handing handle ownership to the HashMap,
            // then enqueue `Added` events for each so the watcher loop will
            // register them on its first iteration. This is the sole path for
            // watcher registration — see comment on `repo_handle_event_tx` above.
            //
            // A failure here means the repo silently loses hot-reload: the
            // handle stays in `repo_handles` (so `load_repo_config_handle`
            // skips it on the fast path) but no `Added` event ever fires.
            // Logged so this is observable in production.
            if let Some(tx) = repo_handle_event_tx.as_ref() {
                for (name, handle) in &handles_to_add {
                    match handle.watcher() {
                        Ok(w) => {
                            // Channel is unbounded and the watcher hasn't
                            // started yet, so send cannot fail except via an
                            // `rx` drop — which we control.
                            let _ = tx.send(RepoHandleEvent::Added(name.clone(), w));
                        }
                        Err(e) => {
                            warn!(
                                "Pre-load: failed to create watcher for {name}, \
                                 per-repo hot-reload disabled for this repo until restart: {e:?}",
                            );
                        }
                    }
                }
            }

            repo_handles
                .write()
                .map_err(|e| anyhow!("repo_handles lock poisoned: {e}"))?
                .extend(handles_to_add);
        }

        let maybe_liveness_updater =
            if maybe_config_handle.is_some() || maybe_manifest_handle.is_some() {
                Some(runtime_handle.spawn(liveness_updater()))
            } else {
                None
            };

        let maybe_config_updater =
            if maybe_config_handle.is_some() || maybe_manifest_handle.is_some() {
                cloned!(
                    repo_handles,
                    repo_configs,
                    storage_configs,
                    config_info,
                    update_receivers,
                );
                let config_store_clone = config_store.clone();
                let tier = tier_name.clone();
                Some(runtime_handle.spawn(unified_config_watcher(
                    maybe_config_handle.clone(),
                    maybe_manifest_handle.clone(),
                    repo_handles,
                    config_store_clone,
                    tier,
                    repo_configs,
                    storage_configs,
                    config_info,
                    update_receivers,
                    repo_handle_event_rx,
                )))
            } else {
                None
            };

        Ok(Self {
            repo_configs,
            storage_configs,
            update_receivers,
            config_info,
            maybe_config_updater,
            maybe_config_handle,
            maybe_liveness_updater,
            maybe_manifest_handle,
            repo_handles,
            config_store: Some(config_store.clone()),
            tier_name,
            repo_handle_event_tx,
        })
    }

    /// The latest repo configs fetched from the underlying configuration store.
    pub fn repo_configs(&self) -> Arc<RepoConfigs> {
        // Load full since there could be lots of calls to repo_configs.
        self.repo_configs.load_full()
    }

    /// The latest storage configs fetched from the underlying configuration store.
    pub fn storage_configs(&self) -> Arc<StorageConfigs> {
        // Load full since there could be lots of calls to storage_configs.
        self.storage_configs.load_full()
    }

    /// The info on the latest config fetched from the underlying configuration store.
    pub fn config_info(&self) -> Arc<Option<ConfigInfo>> {
        self.config_info.load_full()
    }

    /// Returns the ConfigStore, if available (configerator-backed configs only).
    pub fn config_store(&self) -> Option<&ConfigStore> {
        self.config_store.as_ref()
    }

    /// Returns the current TierManifest, if split-loading is enabled.
    pub fn manifest(&self) -> Option<Arc<TierManifest>> {
        self.maybe_manifest_handle.as_ref().map(|h| h.get())
    }

    /// The tier name (derived from the configerator config path), if split-
    /// loading is enabled. The reconcile controller needs it to parse `RepoSpec`s.
    pub fn tier_name(&self) -> Option<&str> {
        self.tier_name.as_deref()
    }

    /// The live per-repo `RepoSpec` read straight from its subscribed config
    /// handle (bypassing the bulk config cache), so the reconcile controller sees
    /// current desired state for drift detection. Returns `None` when the repo
    /// has no subscribed handle (not split-loaded, or not yet subscribed) —
    /// callers must treat that as "unknown", never "drop".
    pub fn live_repo_spec(&self, repo_name: &str) -> Option<Arc<RepoSpec>> {
        self.repo_handles
            .read()
            .ok()?
            .get(repo_name)
            .map(|handle| handle.get())
    }

    /// Is automatic update of the underlying configuration enabled?
    pub fn auto_update_enabled(&self) -> bool {
        // If the config updater handle is none, configs won't be updated.
        self.maybe_config_updater.is_some()
    }

    // Config watcher that can be used to get notified of the latest
    // changes in the underlying config and to act on it. This is useful
    // if the processing to be performed is long running which is not supported
    // via ConfigUpdateReceivers
    pub fn config_watcher(&self) -> Option<ConfigUpdateWatcher<RawRepoConfigs>> {
        self.maybe_config_handle
            .as_ref()
            .and_then(|config_handle| config_handle.watcher().ok())
    }

    /// Register an instance of ConfigUpdateReceiver to receive notifications of updates to
    /// the underlying configs which can then be used to perform further actions. Note that
    /// the operation performed by the ConfigUpdateReceiver should not be too long running.
    /// If that's the case, use config_watcher method instead.
    ///
    /// Thread-safe: uses `rcu` so concurrent registrations from multiple
    /// services don't lose entries to a load-modify-store race.
    pub fn register_for_update(&self, update_receiver: Arc<dyn ConfigUpdateReceiver>) {
        self.update_receivers.rcu(|current| {
            let mut next: Vec<Arc<dyn ConfigUpdateReceiver>> = (**current).clone();
            next.push(update_receiver.clone());
            next
        });
    }

    /// Drop the per-repo ConfigHandle (called by ShardManager on_drop_shard via
    /// repos_manager::remove_repo). Symmetric counterpart to load_repo_config_handle.
    /// No-op if no handle is held.
    pub fn remove_repo_config_handle(&self, repo_name: &str) {
        let had_handle = match self.repo_handles.write() {
            Ok(mut handles) => handles.remove(repo_name).is_some(),
            Err(e) => {
                error!("repo_handles lock poisoned while removing {repo_name}: {e}");
                false
            }
        };
        // Only evict the bulk repo_configs entry when we actually dropped a handle
        // (i.e. a split-loaded repo). This forces a later get_or_load to re-parse
        // from the fresh handle instead of serving stale config (S685134), while
        // leaving a legacy-blob-only entry intact so it can still be re-added.
        if had_handle {
            info!("Removed config handle for repo: {repo_name}");
            if self.repo_configs.load().repos.contains_key(repo_name) {
                self.repo_configs.rcu(|current| {
                    let mut next = (**current).clone();
                    next.remove_repo(repo_name);
                    next
                });
            }
        }
    }

    /// Create a per-repo ConfigHandle on-demand (called by ShardManager on_add_shard).
    pub fn load_repo_config_handle(&self, repo_name: &str) -> Result<()> {
        // Fast path: already loaded
        if self
            .repo_handles
            .read()
            .map_err(|e| anyhow!("repo_handles lock poisoned: {e}"))?
            .contains_key(repo_name)
        {
            return Ok(());
        }

        let manifest = self
            .maybe_manifest_handle
            .as_ref()
            .context("No manifest handle available")?
            .get();

        let entry = manifest
            .repos
            .iter()
            .find(|e| e.repo_name == repo_name)
            .ok_or_else(|| anyhow!("Repo {repo_name} not found in manifest"))?;

        let config_store = self
            .config_store
            .as_ref()
            .context("No config store available")?;

        let handle = configerator_repo_spec_handle(&entry.config_path, config_store)?;
        // Derive the watcher BEFORE moving the handle into repo_handles —
        // the watcher only requires &self on ConfigHandle, but we hand off
        // ownership of the handle to the HashMap below.
        //
        // Subscription is live from `handle.watcher()` onwards: any
        // configerator updates between here and when the unified_config_watcher
        // loop processes the `Added` event get buffered in the
        // `tokio::sync::watch::Receiver` (latest-value semantics) and are
        // delivered on the first `wait_for_next` call. No update can be
        // dropped.
        let watcher = handle.watcher();
        self.repo_handles
            .write()
            .map_err(|e| anyhow!("repo_handles lock poisoned: {e}"))?
            .insert(repo_name.to_owned(), handle);
        // Notify unified_config_watcher to start watching this repo for
        // per-repo content updates. Send AFTER the handle is in the map so
        // the `still_present` check in `handle_per_repo_fire` passes on
        // dispatch.
        match watcher {
            Ok(w) => {
                if let Some(tx) = self.repo_handle_event_tx.as_ref() {
                    if let Err(e) = tx.send(RepoHandleEvent::Added(repo_name.to_owned(), w)) {
                        warn!(
                            "Failed to send Added event for repo {repo_name} \
                             (watcher loop gone?): {e}",
                        );
                    }
                }
            }
            Err(e) => {
                warn!(
                    "load_repo_config_handle: failed to create watcher for {repo_name}, \
                     per-repo hot-reload disabled for this repo until restart: {e:?}",
                );
            }
        }
        Ok(())
    }

    /// Idempotent best-effort subscription of a per-repo `ConfigHandle`.
    /// No-op when already registered, when split-loading is off, or when
    /// the repo is not in the manifest (legacy-only). Subscription
    /// failure is the only `Err` path. Closes the gap that caused
    /// S678887.
    pub fn ensure_repo_config_handle(&self, repo_name: &str) -> Result<()> {
        if self
            .repo_handles
            .read()
            .map_err(|e| anyhow!("repo_handles lock poisoned: {e}"))?
            .contains_key(repo_name)
        {
            return Ok(());
        }

        let Some(manifest_handle) = self.maybe_manifest_handle.as_ref() else {
            return Ok(());
        };
        let manifest = manifest_handle.get();

        let Some(entry) = manifest.repos.iter().find(|e| e.repo_name == repo_name) else {
            return Ok(());
        };

        let config_store = self
            .config_store
            .as_ref()
            .context("No config store available")?;

        let handle = configerator_repo_spec_handle(&entry.config_path, config_store)
            .inspect_err(|_| {
                STATS::ensure_repo_handle_failure_count.add_value(1);
            })
            .with_context(|| {
                format!(
                    "ensure_repo_config_handle: failed to subscribe to repo {repo_name} \
                     (config_path={})",
                    entry.config_path
                )
            })?;
        let watcher = handle.watcher();

        // Check-under-write-lock: if a concurrent caller won the race,
        // drop our handle — its configerator subscription cancels on Drop.
        let inserted = {
            let mut handles = self
                .repo_handles
                .write()
                .map_err(|e| anyhow!("repo_handles lock poisoned: {e}"))?;
            if handles.contains_key(repo_name) {
                false
            } else {
                handles.insert(repo_name.to_owned(), handle);
                true
            }
        };

        if inserted {
            STATS::ensure_repo_handle_success_count.add_value(1);
            // Send Added AFTER insert so handle_per_repo_fire's still_present check passes.
            match watcher {
                Ok(w) => {
                    if let Some(tx) = self.repo_handle_event_tx.as_ref() {
                        if let Err(e) = tx.send(RepoHandleEvent::Added(repo_name.to_owned(), w)) {
                            warn!("Failed to send Added event for {repo_name}: {e}");
                        }
                    }
                }
                Err(e) => {
                    warn!(
                        "Failed to create watcher for {repo_name}, per-repo hot-reload \
                         disabled until restart: {e:?}"
                    );
                }
            }
        }
        Ok(())
    }

    /// Load a repo config on-demand. Checks the repo_configs cache first,
    /// falls back to loading from the TierManifest via ConfigHandle.
    ///
    /// Thread-safe: uses `ArcSwap::rcu` for write serialization. The closure
    /// inside `rcu` runs at least once (possibly multiple times on CAS retry),
    /// so the expensive work — ConfigHandle subscription, RepoSpec parsing —
    /// happens once OUTSIDE the closure.
    pub fn get_or_load_repo_config(&self, repo_name: &str) -> Result<RepoConfig> {
        // Subscribe per-repo handle before the cache lookup — otherwise the
        // fast path returns a legacy-blob entry without ever subscribing,
        // and a subsequent blob hot-reload silently drops the repo. S678887.
        self.ensure_repo_config_handle(repo_name)?;
        // Fast path: lock-free read from cache (covers both legacy blob
        // and previously loaded split-config repos)
        if let Some(config) = self.repo_configs.load_full().repos.get(repo_name) {
            return Ok((**config).clone());
        }

        // Slow path: try loading from manifest. If split-loading infrastructure
        // is unavailable (no manifest, no config store), this will fail and
        // we return the error — the fast path above already checked the legacy blob.
        let repo_config = self.load_and_parse_repo_config(repo_name)?;

        // Insert into cache via rcu. The closure is idempotent: on CAS retry
        // it re-runs against a fresher snapshot, and if a concurrent writer
        // (per-repo refresh, another get_or_load) already inserted this repo,
        // we keep their entry to avoid a redundant clone+store.
        self.repo_configs.rcu(|current| {
            let mut next = (**current).clone();
            if !next.repos.contains_key(repo_name) {
                next.insert_repo(repo_name.to_owned(), repo_config.clone());
            }
            next
        });

        // All paths leave the cache containing some entry for repo_name with
        // the same value (configs for one repo at one snapshot are
        // deterministic). Return what we parsed.
        Ok(repo_config)
    }

    /// Subscribe to a repo's ConfigHandle and parse its RepoSpec into a RepoConfig.
    /// This is the shared helper for get_or_load_repo_config and batch_load_repo_configs.
    fn load_and_parse_repo_config(&self, repo_name: &str) -> Result<RepoConfig> {
        self.load_repo_config_handle(repo_name)?;
        let handle = self
            .repo_handles
            .read()
            .map_err(|e| anyhow!("repo_handles lock poisoned: {e}"))?
            .get(repo_name)
            .context("handle not found after load")?
            .clone();
        let repo_spec = handle.get();
        let tier = self
            .tier_name
            .as_deref()
            .context("tier_name required for split-loading")?;
        let manifest = self
            .maybe_manifest_handle
            .as_ref()
            .context("manifest handle required for split-loading")?
            .get();
        parse_repo_spec(Arc::unwrap_or_clone(repo_spec), tier, &manifest.storage)
    }

    /// Batch-load repo configs, surfacing per-repo parse failures instead of
    /// warn+dropping them. Single rcu acquisition, single HashMap clone, single
    /// ArcSwap store regardless of how many repos are loaded.
    ///
    /// `loaded` holds the successes (cached hits + freshly parsed), `failed`
    /// holds the per-repo parse errors. The outer `Result` is reserved for
    /// infra errors (e.g. lock poison, handle subscription failure), not
    /// per-repo parse errors.
    pub fn batch_load_repo_configs_checked(
        &self,
        repo_names: &[String],
    ) -> Result<BatchLoadOutcome> {
        // Subscribe per-repo handles up-front; the cached-fast-path loop
        // below would otherwise skip subscription. S678887.
        for name in repo_names {
            self.ensure_repo_config_handle(name)?;
        }

        // Step 1: Separate cached from missing
        let current = self.repo_configs.load_full();
        let mut results: Vec<(String, RepoConfig)> = Vec::new();
        let mut missing: Vec<String> = Vec::new();

        for name in repo_names {
            if let Some(config) = current.repos.get(name.as_str()) {
                results.push((name.clone(), (**config).clone()));
            } else {
                missing.push(name.clone());
            }
        }

        if missing.is_empty() {
            return Ok(BatchLoadOutcome {
                loaded: results,
                failed: Vec::new(),
            });
        }

        // Step 2: Subscribe to ConfigHandles + parse OUTSIDE rcu. Accumulate
        // per-repo parse failures instead of dropping them, so the caller can
        // decide whether to fail closed.
        let mut newly_loaded: Vec<(String, RepoConfig)> = Vec::new();
        let mut failed: Vec<(String, anyhow::Error)> = Vec::new();
        for name in &missing {
            match self.load_and_parse_repo_config(name) {
                Ok(config) => newly_loaded.push((name.clone(), config)),
                Err(e) => failed.push((name.clone(), e)),
            }
        }

        // Step 3: rcu — bulk insert via a single closure that re-runs on CAS
        // failure. Safe against concurrent per-repo refreshes and other
        // get_or_load callers without needing a separate lock. Already-present
        // entries (set by a concurrent writer between Step 1 and here) win
        // — caller-side idempotency.
        if !newly_loaded.is_empty() {
            self.repo_configs.rcu(|current| {
                let mut next = (**current).clone();
                for (name, config) in &newly_loaded {
                    if !next.repos.contains_key(name.as_str()) {
                        next.insert_repo(name.clone(), config.clone());
                    }
                }
                next
            });
        }

        results.extend(newly_loaded);
        Ok(BatchLoadOutcome {
            loaded: results,
            failed,
        })
    }

    /// Batch-load repo configs. Single rcu acquisition, single HashMap clone,
    /// single ArcSwap store regardless of how many repos are loaded.
    /// This is the default path for startup (`open_managed_repos`).
    ///
    /// Thin wrapper over [`Self::batch_load_repo_configs_checked`] that
    /// warn+drops per-repo parse failures (historical behavior). Callers that
    /// need to observe the failures should use the checked variant.
    pub fn batch_load_repo_configs(
        &self,
        repo_names: &[String],
    ) -> Result<Vec<(String, RepoConfig)>> {
        let outcome = self.batch_load_repo_configs_checked(repo_names)?;
        for (name, e) in &outcome.failed {
            warn!("batch_load: failed to load config for {name}: {e:#}");
        }
        Ok(outcome.loaded)
    }

    /// Load configs for all repos discovered from both the legacy blob and
    /// the manifest. Uses batch loading (single rcu, single clone).
    pub fn load_all_repo_configs(&self) -> Result<Vec<(String, RepoConfig)>> {
        let names = self.all_repo_names();
        self.batch_load_repo_configs(&names)
    }

    /// Like [`Self::load_all_repo_configs`] but surfaces per-repo parse
    /// failures (via [`Self::batch_load_repo_configs_checked`]) instead of
    /// warn+dropping them. Seam for fail-closed startup.
    pub fn load_all_repo_configs_checked(&self) -> Result<BatchLoadOutcome> {
        let names = self.all_repo_names();
        self.batch_load_repo_configs_checked(&names)
    }

    /// Union of repo names from the legacy blob cache and the manifest.
    fn all_repo_names(&self) -> Vec<String> {
        let mut all_names: HashSet<String> = self
            .repo_configs
            .load_full()
            .repos
            .keys()
            .cloned()
            .collect();
        if let Some(manifest) = self.manifest() {
            for entry in &manifest.repos {
                all_names.insert(entry.repo_name.clone());
            }
        }
        all_names.into_iter().collect()
    }

    /// Load a repo config by repository ID. O(1) cache lookup via repos_by_id
    /// index, falls back to searching the manifest by repo_id.
    pub fn get_or_load_repo_config_by_id(&self, repo_id: i32) -> Result<(String, RepoConfig)> {
        // Fast path: O(1) lookup via repos_by_id index
        let current = self.repo_configs.load_full();
        if let Some((name, config)) = current.get_repo_config_by_raw_id(repo_id) {
            let name = name.clone();
            // See get_or_load_repo_config — S678887.
            self.ensure_repo_config_handle(&name)?;
            return Ok((name, config.clone()));
        }

        // Slow path: search manifest for repo_id.
        // If no manifest is available (e.g. integration tests, file-backed configs),
        // fall back to the original "unknown repoid" error.
        let manifest = match self.maybe_manifest_handle.as_ref() {
            Some(handle) => handle.get(),
            None => anyhow::bail!("unknown repoid: RepositoryId({repo_id})"),
        };
        let entry = manifest
            .repos
            .iter()
            .find(|e| e.repo_id == repo_id)
            .ok_or_else(|| anyhow!("unknown repoid: RepositoryId({repo_id})"))?;
        let config = self.get_or_load_repo_config(&entry.repo_name)?;
        Ok((entry.repo_name.clone(), config))
    }
}

impl Drop for MononokeConfigs {
    // If MononokeConfigs is getting dropped, then we need to terminate the updater
    // process as well.
    fn drop(&mut self) {
        // If the updater process exists, abort it.
        if let Some(updater_handle) = self.maybe_config_updater.as_ref() {
            updater_handle.abort();
        }
        // If the liveness updater process exists, abort it.
        if let Some(liveness_updater) = self.maybe_liveness_updater.as_ref() {
            liveness_updater.abort();
        }
    }
}

#[cfg(test)]
mod tests;
