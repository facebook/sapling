/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! `unified_config_watcher` + its supporting helpers.
//!
//! The watcher is a single tokio task that owns four event sources:
//!
//! 1. **Legacy blob** [`BlobSource`] — `Loaded` fires on tier-blob changes;
//!    `Skipped` (skip_tier_blob_load) parks this arm forever
//! 2. **Tier manifest** `ConfigHandle<TierManifest>` — fires on manifest changes
//!    (repo add/remove, sharding mode flips)
//! 3. **Per-repo control channel** `mpsc::UnboundedReceiver<RepoHandleEvent>` —
//!    notifies the loop when a new per-repo `ConfigHandle<RepoSpec>` is installed
//!    by `load_repo_config_handle` / `ensure_repo_config_handle` (ShardManager
//!    on_add_shard, startup batch loading)
//! 4. **Per-repo wait fan-in** `FuturesUnordered<wait_one>` — one in-flight
//!    future per per-repo watcher; fires when a repo's RepoSpec content changes
//!
//! All four arms feed a single `tokio::select!` so config-application work
//! serializes within one task.

use std::collections::HashMap;
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::RwLock;
use std::time::Duration;

use anyhow::Result;
use anyhow::anyhow;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use cached_config::ConfigUpdateWatcher;
use futures::future::join_all;
use futures::stream::FuturesUnordered;
use futures::stream::StreamExt;
use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use metaconfig_parser::config::load_configs_from_raw;
use metaconfig_parser::configerator_repo_spec_handle;
use metaconfig_parser::parse_manifest_common_and_storage;
use metaconfig_parser::parse_repo_spec;
use metaconfig_types::CommonConfig;
use metaconfig_types::ConfigInfo;
use metaconfig_types::RepoConfig;
use repos::RawRepoConfigs;
use repos::RepoSpec;
use repos::TierManifest;
use stats::prelude::*;
use tokio::sync::mpsc;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::warn;

use crate::STATS;
use crate::Swappable;
use crate::config_info::build_config_info;
use crate::receiver::ConfigUpdateReceiver;
use crate::resolve_manifest_common_and_storage;
use crate::use_manifest_source;

const LIVENESS_INTERVAL: Duration = Duration::from_secs(300);

/// Nothing else wakes the loop on a JustKnob change, so this bounds rollback latency.
const KNOB_POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Result of awaiting one per-repo watcher fire. Owns the watcher so the
/// caller can re-push a fresh wait future for it without re-subscribing.
type PerRepoWaitResult = (String, Result<Arc<RepoSpec>>, ConfigUpdateWatcher<RepoSpec>);

/// Boxed `wait_one` future. Trait-object form because `FuturesUnordered`
/// can't be generic over a concrete async-fn type.
type PerRepoFuture = Pin<Box<dyn std::future::Future<Output = PerRepoWaitResult> + Send>>;

/// Notification sent to `unified_config_watcher` when a new per-repo handle
/// is registered. Removal is implicit (the watcher's `wait_for_next` returns
/// `Err` when the handle is dropped — see comment on `handle_per_repo_fire`).
pub(crate) enum RepoHandleEvent {
    Added(String, ConfigUpdateWatcher<RepoSpec>),
}

/// Where the watcher sources the legacy tier blob from; the two arms demand different reload failure policies.
pub(crate) enum BlobSource {
    /// Legacy: blob loaded and watched; manifest parse failures fall back to the blob.
    Loaded(ConfigHandle<RawRepoConfigs>),
    /// skip_tier_blob_load: manifest is authoritative; parse failures keep last known good.
    Skipped,
}

/// Background task that periodically bumps the `liveness_count` stat so
/// monitoring can detect a hung config-update task. Spawned alongside
/// `unified_config_watcher` in `MononokeConfigs::new`.
pub(crate) async fn liveness_updater() {
    loop {
        STATS::liveness_count.add_value(1);
        tokio::time::sleep(LIVENESS_INTERVAL).await;
    }
}

