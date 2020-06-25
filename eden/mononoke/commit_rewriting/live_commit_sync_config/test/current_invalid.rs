/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use fbinit::FacebookInit;
use live_commit_sync_config::ErrorKind;
use mononoke_types::RepositoryId;

use crate::{
    get_ctx_source_store_and_live_config, CURRENT_COMMIT_SYNC_CONFIG_V1, EMPTY_PUSHREDIRECTOR,
    EMTPY_COMMMIT_SYNC_ALL,
};

const CURRENT_COMMIT_SYNC_CONFIG_INVALID_LARGE_PART_OF_MULTIPLE: &str = r#"{
    "repos": {
        "large_repo_1": {
            "large_repo_id": 0,
            "common_pushrebase_bookmarks": ["b1"],
            "small_repos": [
                {
                    "repoid": 1,
                    "default_action": "preserve",
                    "default_prefix": "f1",
                    "bookmark_prefix": "bp1_v2/",
                    "mapping": {"d": "dd"},
                    "direction": "large_to_small"
                }
            ],
            "version_name": "TEST_VERSION_NAME_LIVE_2"
        },
        "large_repo_2": {
            "large_repo_id": 0,
            "common_pushrebase_bookmarks": ["b1"],
            "small_repos": [
                {
                    "repoid": 2,
                    "default_action": "preserve",
                    "default_prefix": "f1",
                    "bookmark_prefix": "bp1_v2/",
                    "mapping": {"d": "dd"},
                    "direction": "large_to_small"
                }
            ],
            "version_name": "TEST_VERSION_NAME_LIVE"
        }
    }
}"#;

const CURRENT_COMMIT_SYNC_CONFIG_INVALID_SMALL_PART_OF_MULTIPLE: &str = r#"{
    "repos": {
        "large_repo_1": {
            "large_repo_id": 0,
            "common_pushrebase_bookmarks": ["b1"],
            "small_repos": [
                {
                    "repoid": 1,
                    "default_action": "preserve",
                    "default_prefix": "f1",
                    "bookmark_prefix": "bp1_v2/",
                    "mapping": {"d": "dd"},
                    "direction": "large_to_small"
                }
            ],
            "version_name": "TEST_VERSION_NAME_LIVE"
        },
        "large_repo_2": {
            "large_repo_id": 2,
            "common_pushrebase_bookmarks": ["b1"],
            "small_repos": [
                {
                    "repoid": 1,
                    "default_action": "preserve",
                    "default_prefix": "f1",
                    "bookmark_prefix": "bp1_v2/",
                    "mapping": {"d": "dd"},
                    "direction": "large_to_small"
                }
            ],
            "version_name": "TEST_VERSION_NAME_LIVE"
        }

    }
}"#;

const CURRENT_COMMIT_SYNC_CONFIG_INVALID_SMALL_IN_ONE_LARGE_IN_OTHER: &str = r#"{
    "repos": {
        "large_repo_1": {
            "large_repo_id": 0,
            "common_pushrebase_bookmarks": ["b1"],
            "small_repos": [
                {
                    "repoid": 1,
                    "default_action": "preserve",
                    "default_prefix": "f1",
                    "bookmark_prefix": "bp1_v2/",
                    "mapping": {"d": "dd"},
                    "direction": "large_to_small"
                }
            ],
            "version_name": "TEST_VERSION_NAME_LIVE"
        },
        "large_repo_2": {
            "large_repo_id": 1,
            "common_pushrebase_bookmarks": ["b1"],
            "small_repos": [
                {
                    "repoid": 2,
                    "default_action": "preserve",
                    "default_prefix": "f1",
                    "bookmark_prefix": "bp1_v2/",
                    "mapping": {"d": "dd"},
                    "direction": "large_to_small"
                }
            ],
            "version_name": "TEST_VERSION_NAME_LIVE"
        }

    }
}"#;

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

#[fbinit::test]
fn test_unknown_repo(fb: FacebookInit) {
    let (ctx, _test_source, _store, live_commit_sync_config) = get_ctx_source_store_and_live_config(
        fb,
        EMPTY_PUSHREDIRECTOR,
        CURRENT_COMMIT_SYNC_CONFIG_V1,
        EMTPY_COMMMIT_SYNC_ALL,
    );
    let repo_9 = RepositoryId::new(9);
    // Unknown repo
    let csc_r9_res = live_commit_sync_config.get_current_commit_sync_config(&ctx, repo_9);
    assert!(is_error_kind!(
        csc_r9_res,
        ErrorKind::NotPartOfAnyCommitSyncConfig(_)
    ));
}

#[fbinit::test]
fn test_large_repo_part_of_multiple_configs(fb: FacebookInit) {
    let (ctx, _test_source, _store, live_commit_sync_config) = get_ctx_source_store_and_live_config(
        fb,
        EMPTY_PUSHREDIRECTOR,
        CURRENT_COMMIT_SYNC_CONFIG_INVALID_LARGE_PART_OF_MULTIPLE,
        EMTPY_COMMMIT_SYNC_ALL,
    );
    let repo_0 = RepositoryId::new(0);
    let csc_r0_res = live_commit_sync_config.get_current_commit_sync_config(&ctx, repo_0);
    assert!(is_error_kind!(
        csc_r0_res,
        ErrorKind::PartOfMultipleCommitSyncConfigs(_)
    ));
}

#[fbinit::test]
fn test_small_repo_part_of_multiple_configs(fb: FacebookInit) {
    let (ctx, _test_source, _store, live_commit_sync_config) = get_ctx_source_store_and_live_config(
        fb,
        EMPTY_PUSHREDIRECTOR,
        CURRENT_COMMIT_SYNC_CONFIG_INVALID_SMALL_PART_OF_MULTIPLE,
        EMTPY_COMMMIT_SYNC_ALL,
    );
    let repo_1 = RepositoryId::new(1);
    let csc_r1_res = live_commit_sync_config.get_current_commit_sync_config(&ctx, repo_1);
    assert!(is_error_kind!(
        csc_r1_res,
        ErrorKind::PartOfMultipleCommitSyncConfigs(_)
    ));
}

#[fbinit::test]
fn test_repo_is_small_in_one_config_and_large_in_the_other(fb: FacebookInit) {
    let (ctx, _test_source, _store, live_commit_sync_config) = get_ctx_source_store_and_live_config(
        fb,
        EMPTY_PUSHREDIRECTOR,
        CURRENT_COMMIT_SYNC_CONFIG_INVALID_SMALL_IN_ONE_LARGE_IN_OTHER,
        EMTPY_COMMMIT_SYNC_ALL,
    );
    let repo_1 = RepositoryId::new(1);
    let csc_r1_res = live_commit_sync_config.get_current_commit_sync_config(&ctx, repo_1);
    assert!(is_error_kind!(
        csc_r1_res,
        ErrorKind::PartOfMultipleCommitSyncConfigs(_)
    ));
}
