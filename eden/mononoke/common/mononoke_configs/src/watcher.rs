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
//! 1. **Legacy blob** `ConfigHandle<RawRepoConfigs>` — fires on tier-blob changes
//! 2. **Tier manifest** `ConfigHandle<TierManifest>` — fires on manifest changes
//!    (repo add/remove, sharding mode flips)
//! 3. **Per-repo control channel** `mpsc::UnboundedReceiver<RepoHandleEvent>` —
//!    notifies the loop when a new per-repo `ConfigHandle<RepoSpec>` is installed
//!    by `MononokeConfigs::new` (pre-load) or `load_repo_config_handle`
//!    (ShardManager on_add_shard)
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

const LIVENESS_INTERVAL: Duration = Duration::from_secs(300);

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
        next.repos.insert(repo_name.to_owned(), new_config.clone());
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
    blob_handle: Option<ConfigHandle<RawRepoConfigs>>,
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
    let mut blob_watcher = blob_handle
        .as_ref()
        .map(|h| h.watcher())
        .transpose()
        .unwrap_or_else(|e| {
            error!("Failed to create blob config watcher: {e:?}");
            None
        });
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

    let mut prev_blob: Option<Arc<RawRepoConfigs>> = None;
    let mut prev_manifest: Option<Arc<TierManifest>> = None;
    let mut cached_parsed: Option<RepoConfigs> = None;

    // Last-applied RepoSpec per repo; lets the per-repo arm skip spurious
    // identical-content reloads. Seeded at registration.
    let mut prev_specs: HashMap<String, Arc<RepoSpec>> = HashMap::new();

    // Per-repo watcher set. Populated entirely through the control-channel
    // arm — `MononokeConfigs::new` enqueues `Added` events for pre-loaded
    // handles before the watcher task starts, and `load_repo_config_handle`
    // enqueues for on-demand handles. Routing every registration through the
    // single channel avoids the duplicate-registration race that a separate
    // seed loop would create against a concurrent `load_repo_config_handle`.
    let mut per_repo_wait_futures: FuturesUnordered<PerRepoFuture> = FuturesUnordered::new();

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
                    prev_manifest.as_deref(),
                    &repo_configs,
                    &update_receivers,
                    &mut per_repo_wait_futures,
                    &mut prev_specs,
                ).await;
                continue;
            }
        }

        let current_blob = blob_handle.as_ref().map(|h| h.get());
        let current_manifest = manifest_handle.as_ref().map(|h| h.get());

        let blob_changed = content_changed(&prev_blob, &current_blob);
        let manifest_changed = content_changed(&prev_manifest, &current_manifest);

        if !blob_changed && !manifest_changed {
            STATS::spurious_reload_suppressed.add_value(1);
            debug!("Config version bumped but content identical, skipping reload");
            continue;
        }

        info!(
            "Config content changed (blob={blob_changed}, manifest={manifest_changed}), applying update",
        );

        if blob_changed {
            if let Some(ref raw) = current_blob {
                match load_configs_from_raw(Arc::unwrap_or_clone(raw.clone())) {
                    Ok((configs, new_storage)) => {
                        storage_configs.store(Arc::new(new_storage));
                        match build_config_info(raw.clone()) {
                            Ok(info) => config_info.store(Arc::new(Some(info))),
                            Err(e) => warn!("Could not compute new config_info: {e:?}"),
                        }
                        cached_parsed = Some(configs);
                    }
                    Err(e) => {
                        error!("Failed to parse blob config: {e:?}");
                        STATS::refresh_failure_count.add_value(1);
                        continue;
                    }
                }
            } else {
                cached_parsed = None;
            }
            prev_blob = current_blob;
        }

        if manifest_changed {
            if let Some(ref manifest) = current_manifest {
                match sync_repo_handles(manifest, &repo_handles, &config_store) {
                    Ok(new_watchers) => {
                        // Register the watchers from any newly-added handles so
                        // per-repo content changes for them propagate without
                        // waiting for the next bulk reload.
                        for (name, watcher) in new_watchers {
                            push_per_repo_watcher(
                                name,
                                watcher,
                                &repo_handles,
                                &mut prev_specs,
                                &mut per_repo_wait_futures,
                            );
                        }
                    }
                    Err(e) => {
                        // Don't update prev_manifest so we retry on the next watcher cycle.
                        // Transient failures (e.g., configerator timeout for a new repo
                        // handle) will self-heal on the next notification.
                        error!("Failed to sync repo handles: {e:?}");
                        STATS::refresh_failure_count.add_value(1);
                        continue;
                    }
                }
            }
            prev_manifest = current_manifest;
        }

        let base = cached_parsed
            .clone()
            .unwrap_or_else(|| RepoConfigs::new(HashMap::new(), CommonConfig::default()));

        let merged = match (&prev_manifest, tier_name.as_deref()) {
            (Some(manifest), Some(tier)) => {
                let handles = match repo_handles.read() {
                    Ok(h) => h,
                    Err(e) => {
                        error!("Failed to read repo handles lock: {e:?}");
                        STATS::refresh_failure_count.add_value(1);
                        continue;
                    }
                };
                let mut repos = base.repos.clone();
                for entry in &manifest.repos {
                    if let Some(handle) = handles.get(&entry.repo_name) {
                        let spec = handle.get();
                        match parse_repo_spec(Arc::unwrap_or_clone(spec), tier, &manifest.storage) {
                            Ok(config) => {
                                repos.insert(entry.repo_name.clone(), config);
                            }
                            Err(e) => {
                                error!(
                                    "Failed to parse RepoSpec for repo '{}', skipping: {e:?}",
                                    entry.repo_name,
                                );
                            }
                        }
                    } else {
                        STATS::merge_skipped_no_handle.add_value(1);
                    }
                }
                RepoConfigs::new(repos, base.common.clone())
            }
            _ => base,
        };

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
mod tests {
    use anyhow::anyhow;
    use arc_swap::ArcSwap;
    use async_trait::async_trait;
    use mononoke_macros::mononoke;
    use repos::TierRepoEntry;