/// `==` comparison that treats both-`None` as "no change" and any other
/// missing-side combination as "changed". Used to dedupe spurious reloads
/// where the underlying configerator version bumped but the content didn't.
fn content_changed<T: PartialEq>(prev: &Option<Arc<T>>, current: &Option<Arc<T>>) -> bool {
    match (prev, current) {
        (Some(a), Some(b)) => **a != **b,
        (None, None) => false,
        _ => true,
    }
}

/// Per-repo analogue of [`content_changed`]: did the `RepoSpec` change from the
/// last-applied content? `None` (nothing recorded yet) counts as changed.
fn spec_content_changed(prev: Option<&Arc<RepoSpec>>, current: &RepoSpec) -> bool {
    match prev {
        Some(p) => **p != *current,
        None => true,
    }
}

/// Awaits the next update on a `ConfigUpdateWatcher`, parking forever if no
/// watcher is configured. Used to keep the blob/manifest arms of `select!`
/// valid when only one of the two is active.
async fn wait_for_handle<T: Send + Sync + 'static>(
    watcher: &mut Option<ConfigUpdateWatcher<T>>,
) -> Result<()> {
    match watcher {
        Some(w) => {
            w.wait_for_next().await?;
            Ok(())
        }
        None => std::future::pending().await,
    }
}

