/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use fbinit::FacebookInit;
use live_commit_sync_config::LiveCommitSyncConfig;
use mononoke_types::RepositoryId;
use pretty_assertions::assert_eq;

use crate::get_ctx_source_store_and_live_config;
use crate::EMPTY_PUSHREDIRECTOR;

const ALL_COMMIT_SYNC_CONFIG_V1: &str = r#"{
    "repos": {
        "large_repo": {
            "versions": [
                {
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
                {
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
                    "version_name": "TEST_VERSION_NAME_LIVE_2"
                }
            ],
            "current_version": "TEST_VERSION_NAME_LIVE_2",
            "common": {
                "large_repo_id": 0,
                "common_pushrebase_bookmarks": ["b1"],
                "small_repos": {
                    1: {
                        "bookmark_prefix": "bp1/"
                    },
                    2: {
                        "bookmark_prefix": "bp2/"
                    }
                }
            }
        },
        "large_repo_2": {
            "versions": [
                {
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
            ],
            "current_version": "TEST_VERSION_NAME_R3_1,",
            "common": {
                "large_repo_id": 3,
                "common_pushrebase_bookmarks": ["b3"],
                "small_repos": {
                    4: {
                        "bookmark_prefix": "bp3/"
                    }
                }
            }
        }
    }
}"#;

#[fbinit::test]
async fn test_different_repos_same_group(fb: FacebookInit) {
    let (_ctx, _test_source, _store, live_commit_sync_config) =
        get_ctx_source_store_and_live_config(fb, EMPTY_PUSHREDIRECTOR, ALL_COMMIT_SYNC_CONFIG_V1);

    let repo_0 = RepositoryId::new(0);
    let repo_1 = RepositoryId::new(1);
    let repo_2 = RepositoryId::new(2);
    let repo_3 = RepositoryId::new(3);
    let repo_4 = RepositoryId::new(4);

    let av0 = live_commit_sync_config
        .get_all_commit_sync_config_versions(repo_0)
        .await
        .unwrap();
    let av1 = live_commit_sync_config
        .get_all_commit_sync_config_versions(repo_1)
        .await
        .unwrap();
    let av2 = live_commit_sync_config
        .get_all_commit_sync_config_versions(repo_2)
        .await
        .unwrap();
    let av3 = live_commit_sync_config
        .get_all_commit_sync_config_versions(repo_3)
        .await
        .unwrap();
    let av4 = live_commit_sync_config
        .get_all_commit_sync_config_versions(repo_4)
        .await
        .unwrap();

    assert_eq!(av0, av1);
    assert_eq!(av0, av2);
    assert_eq!(av3, av4);
    assert_ne!(av0, av3);
}

#[fbinit::test]
async fn test_version_counts(fb: FacebookInit) {
    let (_ctx, _test_source, _store, live_commit_sync_config) =
        get_ctx_source_store_and_live_config(fb, EMPTY_PUSHREDIRECTOR, ALL_COMMIT_SYNC_CONFIG_V1);

    let repo_0 = RepositoryId::new(0);
    let repo_4 = RepositoryId::new(4);

    let av0 = live_commit_sync_config
        .get_all_commit_sync_config_versions(repo_0)
        .await
        .unwrap();
    let av4 = live_commit_sync_config
        .get_all_commit_sync_config_versions(repo_4)
        .await
        .unwrap();

    assert_eq!(av0.len(), 2);
    assert_eq!(av4.len(), 1);
}