    use super::*;

    fn tier_entry(name: &str, is_deep_sharded: bool) -> TierRepoEntry {
        TierRepoEntry {
            repo_name: name.to_owned(),
            is_deep_sharded,
            ..Default::default()
        }
    }

    fn manifest_with(entries: Vec<TierRepoEntry>) -> TierManifest {
        TierManifest {
            repos: entries,
            ..Default::default()
        }
    }

    // Regression: deep-sharded handles inserted on-demand by ShardManager
    // must survive manifest refresh. See D106658358.
    #[mononoke::test]
    fn test_compute_handles_to_remove_preserves_deep_sharded() {
        let manifest = manifest_with(vec![
            tier_entry("non_sharded_repo", false),
            tier_entry("deep_sharded_repo", true),
        ]);
        let current: HashSet<String> = ["non_sharded_repo", "deep_sharded_repo"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let to_remove = compute_handles_to_remove(&current, &manifest);
        assert!(
            to_remove.is_empty(),
            "deep-sharded repo present in manifest must not be removed, got {to_remove:?}",
        );
    }

    #[mononoke::test]
    fn test_compute_handles_to_remove_drops_repos_missing_from_manifest() {
        let manifest = manifest_with(vec![tier_entry("still_present", true)]);
        let current: HashSet<String> = ["still_present", "gone_from_manifest"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let to_remove = compute_handles_to_remove(&current, &manifest);
        assert_eq!(
            to_remove,
            vec!["gone_from_manifest".to_string()],
            "only entries absent from manifest should be removed",
        );
    }

    #[mononoke::test]
    fn test_compute_handles_to_remove_empty_manifest() {
        let manifest = manifest_with(vec![]);
        let current: HashSet<String> = ["a", "b"].iter().map(|s| s.to_string()).collect();
        let mut to_remove = compute_handles_to_remove(&current, &manifest);
        to_remove.sort();
        assert_eq!(to_remove, vec!["a".to_string(), "b".to_string()]);
    }

    #[mononoke::test]
    fn test_compute_handles_to_remove_empty_current() {
        let manifest = manifest_with(vec![tier_entry("a", false), tier_entry("b", true)]);
        let current: HashSet<String> = HashSet::new();
        let to_remove = compute_handles_to_remove(&current, &manifest);
        assert!(to_remove.is_empty());
    }

    /// Records every `apply_repo_update` call for assertion. Tracks the
    /// snapshot of the bulk `RepoConfigs` Arc observed at the moment of the
    /// call so tests can verify the rcu-patch-THEN-receiver ordering.
    struct RecordingReceiver {
        repo_configs: Swappable<RepoConfigs>,
        calls: tokio::sync::Mutex<Vec<RecordedCall>>,
    }

    #[derive(Clone)]
    struct RecordedCall {
        repo_name: String,
        repo_config: RepoConfig,
        bulk_arc_snapshot: Arc<RepoConfigs>,
    }

    #[async_trait]
    impl ConfigUpdateReceiver for RecordingReceiver {
        async fn apply_update(
            &self,
            _repo_configs: Arc<RepoConfigs>,
            _storage_configs: Arc<StorageConfigs>,
        ) -> Result<()> {
            Ok(())
        }

        async fn apply_repo_update(&self, repo_name: &str, repo_config: &RepoConfig) -> Result<()> {
            self.calls.lock().await.push(RecordedCall {
                repo_name: repo_name.to_owned(),
                repo_config: repo_config.clone(),
                bulk_arc_snapshot: self.repo_configs.load_full(),
            });
            Ok(())
        }
    }

    fn empty_repo_configs() -> Arc<ArcSwap<RepoConfigs>> {
        Arc::new(ArcSwap::from_pointee(RepoConfigs::new(
            HashMap::new(),
            CommonConfig::default(),
        )))
    }

    fn repo_config_with_id(id: i32) -> RepoConfig {
        RepoConfig {
            repoid: mononoke_types::RepositoryId::new(id),
            ..Default::default()
        }
    }

    // Verifies (b): apply_repo_update is called on every registered receiver
    // with the correct repo name and config.
    #[mononoke::test]
    async fn test_apply_per_repo_update_calls_receivers() {
        let repo_configs = empty_repo_configs();
        let receiver = Arc::new(RecordingReceiver {
            repo_configs: repo_configs.clone(),
            calls: tokio::sync::Mutex::new(Vec::new()),
        });
        let receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>> =
            Arc::new(ArcSwap::from_pointee(vec![
                receiver.clone() as Arc<dyn ConfigUpdateReceiver>
            ]));

        let succeeded =
            apply_per_repo_update("foo", repo_config_with_id(42), &repo_configs, &receivers).await;
        assert!(succeeded);

        let calls = receiver.calls.lock().await;
        assert_eq!(
            calls.len(),
            1,
            "exactly one apply_repo_update call expected"
        );
        assert_eq!(calls[0].repo_name, "foo");
        assert_eq!(calls[0].repo_config.repoid.id(), 42);
    }

    // Verifies (a): the bulk RepoConfigs Arc is patched with the new config.
    #[mononoke::test]
    async fn test_apply_per_repo_update_patches_bulk_arc() {
        let repo_configs = empty_repo_configs();
        let receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>> =
            Arc::new(ArcSwap::from_pointee(vec![]));

        let succeeded =
            apply_per_repo_update("foo", repo_config_with_id(7), &repo_configs, &receivers).await;
        assert!(succeeded);

        let after = repo_configs.load();
        let stored = after
            .repos
            .get("foo")
            .expect("foo should be in bulk Arc after per-repo apply");
        assert_eq!(stored.repoid.id(), 7);
    }

    // Verifies the ordering invariant: the bulk Arc is patched BEFORE the
    // receiver is notified.
    #[mononoke::test]
    async fn test_apply_per_repo_update_arc_patched_before_receiver_called() {
        let repo_configs = empty_repo_configs();
        let receiver = Arc::new(RecordingReceiver {
            repo_configs: repo_configs.clone(),
            calls: tokio::sync::Mutex::new(Vec::new()),
        });
        let receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>> =
            Arc::new(ArcSwap::from_pointee(vec![
                receiver.clone() as Arc<dyn ConfigUpdateReceiver>
            ]));

        let succeeded =
            apply_per_repo_update("foo", repo_config_with_id(99), &repo_configs, &receivers).await;
        assert!(succeeded);

        let calls = receiver.calls.lock().await;
        let snapshot = &calls[0].bulk_arc_snapshot;
        let in_snapshot = snapshot
            .repos
            .get("foo")
            .expect("bulk Arc must contain new config BEFORE receiver is called");
        assert_eq!(
            in_snapshot.repoid.id(),
            99,
            "receiver must observe the new config in the bulk Arc",
        );
    }

    // Verifies update_receivers fan-out: every registered receiver sees the call.
    #[mononoke::test]
    async fn test_apply_per_repo_update_fans_out_to_all_receivers() {
        let repo_configs = empty_repo_configs();
        let r1 = Arc::new(RecordingReceiver {
            repo_configs: repo_configs.clone(),
            calls: tokio::sync::Mutex::new(Vec::new()),
        });
        let r2 = Arc::new(RecordingReceiver {
            repo_configs: repo_configs.clone(),
            calls: tokio::sync::Mutex::new(Vec::new()),
        });
        let receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>> =
            Arc::new(ArcSwap::from_pointee(vec![
                r1.clone() as Arc<dyn ConfigUpdateReceiver>,
                r2.clone() as Arc<dyn ConfigUpdateReceiver>,
            ]));

        let succeeded =
            apply_per_repo_update("bar", repo_config_with_id(11), &repo_configs, &receivers).await;
        assert!(succeeded);

        assert_eq!(r1.calls.lock().await.len(), 1, "first receiver called");
        assert_eq!(r2.calls.lock().await.len(), 1, "second receiver called");
    }

    // A receiver that errors out must not block other receivers from being
    // notified — and the overall return must reflect failure.
    struct FailingReceiver;

    #[async_trait]
    impl ConfigUpdateReceiver for FailingReceiver {
        async fn apply_update(
            &self,
            _repo_configs: Arc<RepoConfigs>,
            _storage_configs: Arc<StorageConfigs>,
        ) -> Result<()> {
            Ok(())
        }

        async fn apply_repo_update(
            &self,
            _repo_name: &str,
            _repo_config: &RepoConfig,
        ) -> Result<()> {
            Err(anyhow!("simulated receiver failure"))
        }
    }

    #[mononoke::test]
    async fn test_apply_per_repo_update_receiver_error_does_not_block_others() {
        let repo_configs = empty_repo_configs();
        let healthy = Arc::new(RecordingReceiver {
            repo_configs: repo_configs.clone(),
            calls: tokio::sync::Mutex::new(Vec::new()),
        });
        let failing = Arc::new(FailingReceiver);
        let receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>> =
            Arc::new(ArcSwap::from_pointee(vec![
                failing.clone() as Arc<dyn ConfigUpdateReceiver>,
                healthy.clone() as Arc<dyn ConfigUpdateReceiver>,
            ]));

        let succeeded =
            apply_per_repo_update("baz", repo_config_with_id(3), &repo_configs, &receivers).await;
        assert!(!succeeded, "must return false when any receiver fails");

        assert_eq!(
            healthy.calls.lock().await.len(),
            1,
            "healthy receiver must still be called after failing receiver errors",
        );
        // And the bulk Arc must still be patched.
        let stored = repo_configs.load();
        assert!(stored.repos.contains_key("baz"));
    }

    // -------------------------------------------------------------------------
    // handle_per_repo_fire orchestration tests
    //
    // These cover the per-repo arm orchestration in isolation: missing-handle,
    // missing-tier, missing-manifest, and the re-push contract.
    //
    // We can't construct real ConfigUpdateWatcher<RepoSpec> values in tests (no
    // public constructor), so these tests cover paths where `result: Err`
    // means we don't even reach the parse step — `wait_for_next` produced an
    // Err, the path early-returns without touching the watcher again. That
    // catches the still-present, tier-missing, manifest-missing, and parse-
    // failure paths without needing a watcher mock.
    // -------------------------------------------------------------------------

    fn make_repo_handles(names: &[&str]) -> Arc<RwLock<HashMap<String, ConfigHandle<RepoSpec>>>> {
        let map: HashMap<String, ConfigHandle<RepoSpec>> = names
            .iter()
            .map(|n| (n.to_string(), make_static_handle()))
            .collect();
        Arc::new(RwLock::new(map))
    }

    /// Static handle that can never produce a watcher. Used to populate
    /// `repo_handles` so the `still_present` check finds the entry but the
    /// test never needs to inject a real watcher fire.
    fn make_static_handle() -> ConfigHandle<RepoSpec> {
        ConfigHandle::from_json("{}").expect("RepoSpec::default serializes as {}")
    }

    // Orchestration tests for handle_per_repo_fire. We can't construct
    // ConfigUpdateWatcher<RepoSpec> from a static handle (`watcher()` returns
    // Err for `from_json`-built handles), so we spin up a ConfigStore +
    // TestSource and register a real handle for a dummy path to obtain a
    // live watcher value for the test fixtures.
    fn fresh_watcher() -> ConfigUpdateWatcher<RepoSpec> {
        let source = cached_config::TestSource::new();
        source.insert_config(
            "test/path",
            "{}",
            cached_config::ModificationTime::UnixTimestamp(0),
        );
        let store = cached_config::ConfigStore::new(Arc::new(source), Duration::from_secs(1), None);
        store
            .get_config_handle::<RepoSpec>("test/path".to_string())
            .expect("handle for inserted path")
            .watcher()
            .expect("registered handle has a watcher")
    }

    fn empty_receivers() -> Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>> {
        Arc::new(ArcSwap::from_pointee(vec![]))
    }

    // still_present=false → handle was removed between fire and dispatch
    // → drop the watcher, do NOT re-push.
    #[mononoke::test]
    async fn test_handle_per_repo_fire_drops_removed_repo() {
        let handles = make_repo_handles(&[]); // empty: "removed_repo" not present
        let configs = empty_repo_configs();
        let receivers = empty_receivers();
        let mut futs: FuturesUnordered<PerRepoFuture> = FuturesUnordered::new();
        let mut prev_specs: HashMap<String, Arc<RepoSpec>> = HashMap::new();

        handle_per_repo_fire(
            "removed_repo".to_string(),
            Ok(Arc::new(RepoSpec::default())),
            fresh_watcher(),
            &handles,
            Some("test_tier"),
            None,
            &configs,
            &receivers,
            &mut futs,
            &mut prev_specs,
        )
        .await;

        assert!(
            futs.is_empty(),
            "must not re-push for an absent repo (would leak the watcher subscription)",
        );
        assert!(
            !configs.load().repos.contains_key("removed_repo"),
            "bulk Arc must not be patched for an absent repo",
        );
    }

    // result=Err (handle dropped) → drop the watcher, do NOT re-push.
    #[mononoke::test]
    async fn test_handle_per_repo_fire_drops_on_err_result() {
        let handles = make_repo_handles(&["foo"]); // present
        let configs = empty_repo_configs();
        let receivers = empty_receivers();
        let mut futs: FuturesUnordered<PerRepoFuture> = FuturesUnordered::new();
        let mut prev_specs: HashMap<String, Arc<RepoSpec>> = HashMap::new();

        handle_per_repo_fire(
            "foo".to_string(),
            Err(anyhow!("simulated watch channel closed")),
            fresh_watcher(),
            &handles,
            Some("test_tier"),
            None,
            &configs,
            &receivers,
            &mut futs,
            &mut prev_specs,
        )
        .await;

        assert!(
            futs.is_empty(),
            "Err result must not re-push (sender gone, no future updates possible)",
        );
        assert!(
            !configs.load().repos.contains_key("foo"),
            "bulk Arc must not be patched on Err result",
        );
    }

    // tier_name=None → log+skip but re-push so watching continues in case
    // tier_name appears later.
    #[mononoke::test]
    async fn test_handle_per_repo_fire_repushes_when_tier_missing() {
        let handles = make_repo_handles(&["foo"]);
        let configs = empty_repo_configs();
        let receivers = empty_receivers();
        let mut futs: FuturesUnordered<PerRepoFuture> = FuturesUnordered::new();
        let mut prev_specs: HashMap<String, Arc<RepoSpec>> = HashMap::new();

        handle_per_repo_fire(
            "foo".to_string(),
            Ok(Arc::new(RepoSpec::default())),
            fresh_watcher(),
            &handles,
            None, // tier_name missing
            None,
            &configs,
            &receivers,
            &mut futs,
            &mut prev_specs,
        )
        .await;

        assert_eq!(
            futs.len(),
            1,
            "must re-push watcher when tier_name is missing so future fires are observed",
        );
    }

    // prev_manifest=None → log+skip but re-push (next manifest fire bulk-reloads anyway).
    #[mononoke::test]
    async fn test_handle_per_repo_fire_repushes_when_manifest_missing() {
        let handles = make_repo_handles(&["foo"]);
        let configs = empty_repo_configs();
        let receivers = empty_receivers();
        let mut futs: FuturesUnordered<PerRepoFuture> = FuturesUnordered::new();
        let mut prev_specs: HashMap<String, Arc<RepoSpec>> = HashMap::new();

        handle_per_repo_fire(
            "foo".to_string(),
            Ok(Arc::new(RepoSpec::default())),
            fresh_watcher(),
            &handles,
            Some("test_tier"),
            None, // manifest missing
            &configs,
            &receivers,
            &mut futs,
            &mut prev_specs,
        )
        .await;

        assert_eq!(
            futs.len(),
            1,
            "must re-push watcher when manifest is missing so future fires are observed",
        );
    }

    // None -> changed (first fire applies); identical -> unchanged; differing -> changed.
    #[mononoke::test]
    fn test_spec_content_changed() {
        let prev = Arc::new(RepoSpec::default());
        let identical = RepoSpec::default();
        let different = RepoSpec {
            repo_id: 42,
            ..Default::default()
        };

        assert!(
            spec_content_changed(None, &identical),
            "no recorded spec must be treated as changed so the first fire applies",
        );
        assert!(
            !spec_content_changed(Some(&prev), &identical),
            "identical RepoSpec content is a spurious version bump, not a change",
        );
        assert!(
            spec_content_changed(Some(&prev), &different),
            "a differing RepoSpec must be treated as changed",
        );
    }

    // Identical content -> no apply (no receiver call, bulk untouched), watcher
    // still re-pushed. tier+manifest present so only the dedup can short-circuit.
    #[mononoke::test]
    async fn test_handle_per_repo_fire_skips_unchanged_spec() {
        let handles = make_repo_handles(&["foo"]);
        let configs = empty_repo_configs();
        let receiver = Arc::new(RecordingReceiver {
            repo_configs: configs.clone(),
            calls: tokio::sync::Mutex::new(Vec::new()),
        });
        let receivers: Swappable<Vec<Arc<dyn ConfigUpdateReceiver>>> =
            Arc::new(ArcSwap::from_pointee(vec![
                receiver.clone() as Arc<dyn ConfigUpdateReceiver>
            ]));
        let mut futs: FuturesUnordered<PerRepoFuture> = FuturesUnordered::new();

        let spec = Arc::new(RepoSpec::default());
        let mut prev_specs: HashMap<String, Arc<RepoSpec>> = HashMap::new();
        prev_specs.insert("foo".to_string(), spec.clone());
        let manifest = manifest_with(vec![tier_entry("foo", false)]);

        handle_per_repo_fire(
            "foo".to_string(),
            Ok(spec.clone()), // identical content to the seeded prev_spec
            fresh_watcher(),
            &handles,
            Some("test_tier"),
            Some(&manifest),
            &configs,
            &receivers,
            &mut futs,
            &mut prev_specs,
        )
        .await;

        assert_eq!(
            receiver.calls.lock().await.len(),
            0,
            "identical content must not trigger a per-repo apply/rebuild",
        );
        assert!(
            !configs.load().repos.contains_key("foo"),
            "bulk Arc must not be patched when content is unchanged",
        );
        assert_eq!(
            futs.len(),
            1,
            "watcher must be re-pushed so future real changes are still observed",
        );
    }
}