/// Awaits the next event from the per-repo control channel. If split-loading
/// is disabled (rx is None) this parks forever, mirroring `wait_for_handle`.
async fn wait_for_event(
    rx: &mut Option<mpsc::UnboundedReceiver<RepoHandleEvent>>,
) -> Option<RepoHandleEvent> {
    match rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

/// Awaits the next item from a stream, parking forever when the stream is
/// empty or terminated. Keeps the per-repo `FuturesUnordered` arm of the
/// `select!` valid before any per-repo watchers have been registered.
async fn next_or_pending<S>(stream: &mut S) -> S::Item
where
    S: futures::Stream + Unpin,
{
    match stream.next().await {
        Some(item) => item,
        None => std::future::pending().await,
    }
}

/// One per-repo wait. Takes ownership of the watcher and returns it alongside
/// the wait result so the caller can re-push the next wait into a
/// `FuturesUnordered` without re-creating the underlying subscription.
async fn wait_one(
    repo_name: String,
    mut watcher: ConfigUpdateWatcher<RepoSpec>,
) -> PerRepoWaitResult {
    let result = watcher.wait_for_next().await;
    (repo_name, result, watcher)
}

/// Register a per-repo watcher, seeding `prev_specs` with the repo's current
/// content so even a one-shot spurious version bump is deduped on its first fire.
fn push_per_repo_watcher(
    name: String,
    watcher: ConfigUpdateWatcher<RepoSpec>,
    repo_handles: &RwLock<HashMap<String, ConfigHandle<RepoSpec>>>,
    prev_specs: &mut HashMap<String, Arc<RepoSpec>>,
    per_repo_wait_futures: &mut FuturesUnordered<PerRepoFuture>,
) {
    match repo_handles.read() {
        Ok(handles) => {
            if let Some(handle) = handles.get(&name) {
                prev_specs.insert(name.clone(), handle.get());
            }
        }
        Err(e) => error!("repo_handles lock poisoned seeding prev_spec for {name}: {e}"),
    }
    per_repo_wait_futures.push(Box::pin(wait_one(name, watcher)));
}

/// Free function (not an inline async block) so the compiler infers a concrete
/// future type. Required to avoid an "implementation of FnOnce is not general
/// enough" HRTB error when used inside a `FuturesUnordered` over a
/// `Vec<Arc<dyn ConfigUpdateReceiver>>` whose `dyn Trait` lifetime variance
/// trips the closure-bound inference inside a spawned `'static` task.
async fn dispatch_apply_repo_update(
    receiver: Arc<dyn ConfigUpdateReceiver>,
    repo_name: String,
    repo_config: RepoConfig,
) -> Result<()> {
    receiver.apply_repo_update(&repo_name, &repo_config).await
}

/// Applies a per-repo config update atomically against the bulk `RepoConfigs`
/// Arc and the receiver-side state.
///
/// **Ordering matters**: the bulk Arc is patched FIRST so receivers that read
/// `MononokeConfigs::repo_configs()` during `apply_repo_update` see the new
/// state for `repo_name`. The trait comment on
/// `ConfigUpdateReceiver::apply_repo_update` documents this ordering invariant
/// ("the caller must have already swapped in the new config").
///
/// Returns `true` iff every receiver's `apply_repo_update` succeeded. The
/// caller increments the per-refresh success/failure stats so per-refresh
/// counts stay mutually exclusive even when N receivers fail on one refresh.
async fn apply_per_repo_update(
    repo_name: &str,
    new_config: RepoConfig,
    repo_configs: &Swappable<RepoConfigs>,
    update_receivers: &Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>>,
) -> bool {
    // (a) Patch the bulk RepoConfigs Arc via rcu. The closure runs at least
    // once and re-runs on any concurrent writer's CAS failure, so this is
    // safe against the other rcu writers (`get_or_load_repo_config`,
    // `batch_load_repo_configs`) without any lock.
    repo_configs.rcu(|current| {
        let mut next = (**current).clone();
        // Via insert_repo to keep repos_by_id consistent (and Arc-wrap).
        next.insert_repo(repo_name.to_owned(), new_config.clone());
        next
    });

    // (b) Call apply_repo_update on each receiver concurrently via
    // FuturesUnordered. In practice there are typically 1-2 receivers; the
    // unbounded fan-out is safe because the receiver count is governed by
    // `register_for_update` call sites in each service binary (not by repo
    // count or request volume).
    // Snapshot receivers to an owned Vec so each future owns its Arc rather
    // than borrowing into the unified_config_watcher task's frame.
    let mut futs: FuturesUnordered<_> = update_receivers
        .load()
        .iter()
        .cloned()
        .map(|r| dispatch_apply_repo_update(r, repo_name.to_owned(), new_config.clone()))
        .collect();
    let mut had_failure = false;
    while let Some(result) = futs.next().await {
        if let Err(e) = result {
            error!("apply_repo_update for repo {repo_name} failed on a receiver: {e:?}");
            had_failure = true;
        }
    }
    !had_failure
}

/// Unified config watcher: monitors the legacy blob `ConfigHandle`, the
/// `TierManifest` `ConfigHandle`, and a dynamic set of per-repo
/// `ConfigHandle<RepoSpec>` watchers via `tokio::select!`, applying changes
/// exactly once.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn unified_config_watcher(
    blob_source: BlobSource,
    manifest_handle: Option<ConfigHandle<TierManifest>>,
    repo_handles: Arc<RwLock<HashMap<String, ConfigHandle<RepoSpec>>>>,
    config_store: ConfigStore,
    tier_name: Option<String>,
    repo_configs: Swappable<RepoConfigs>,
    storage_configs: Swappable<StorageConfigs>,
    config_info: Swappable<Option<ConfigInfo>>,
    update_receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>>,
    mut repo_handle_event_rx: Option<mpsc::UnboundedReceiver<RepoHandleEvent>>,
) {
    // Skipped yields no watcher: the blob select! arm parks forever.
    let mut blob_watcher = match &blob_source {
        BlobSource::Loaded(handle) => handle.watcher().map(Some).unwrap_or_else(|e| {
            error!("Failed to create blob config watcher: {e:?}");
            None
        }),
        BlobSource::Skipped => None,
    };
    let mut manifest_watcher = manifest_handle
        .as_ref()
        .map(|h| h.watcher())
        .transpose()
        .unwrap_or_else(|e| {
            error!("Failed to create manifest watcher: {e:?}");
            None
        });

    if blob_watcher.is_none() && manifest_watcher.is_none() {
        warn!("No config watchers available, unified_config_watcher exiting");
        return;
    }

    let mut state = ReloadState::default();

    // Last-applied RepoSpec per repo; lets the per-repo arm skip spurious
    // identical-content reloads. Seeded at registration.
    let mut prev_specs: HashMap<String, Arc<RepoSpec>> = HashMap::new();

    // Per-repo watcher set, fed by the control-channel arm and sync_repo_handles.
    let mut per_repo_wait_futures: FuturesUnordered<PerRepoFuture> = FuturesUnordered::new();

    // Hoisted: a `sleep` inside the `select!` is rebuilt each iteration and starves.
    let mut knob_poll = tokio::time::interval_at(
        tokio::time::Instant::now() + KNOB_POLL_INTERVAL,
        KNOB_POLL_INTERVAL,
    );
    knob_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            result = wait_for_handle(&mut blob_watcher) => {
                if let Err(e) = result {
                    error!("Error waiting for blob config update: {e:?}");
                    continue;
                }
            }
            result = wait_for_handle(&mut manifest_watcher) => {
                if let Err(e) = result {
                    error!("Error waiting for manifest config update: {e:?}");
                    continue;
                }
            }
            event = wait_for_event(&mut repo_handle_event_rx) => {
                match event {
                    Some(RepoHandleEvent::Added(name, watcher)) => {
                        debug!("Registering per-repo watcher for {name}");
                        push_per_repo_watcher(
                            name,
                            watcher,
                            &repo_handles,
                            &mut prev_specs,
                            &mut per_repo_wait_futures,
                        );
                    }
                    None => {
                        // Sender side dropped — disable this arm so wait_for_event parks.
                        repo_handle_event_rx = None;
                    }
                }
                continue;
            }
            (name, result, watcher) = next_or_pending(&mut per_repo_wait_futures) => {
                handle_per_repo_fire(
                    name,
                    result,
                    watcher,
                    &repo_handles,
                    tier_name.as_deref(),
                    state.prev_manifest.as_deref(),
                    &repo_configs,
                    &update_receivers,
                    &mut per_repo_wait_futures,
                    &mut prev_specs,
                ).await;
                continue;
            }
            _ = knob_poll.tick() => {
                // Fall through only on a flip -- a full pass re-parses every RepoSpec.
                // `last_use_manifest` starts `None`, so the FIRST tick always falls
                // through. Deliberate: that pass is what sets `prev_manifest`, and
                // `handle_per_repo_fire` defers every per-repo reload until it is set.
                // Do not seed this to suppress that pass, and never seed it from a
                // second `use_manifest_source()` read -- `MononokeConfigs::new` already
                // read the knob and applied it, so a flip in between would be recorded
                // as "already applied" and never reconciled.
                if state.last_use_manifest == Some(effective_use_manifest(&blob_source)) {
                    continue;
                }
            }
        }

        run_reload_pass(
            &blob_source,
            manifest_handle.as_ref(),
            &repo_handles,
            &config_store,
            tier_name.as_deref(),
            &repo_configs,
            &storage_configs,
            &config_info,
            &update_receivers,
            &mut state,
            &mut prev_specs,
            &mut per_repo_wait_futures,
        )
        .await;
    }
}

