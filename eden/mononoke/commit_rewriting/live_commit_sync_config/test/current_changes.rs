/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use ascii::AsciiString;
use fbinit::FacebookInit;
use live_commit_sync_config::CONFIGERATOR_CURRENT_COMMIT_SYNC_CONFIGS;
use mononoke_types::RepositoryId;
use std::str::FromStr;

use crate::{
    ensure_all_updated, get_ctx_source_store_and_live_config, CURRENT_COMMIT_SYNC_CONFIG_V1,
    EMPTY_PUSHREDIRECTOR, EMTPY_COMMMIT_SYNC_ALL,
};

const CURRENT_COMMIT_SYNC_CONFIG_V2: &str = r#"{
    "repos": {
        "large_repo": {
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
            },
            {
                "repoid": 2,
                "default_action": "prepend_prefix",
                "default_prefix": "f2",
                "bookmark_prefix": "bp2_v2/",
                "mapping": {"d": "ddd"},
                "direction": "small_to_large"
            }
            ],
            "version_name": "TEST_VERSION_NAME_LIVE_2"
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
            "version_name": "TEST_VERSION_NAME_R3_2"
        }
    }
}"#;

#[fbinit::test]
fn test_changing_commit_sync_config(fb: FacebookInit) {
    let (ctx, test_source, _store, live_commit_sync_config) = get_ctx_source_store_and_live_config(
        fb,
        EMPTY_PUSHREDIRECTOR,
        CURRENT_COMMIT_SYNC_CONFIG_V1,
        EMTPY_COMMMIT_SYNC_ALL,
    );
    let repo_1 = RepositoryId::new(1);
    let repo_2 = RepositoryId::new(2);

    // CommitSyncConfig is accessible by small repo ids
    let csc_r1_v1 = live_commit_sync_config
        .get_current_commit_sync_config(&ctx, repo_1)
        .unwrap();
    let csc_r2_v1 = live_commit_sync_config
        .get_current_commit_sync_config(&ctx, repo_2)
        .unwrap();

    assert_eq!(
        csc_r1_v1.version_name.0,
        "TEST_VERSION_NAME_LIVE_1".to_string()
    );
    assert_eq!(
        csc_r1_v1.small_repos[&repo_1].bookmark_prefix,
        AsciiString::from_str("bp1/").unwrap()
    );
    assert_eq!(
        csc_r2_v1.small_repos[&repo_2].bookmark_prefix,
        AsciiString::from_str("bp2/").unwrap()
    );

    test_source.insert_config(
        CONFIGERATOR_CURRENT_COMMIT_SYNC_CONFIGS,
        CURRENT_COMMIT_SYNC_CONFIG_V2,
        1,
    );
    ensure_all_updated();

    let csc_r1_v2 = live_commit_sync_config
        .get_current_commit_sync_config(&ctx, repo_1)
        .unwrap();
    let csc_r2_v2 = live_commit_sync_config
        .get_current_commit_sync_config(&ctx, repo_2)
        .unwrap();

    assert_eq!(
        csc_r1_v2.version_name.0,
        "TEST_VERSION_NAME_LIVE_2".to_string()
    );

    assert_eq!(
        csc_r1_v2.small_repos[&repo_1].bookmark_prefix,
        AsciiString::from_str("bp1_v2/").unwrap()
    );
    assert_eq!(
        csc_r2_v2.small_repos[&repo_2].bookmark_prefix,
        AsciiString::from_str("bp2_v2/").unwrap()
    );
}
