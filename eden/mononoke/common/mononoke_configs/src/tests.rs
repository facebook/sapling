/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Unit tests for `MononokeConfigs`. `mod tests;` submodule so `super` is the
//! crate root and private items stay visible (as the inline module was).

use std::time::Duration;

use cached_config::ModificationTime;
use cached_config::TestSource;
use metaconfig_types::CommonConfig;
use mononoke_macros::mononoke;
use repos::TierRepoEntry;

use super::*;

fn empty_configs() -> MononokeConfigs {
    MononokeConfigs {
        repo_configs: Arc::new(ArcSwap::from_pointee(RepoConfigs::new(
            HashMap::new(),
            CommonConfig::default(),
        ))),
        storage_configs: Arc::new(ArcSwap::from_pointee(StorageConfigs {
            storage: HashMap::new(),
        })),
        update_receivers: Arc::new(ArcSwap::from_pointee(vec![])),
        config_info: Arc::new(ArcSwap::from_pointee(None)),
        maybe_config_updater: None,
        maybe_liveness_updater: None,
        maybe_config_handle: None,
        maybe_manifest_handle: None,
        repo_handles: Arc::new(RwLock::new(HashMap::new())),
        config_store: None,
        tier_name: None,
        repo_handle_event_tx: None,
    }
}

fn static_handle() -> ConfigHandle<RepoSpec> {
    ConfigHandle::from_json("{}").expect("RepoSpec::default serializes as {}")
}

fn make_store(entries: &[(&str, &str)]) -> ConfigStore {
    let source = TestSource::new();
    for (path, content) in entries {
        source.insert_config(path, content, ModificationTime::UnixTimestamp(0));
    }
    ConfigStore::new(Arc::new(source), Duration::from_secs(1), None)
}

fn configs_with_manifest(
    manifest_path: &str,
    entries: Vec<TierRepoEntry>,
    extra_paths: &[(&str, &str)],
) -> MononokeConfigs {
    let manifest = TierManifest {
        repos: entries,
        ..Default::default()
    };
    let manifest_json = serde_json::to_string(&manifest).unwrap();

    let mut all = vec![(manifest_path, manifest_json.as_str())];
    all.extend_from_slice(extra_paths);
    let store = make_store(&all);

    let mut cfg = empty_configs();
    cfg.maybe_manifest_handle = Some(
        store
            .get_config_handle::<TierManifest>(manifest_path.to_string())
            .unwrap(),
    );
    cfg.config_store = Some(store);
    cfg
}

#[mononoke::test]
fn test_ensure_repo_config_handle_no_manifest_returns_ok() {
    let cfg = empty_configs();
    assert!(cfg.ensure_repo_config_handle("any_repo").is_ok());
    assert!(cfg.repo_handles.read().unwrap().is_empty());
}

#[mononoke::test]
fn test_ensure_repo_config_handle_idempotent_when_already_present() {
    let cfg = empty_configs();
    cfg.repo_handles
        .write()
        .unwrap()
        .insert("existing".to_string(), static_handle());
    assert!(cfg.ensure_repo_config_handle("existing").is_ok());
    // Should not have created a duplicate or attempted manifest lookup.
    assert_eq!(cfg.repo_handles.read().unwrap().len(), 1);
}

#[mononoke::test]
fn test_ensure_repo_config_handle_not_in_manifest_returns_ok() {
    let cfg = configs_with_manifest(
        "test/manifest",
        vec![TierRepoEntry {
            repo_name: "other_repo".to_string(),
            ..Default::default()
        }],
        &[],
    );
    assert!(cfg.ensure_repo_config_handle("missing_repo").is_ok());
    // Repo not in manifest -> no handle registered (legacy-only path).
    assert!(
        cfg.repo_handles
            .read()
            .unwrap()
            .get("missing_repo")
            .is_none()
    );
}

#[mononoke::test]
fn test_ensure_repo_config_handle_registers_when_in_manifest() {
    let repo_cfg_path = "test/repos/aosp_manifest";
    let cfg = configs_with_manifest(
        "test/manifest",
        vec![TierRepoEntry {
            repo_name: "aosp/manifest".to_string(),
            repo_id: 42,
            config_path: repo_cfg_path.to_string(),
            is_deep_sharded: true,
            ..Default::default()
        }],
        &[(repo_cfg_path, "{}")],
    );

    assert!(cfg.ensure_repo_config_handle("aosp/manifest").is_ok());
    // Bug repro: deep-sharded repo in manifest gets a handle registered
    // by ensure_repo_config_handle. This is the registration that S678887
    // relied on but never happened because get_or_load_repo_config's
    // fast path skipped it.
    assert!(
        cfg.repo_handles
            .read()
            .unwrap()
            .get("aosp/manifest")
            .is_some()
    );

    // Idempotency: second call is a no-op fast path.
    assert!(cfg.ensure_repo_config_handle("aosp/manifest").is_ok());
    assert_eq!(cfg.repo_handles.read().unwrap().len(), 1);
}