/// State one reload pass hands to the next; extracted so `run_reload_pass` is unit-testable.
#[derive(Default)]
struct ReloadState {
    /// Last applied blob content, for spurious-reload dedup.
    prev_blob: Option<Arc<RawRepoConfigs>>,
    /// Last applied manifest content; advanced only by a successful pass.
    prev_manifest: Option<Arc<TierManifest>>,
    /// Parsed blob configs, reused when only the manifest changed.
    cached_parsed: Option<RepoConfigs>,
    /// Kept for comparison and fallback. `Arc` because `StorageConfigs` isn't `Clone`.
    cached_blob_storage: Option<Arc<StorageConfigs>>,
    /// Last applied knob value, so a flip alone forces a resolution pass.
    last_use_manifest: Option<bool>,
    /// Reused when a re-parse yields equal content, to keep pointer identity stable.
    cached_manifest_storage: Option<Arc<StorageConfigs>>,
}

/// Flip detection is Loaded-arm-only: skip mode never reads the knob (no blob to route back to).
fn effective_use_manifest(blob_source: &BlobSource) -> bool {
    match blob_source {
        BlobSource::Loaded(_) => use_manifest_source(),
        BlobSource::Skipped => true,
    }
}

/// How a reload pass sources `common`/`storage`.
enum ManifestValues {
    /// Skip mode: authoritative — must not consult `use_manifest_source()` or fall back.
    Authoritative((CommonConfig, StorageConfigs)),
    /// Loaded mode: legacy resolution (divergence stats + knob routing).
    Resolve(Option<Result<(CommonConfig, StorageConfigs)>>),
}

