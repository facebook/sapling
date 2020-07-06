/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![deny(warnings)]

use cached_config::{ConfigStore, TestSource};
use context::CoreContext;
use fbinit::FacebookInit;
use live_commit_sync_config::{
    LiveCommitSyncConfig, CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS,
    CONFIGERATOR_CURRENT_COMMIT_SYNC_CONFIGS, CONFIGERATOR_PUSHREDIRECT_ENABLE,
};
use std::{sync::Arc, thread, time::Duration};

macro_rules! is_error_kind {
    ($result_expression:expr, $( $pattern:pat )|+ $( if $guard: expr )?) => {
        match $result_expression {
            Ok(_) => false,
            Err(e) => match e.downcast_ref::<ErrorKind>() {
                $( Some($pattern) )|+ $( if $guard )? => true,
                _ => false
            }
        }
    }
}

mod all_changes;
mod all_simple;
mod current_changes;
mod current_invalid;
mod current_simple;
mod push_redirection;

const EMPTY_PUSHREDIRECTOR: &str = r#"{
     "per_repo": {}
 }"#;
const EMTPY_COMMMIT_SYNC_CURRENT: &str = r#"{
     "repos": {}
 }"#;
const EMTPY_COMMMIT_SYNC_ALL: &str = r#"{
     "repos": {}
 }"#;

const CURRENT_COMMIT_SYNC_CONFIG_V1: &str = r#"{
    "repos": {
        "large_repo_1": {
            "large_repo_id": 0,
            "common_pushrebase_bookmarks": ["b1"],
            "small_repos": [
                {
                    "repoid": 1,
                    "default_action": "prepend_prefix",
                    "default_prefix": "f1",
                    "bookmark_prefix": "bp1/",
                    "mapping": {"d": "dd"},
                    "direction": "large_to_small"
                },
                {
                    "repoid": 2,
                    "default_action": "prepend_prefix",
                    "default_prefix": "f2",
                    "bookmark_prefix": "bp2/",
                    "mapping": {"d": "ddd"},
                    "direction": "small_to_large"
                }
            ],
            "version_name": "TEST_VERSION_NAME_LIVE_1"
        },
        "large_repo_2": {
            "large_repo_id": 3,
            "common_pushrebase_bookmarks": ["b3"],
            "small_repos": [
                {
                    "repoid": 4,
                    "default_action": "preserve",
                    "default_prefix": "f3",
                    "bookmark_prefix": "bp3/",
                    "mapping": {"d": "dddd"},
                    "direction": "small_to_large"
                }
            ],
            "version_name": "TEST_VERSION_NAME_R3_1"
        }
    }
}"#;

fn ensure_all_updated() {
    // This is copy-pasted from `cached_config`'s own
    // unit test
    thread::yield_now();
    thread::sleep(Duration::from_secs(1));
}

fn get_ctx_source_store_and_live_config(
    fb: FacebookInit,
    pushredirector_config: &str,
    current_commit_sync: &str,
    all_commit_syncs: &str,
) -> (
    CoreContext,
    Arc<TestSource>,
    ConfigStore,
    LiveCommitSyncConfig,
) {
    let ctx = CoreContext::test_mock(fb);
    let test_source = Arc::new(TestSource::new());
    test_source.insert_config(CONFIGERATOR_PUSHREDIRECT_ENABLE, pushredirector_config, 0);
    test_source.insert_config(
        CONFIGERATOR_CURRENT_COMMIT_SYNC_CONFIGS,
        current_commit_sync,
        0,
    );
    test_source.insert_config(CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS, all_commit_syncs, 0);

    // We want to always refresh these paths in the test setting
    test_source.insert_to_refresh(CONFIGERATOR_PUSHREDIRECT_ENABLE.to_string());
    test_source.insert_to_refresh(CONFIGERATOR_CURRENT_COMMIT_SYNC_CONFIGS.to_string());
    test_source.insert_to_refresh(CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS.to_string());

    let store = ConfigStore::new(test_source.clone(), Duration::from_millis(2), None);
    let live_commit_sync_config = LiveCommitSyncConfig::new(ctx.logger(), &store).unwrap();
    (ctx, test_source, store, live_commit_sync_config)
}
