/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use metaconfig_parser::RepoConfigs;
use metaconfig_parser::StorageConfigs;
use metaconfig_types::CommonConfig;
use metaconfig_types::RepoConfig;
use metaconfig_types::ShardedService;
use metaconfig_types::ShardingModeConfig;
use mononoke_configs::ConfigUpdateReceiver;
use mononoke_macros::mononoke;
use tokio::sync::Notify;

use super::ReconcileTrigger;
use super::compute_reloadable_repos;
use super::filter_repos_with_changed_config;
use super::should_reload_single_repo;
use super::tick_interval_secs;

/// Helper to create a RepoConfig with the specified enabled state and sharding config
fn make_repo_config(enabled: bool, deep_sharding_config: Option<ShardingModeConfig>) -> RepoConfig {
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

    let result = compute_reloadable_repos(&repo_configs, None, existing_repos(&["existing_repo"]));
    assert_eq!(get_repo_names(&result), vec!["existing_repo"]);
}

#[mononoke::test]
fn test_existing_disabled_repo_still_reloaded() {
    // Even disabled repos should be reloaded if they're already on the server
    let repo_configs = make_repo_configs(vec![(
        "existing_repo".to_string(),
        make_repo_config(false, None),
    )]);

    let result = compute_reloadable_repos(&repo_configs, None, existing_repos(&["existing_repo"]));
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

#[mononoke::test]
fn test_should_reload_single_repo() {
    // Enabled + served -> rebuild.
    assert!(should_reload_single_repo(true, true));
    // Not served on this host -> skip (don't rebuild a repo we don't serve
    // even though a per-repo watcher fired for it).
    assert!(!should_reload_single_repo(true, false));
    // Disabled -> skip regardless of serving.
    assert!(!should_reload_single_repo(false, true));
    assert!(!should_reload_single_repo(false, false));
}

#[mononoke::test]
fn test_tick_interval_off_uses_fixed_backstop() {
    // Off: fixed 60s, tunable value ignored (knob may be unregistered).
    assert_eq!(tick_interval_secs(false, 5), 60);
    assert_eq!(tick_interval_secs(false, 0), 60);
}

#[mononoke::test]
fn test_tick_interval_on_honors_knob_and_floors_zero() {
    assert_eq!(tick_interval_secs(true, 30), 30);
    assert_eq!(tick_interval_secs(true, 1), 1);
    // 0 must floor to 1s, never a busy loop.
    assert_eq!(tick_interval_secs(true, 0), 1);
}

#[mononoke::test]
async fn test_reconcile_trigger_wakes_on_bulk_update() {
    let notify = Arc::new(Notify::new());
    let trigger = ReconcileTrigger {
        notify: notify.clone(),
    };
    trigger
        .apply_update(
            Arc::new(RepoConfigs::new(HashMap::new(), CommonConfig::default())),
            Arc::new(StorageConfigs {
                storage: HashMap::new(),
            }),
        )
        .await
        .expect("apply_update is infallible");
    // notify_one leaves a permit, so notified() resolves at once; the timeout
    // only guards against a hang if the trigger failed to fire.
    tokio::time::timeout(Duration::from_secs(30), notify.notified())
        .await
        .expect("a bulk config update must wake the reconcile loop");
}

#[mononoke::test]
async fn test_reconcile_trigger_wakes_on_per_repo_update() {
    let notify = Arc::new(Notify::new());
    let trigger = ReconcileTrigger {
        notify: notify.clone(),
    };
    trigger
        .apply_repo_update("some_repo", &RepoConfig::default())
        .await
        .expect("apply_repo_update is infallible");
    tokio::time::timeout(Duration::from_secs(30), notify.notified())
        .await
        .expect("a per-repo config update must wake the reconcile loop");
}