/// One reload pass. Everything fallible returns BEFORE the commit section, so a failed pass never mixes sources.
#[expect(
    clippy::too_many_arguments,
    reason = "pass body extracted verbatim from the watcher loop for testability; \
              the arguments are the loop's captured environment"
)]
async fn run_reload_pass(
    blob_source: &BlobSource,
    manifest_handle: Option<&ConfigHandle<TierManifest>>,
    repo_handles: &RwLock<HashMap<String, ConfigHandle<RepoSpec>>>,
    config_store: &ConfigStore,
    tier_name: Option<&str>,
    repo_configs: &Swappable<RepoConfigs>,
    storage_configs: &Swappable<StorageConfigs>,
    config_info: &Swappable<Option<ConfigInfo>>,
    update_receivers: &Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>>,
    state: &mut ReloadState,
    prev_specs: &mut HashMap<String, Arc<RepoSpec>>,
    per_repo_wait_futures: &mut FuturesUnordered<PerRepoFuture>,
) {
    let current_blob = match blob_source {
        BlobSource::Loaded(handle) => Some(handle.get()),
        BlobSource::Skipped => None,
    };
    let current_manifest = manifest_handle.map(|h| h.get());

    let blob_changed = content_changed(&state.prev_blob, &current_blob);
    let manifest_changed = content_changed(&state.prev_manifest, &current_manifest);

    let use_manifest = effective_use_manifest(blob_source);
    let source_changed = state.last_use_manifest != Some(use_manifest);

    if !blob_changed && !manifest_changed && !source_changed {
        STATS::spurious_reload_suppressed.add_value(1);
        debug!("Config version bumped but content identical, skipping reload");
        return;
    }

    info!(
        "Config content changed (blob={blob_changed}, manifest={manifest_changed}, source={source_changed}), applying update",
    );

    // Single parse site feeding both arms; computed pre-mutation so skip mode can fail closed.
    let manifest_parsed = current_manifest
        .as_deref()
        .map(parse_manifest_common_and_storage);

    // Skipped: bail before any state mutation so the retry survives the dedup; Loaded: blob fallback.
    let manifest_values = match (blob_source, manifest_parsed) {
        (BlobSource::Skipped, Some(Ok(parsed))) => ManifestValues::Authoritative(parsed),
        (BlobSource::Skipped, Some(Err(e))) => {
            error!(
                "Failed to parse common/storage from tier manifest in skip mode, \
                 keeping last known good config: {e:?}"
            );
            STATS::manifest_common_parse_failure.add_value(1);
            STATS::refresh_failure_count.add_value(1);
            return;
        }
        (BlobSource::Skipped, None) => {
            // Unreachable (skip mode requires a manifest handle); keep last known good, don't panic.
            error!("No tier manifest available in skip mode, keeping last known good config");
            STATS::refresh_failure_count.add_value(1);
            return;
        }
        (BlobSource::Loaded(_), parsed) => ManifestValues::Resolve(parsed),
    };

    if blob_changed {
        if let Some(ref raw) = current_blob {
            match load_configs_from_raw(Arc::unwrap_or_clone(raw.clone())) {
                Ok((configs, new_storage)) => {
                    state.cached_blob_storage = Some(Arc::new(new_storage));
                    match build_config_info(raw.clone()) {
                        Ok(info) => config_info.store(Arc::new(Some(info))),
                        Err(e) => warn!("Could not compute new config_info: {e:?}"),
                    }
                    state.cached_parsed = Some(configs);
                }
                Err(e) => {
                    error!("Failed to parse blob config: {e:?}");
                    STATS::refresh_failure_count.add_value(1);
                    return;
                }
            }
        } else {
            state.cached_parsed = None;
        }
        state.prev_blob = current_blob;
    }

    if manifest_changed {
        if let Some(ref manifest) = current_manifest {
            match sync_repo_handles(manifest, repo_handles, config_store) {
                Ok(new_watchers) => {
                    // Register new watchers so per-repo changes propagate without a bulk reload.
                    for (name, watcher) in new_watchers {
                        push_per_repo_watcher(
                            name,
                            watcher,
                            repo_handles,
                            prev_specs,
                            per_repo_wait_futures,
                        );
                    }
                }
                Err(e) => {
                    // Don't advance prev_manifest: transient failures retry on the next fire.
                    error!("Failed to sync repo handles: {e:?}");
                    STATS::refresh_failure_count.add_value(1);
                    return;
                }
            }
        }
        state.prev_manifest = current_manifest;
    }

    let base = state
        .cached_parsed
        .clone()
        .unwrap_or_else(|| RepoConfigs::new(HashMap::new(), CommonConfig::default()));

    // Resolve only -- committing before the `return`s below could mix sources.
    let routed = match manifest_values {
        // Authoritative bypasses use_manifest_source() — then_some would default common on failure.
        ManifestValues::Authoritative(parsed) => Some(parsed),
        ManifestValues::Resolve(parsed) => resolve_manifest_common_and_storage(
            parsed,
            &base.common,
            state.cached_blob_storage.as_deref(),
            use_manifest,
        ),
    };
    let (resolved_common, resolved_storage) = match routed {
        Some((common, storage)) => {
            let storage = match state.cached_manifest_storage.take() {
                Some(cached) if *cached == storage => cached,
                _ => Arc::new(storage),
            };
            state.cached_manifest_storage = Some(storage.clone());
            (common, Some(storage))
        }
        None => {
            state.cached_manifest_storage = None;
            (base.common.clone(), state.cached_blob_storage.clone())
        }
    };

    let merged = match (&state.prev_manifest, tier_name) {
        (Some(manifest), Some(tier)) => {
            let handles = match repo_handles.read() {
                Ok(h) => h,
                Err(e) => {
                    error!("Failed to read repo handles lock: {e:?}");
                    STATS::refresh_failure_count.add_value(1);
                    return;
                }
            };
            // Served snapshot for per-repo keep-last-known-good below.
            let served = repo_configs.load_full();
            // O(1) clone of the persistent map (structural sharing).
            let mut repos = base.repos.clone();
            for entry in &manifest.repos {
                if let Some(handle) = handles.get(&entry.repo_name) {
                    let spec = handle.get();
                    match parse_repo_spec(Arc::unwrap_or_clone(spec), tier, &manifest.storage) {
                        Ok(config) => {
                            repos.insert(entry.repo_name.clone(), Arc::new(config));
                        }
                        Err(e) => {
                            error!(
                                "Failed to parse RepoSpec for repo '{}', keeping last \
                                 served config if any: {e:?}",
                                entry.repo_name,
                            );
                            STATS::per_repo_refresh_failure_count.add_value(1);
                            // Keep-last-known-good: served entry wins over blob-base on parse
                            // failure — never drop the repo (skip-mode base.repos is empty).
                            if let Some(served_config) = served.repos.get(&entry.repo_name) {
                                repos.insert(entry.repo_name.clone(), served_config.clone());
                            }
                        }
                    }
                } else {
                    STATS::merge_skipped_no_handle.add_value(1);
                }
            }
            // from_arc_map rebuilds repos_by_id from the merged map.
            RepoConfigs::from_arc_map(repos, resolved_common)
        }
        _ => RepoConfigs {
            common: resolved_common,
            ..base
        },
    };

    // Commit only now: everything above can `return`.
    state.last_use_manifest = Some(use_manifest);
    if let Some(storage) = resolved_storage {
        storage_configs.store(storage);
    }

    let new_configs = Arc::new(merged);
    repo_configs.store(new_configs.clone());
    let current_storage = storage_configs.load_full();
    let receivers = update_receivers.load();
    let results = join_all(
        receivers
            .iter()
            .map(|r| r.apply_update(new_configs.clone(), current_storage.clone())),
    )
    .await;
    let had_failure = results.iter().any(|r| r.is_err());
    for (i, result) in results.iter().enumerate() {
        if let Err(e) = result {
            error!("Config update receiver {i} failed: {e:?}");
        }
    }
    if had_failure {
        STATS::refresh_failure_count.add_value(1);
    } else {
        info!("Successfully applied config update");
        STATS::refresh_success_count.add_value(1);
        // Keep the timeseries alive for OneDetection alerting
        STATS::refresh_failure_count.add_value(0);
    }
}

/// Body of the per-repo `select!` arm — extracted so the main loop reads as
/// straight-line orchestration rather than nested control flow.
///
/// Pure with respect to global state (everything is passed by argument) so
/// the per-repo dispatch logic can be unit-tested directly without spinning
/// up the watcher's full `select!` loop. See `tests::handle_per_repo_fire_*`.
///
/// Note on `result: Err`: `wait_for_next` wraps `tokio::sync::watch::Receiver`,
/// which only errors when the corresponding `watch::Sender` is dropped. The
/// only path that drops the Sender (without dropping the whole process) is
/// `remove_repo_config_handle` removing the handle from `repo_handles`. So
/// `Err` always means "handle gone, don't re-push." There is no transient
/// error class to retry against — unlike `broadcast::Receiver` there is no
/// `Lagged` variant.
#[allow(clippy::too_many_arguments)]
async fn handle_per_repo_fire(
    name: String,
    result: Result<Arc<RepoSpec>>,
    watcher: ConfigUpdateWatcher<RepoSpec>,
    repo_handles: &RwLock<HashMap<String, ConfigHandle<RepoSpec>>>,
    tier_name: Option<&str>,
    prev_manifest: Option<&TierManifest>,
    repo_configs: &Swappable<RepoConfigs>,
    update_receivers: &Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>>,
    per_repo_wait_futures: &mut FuturesUnordered<PerRepoFuture>,
    prev_specs: &mut HashMap<String, Arc<RepoSpec>>,
) {
    // Handle removed concurrently by remove_repo_config_handle (which drops
    // the ConfigHandle, closing the watcher channel). Don't re-push.
    let still_present = match repo_handles.read() {
        Ok(h) => h.contains_key(&name),
        Err(e) => {
            error!("repo_handles lock poisoned dispatching per-repo update for {name}: {e:?}");
            STATS::per_repo_refresh_failure_count.add_value(1);
            return;
        }
    };
    if !still_present {
        debug!("Per-repo watcher fired for absent repo {name}, dropping");
        prev_specs.remove(&name);
        return;
    }

    let spec = match result {
        Ok(s) => s,
        Err(e) => {
            // Sender closed: handle dropped. Don't re-push.
            debug!("Per-repo watcher for {name} closed: {e:?}");
            prev_specs.remove(&name);
            return;
        }
    };

    // Skip spurious version bumps: identical RepoSpec content -> no parse/rebuild
    // (repo_factory::build re-preloads the commit graph). Raw compare.
    if !spec_content_changed(prev_specs.get(&name), &spec) {
        STATS::spurious_reload_suppressed.add_value(1);
        debug!("Per-repo config content unchanged for {name}, skipping reload");
        // Repoint at the new Arc (same content) to release the old allocation
        // and keep sharing storage with the live handle instead of pinning a dup.
        prev_specs.insert(name.clone(), spec);
        per_repo_wait_futures.push(Box::pin(wait_one(name, watcher)));
        return;
    }

    let Some(tier) = tier_name else {
        error!("Per-repo watcher fired without tier_name set (repo {name}); skipping");
        STATS::per_repo_refresh_failure_count.add_value(1);
        per_repo_wait_futures.push(Box::pin(wait_one(name, watcher)));
        return;
    };
    let Some(manifest_for_storage) = prev_manifest else {
        // Manifest watcher hasn't fired yet — we have no storage_config to use
        // when parsing. Skip; the next manifest fire will trigger a bulk reload
        // that picks up the new spec.
        debug!(
            "Per-repo watcher fired for {name} before manifest_watcher; deferring to bulk reload"
        );
        per_repo_wait_futures.push(Box::pin(wait_one(name, watcher)));
        return;
    };

    // Cheap Arc clone; parse_repo_spec consumes the original below.
    let applied_spec = spec.clone();
    let new_config = match parse_repo_spec(
        Arc::unwrap_or_clone(spec),
        tier,
        &manifest_for_storage.storage,
    ) {
        Ok(c) => c,
        Err(e) => {
            error!("Failed to parse RepoSpec for {name}: {e:?}");
            STATS::per_repo_refresh_failure_count.add_value(1);
            per_repo_wait_futures.push(Box::pin(wait_one(name, watcher)));
            return;
        }
    };

    info!("Per-repo config refresh: {name}");
    let succeeded = apply_per_repo_update(&name, new_config, repo_configs, update_receivers).await;
    if succeeded {
        STATS::per_repo_refresh_count.add_value(1);
        // Record applied content for dedup, but only once a receiver exists to act
        // on it. The watcher runs before receivers register at startup; advancing
        // then would dedup away the healing reload that fires once they do, leaving
        // the repo on stale config. Failures don't advance either, so they retry.
        if !update_receivers.load().is_empty() {
            prev_specs.insert(name.clone(), applied_spec);
        }
    } else {
        STATS::per_repo_refresh_failure_count.add_value(1);
    }
    // Re-push so we observe the next update for this watcher.
    per_repo_wait_futures.push(Box::pin(wait_one(name, watcher)));
}

/// Adds preload handles for new non-deep-sharded entries; removes handles
/// whose repo is gone from the manifest. The add/remove filters are
/// asymmetric on purpose: deep-sharded repos are loaded on-demand by
/// ShardManager via `load_repo_config_handle` and must survive manifest
/// refreshes.
///
/// Returns the list of `(name, watcher)` pairs that were just installed, so
/// the caller (`unified_config_watcher`) can register them with its per-repo
/// `FuturesUnordered` set. Watchers for repos that produced an `Err` from
/// `handle.watcher()` (i.e. static configs in test fixtures) log a warning
/// and are skipped — they have no live channel to observe anyway, but the
/// dataloss is observable in production via the warn log.
pub(crate) fn sync_repo_handles(
    manifest: &TierManifest,
    repo_handles: &RwLock<HashMap<String, ConfigHandle<RepoSpec>>>,
    config_store: &ConfigStore,
) -> Result<Vec<(String, ConfigUpdateWatcher<RepoSpec>)>> {
    let current_repos: HashSet<String> = repo_handles
        .read()
        .map_err(|e| anyhow!("repo_handles lock poisoned: {e}"))?
        .keys()
        .cloned()
        .collect();

    let to_remove = compute_handles_to_remove(&current_repos, manifest);

    let new_handles: Vec<_> = manifest
        .repos
        .iter()
        .filter(|entry| !entry.is_deep_sharded && !current_repos.contains(&entry.repo_name))
        .filter_map(
            |entry| match configerator_repo_spec_handle(&entry.config_path, config_store) {
                Ok(handle) => {
                    info!("Added config handle for new repo: {}", entry.repo_name);
                    Some((entry.repo_name.clone(), handle))
                }
                Err(e) => {
                    error!("Failed to load config for {}: {e:?}", entry.repo_name);
                    STATS::refresh_failure_count.add_value(1);
                    None
                }
            },
        )
        .collect();

    // Derive watchers BEFORE handing handle ownership to the HashMap. Log
    // any watcher() failure since it disables per-repo hot-reload for that
    // repo until the next manifest refresh re-adds it.
    let new_watchers: Vec<_> = new_handles
        .iter()
        .filter_map(|(name, handle)| match handle.watcher() {
            Ok(w) => Some((name.clone(), w)),
            Err(e) => {
                warn!(
                    "sync_repo_handles: failed to create watcher for {name}, \
                     per-repo hot-reload disabled until next manifest refresh: {e:?}",
                );
                None
            }
        })
        .collect();

    if !new_handles.is_empty() || !to_remove.is_empty() {
        let mut handles = repo_handles
            .write()
            .map_err(|e| anyhow!("repo_handles lock poisoned: {e}"))?;
        handles.extend(new_handles);
        for repo_name in &to_remove {
            handles.remove(repo_name);
            info!("Removed config handle for repo: {repo_name}");
        }
    }

    Ok(new_watchers)
}

/// Names in `current_repos` no longer present in the manifest. Pure helper
/// extracted to make the diff testable without a `ConfigStore`.
fn compute_handles_to_remove(
    current_repos: &HashSet<String>,
    manifest: &TierManifest,
) -> Vec<String> {
    let manifest_repo_names: HashSet<&str> = manifest
        .repos
        .iter()
        .map(|e| e.repo_name.as_str())
        .collect();
    current_repos
        .iter()
        .filter(|name| !manifest_repo_names.contains(name.as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests;
