/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cached_config::ModificationTime;
use fbinit::FacebookInit;
use live_commit_sync_config::ErrorKind;
use live_commit_sync_config::LiveCommitSyncConfig;
use live_commit_sync_config::CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS;
use metaconfig_types::CommitSyncConfigVersion;
use mononoke_types::RepositoryId;
use pretty_assertions::assert_eq;

use crate::ensure_all_updated;
use crate::get_ctx_source_store_and_live_config;
use crate::EMPTY_PUSHREDIRECTOR;

// Since these huge blobs of json may be hard to read, I will explain
// the difference in the comment. The comparison of `ALL_COMMIT_SYNC_CONFIG_V1`
// and `ALL_COMMIT_SYNC_CONFIG_V2` and `ALL_COMMIT_SYNC_CONFIG_V3` is as follows:
// - for `large_repo` (repo_id=0), V2 adds a new version of `CommitSyncConfig`,
//   called `TEST_VERSION_NAME_LIVE_2`, and makes it a current version, V3 does not
//   change the list of versions, but makes `TEST_VERSION_NAME_LIVE_1` a current
//   version once again
// - for `large_repo_2` nothing changes between versions

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
                }
            ],
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
                    "version_name": "TEST_VERSION_NAME_LIVE_R3_1"
                }
            ],
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

const ALL_COMMIT_SYNC_CONFIG_V2: &str = r#"{
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
                    "version_name": "TEST_VERSION_NAME_LIVE_R3_1"
                }
            ],
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

const ALL_COMMIT_SYNC_CONFIG_V3: &str = r#"{
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
                    "version_name": "TEST_VERSION_NAME_LIVE_R3_1"
                }
            ],
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
async fn test_adding_a_new_version(fb: FacebookInit) {
    let (_ctx, test_source, _store, live_commit_sync_config) =
        get_ctx_source_store_and_live_config(fb, EMPTY_PUSHREDIRECTOR, ALL_COMMIT_SYNC_CONFIG_V1);
    let repo_1 = RepositoryId::new(1);
    let repo_3 = RepositoryId::new(3);

    let v1 = CommitSyncConfigVersion("TEST_VERSION_NAME_LIVE_1".to_string());
    let v2 = CommitSyncConfigVersion("TEST_VERSION_NAME_LIVE_2".to_string());
    let vr31 = CommitSyncConfigVersion("TEST_VERSION_NAME_LIVE_R3_1".to_string());

    // Before we apply any changes, configs are as expected
    let av1 = live_commit_sync_config
        .get_all_commit_sync_config_versions(repo_1)
        .await
        .unwrap();
    let av3 = live_commit_sync_config
        .get_all_commit_sync_config_versions(repo_3)
        .await
        .unwrap();

    assert_eq!(av1.len(), 1);
    assert!(av1.contains_key(&v1));
    assert_eq!(av3.len(), 1);
    assert!(av3.contains_key(&vr31));

    // Let's make a change to our config source: add a new version
    // and make it the current one
    test_source.insert_config(
        CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS,
        ALL_COMMIT_SYNC_CONFIG_V2,
        ModificationTime::UnixTimestamp(1),
    );
    ensure_all_updated();

    // Ensure an added version is recognized after a change
    let av1 = live_commit_sync_config
        .get_all_commit_sync_config_versions(repo_1)
        .await
        .unwrap();
    let av3 = live_commit_sync_config
        .get_all_commit_sync_config_versions(repo_3)
        .await
        .unwrap();

    assert_eq!(av1.len(), 2);
    assert!(av1.contains_key(&v1));
    assert!(av1.contains_key(&v2));
    assert_eq!(av3.len(), 1);
    assert!(av3.contains_key(&vr31));

    // Let's make a change to our config source: revert to the previous current version
    test_source.insert_config(
        CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS,
        ALL_COMMIT_SYNC_CONFIG_V3,
        ModificationTime::UnixTimestamp(2),
    );
    ensure_all_updated();

    // Ensure new current version is picked up, but len of versions is the same for r1
    let av1 = live_commit_sync_config
        .get_all_commit_sync_config_versions(repo_1)
        .await
        .unwrap();
    let av3 = live_commit_sync_config
        .get_all_commit_sync_config_versions(repo_3)
        .await
        .unwrap();

    assert_eq!(av1.len(), 2);
    assert!(av1.contains_key(&v1));
    assert!(av1.contains_key(&v2));
    assert_eq!(av3.len(), 1);
    assert!(av3.contains_key(&vr31));
}

#[fbinit::test]
async fn test_query_by_version_name(fb: FacebookInit) {
    let (_ctx, test_source, _store, live_commit_sync_config) =
        get_ctx_source_store_and_live_config(fb, EMPTY_PUSHREDIRECTOR, ALL_COMMIT_SYNC_CONFIG_V1);
    let repo_1 = RepositoryId::new(1);
    let repo_3 = RepositoryId::new(3);

    let v1 = CommitSyncConfigVersion("TEST_VERSION_NAME_LIVE_1".to_string());
    let v2 = CommitSyncConfigVersion("TEST_VERSION_NAME_LIVE_2".to_string());

    // Before we apply any changes, configs are as expected
    let r1_v1 = live_commit_sync_config
        .get_commit_sync_config_by_version(repo_1, &v1)
        .await
        .unwrap();
    let r3_v1_res = live_commit_sync_config
        .get_commit_sync_config_by_version(repo_3, &v1)
        .await;

    assert_eq!(r1_v1.version_name, v1);
    // This version is not for r3, so we did not get a repo
    assert!(is_error_kind!(
        r3_v1_res,
        ErrorKind::UnknownCommitSyncConfigVersion(_, _)
    ));

    // Let's make a change to our config source: add a new version
    // and make it the current one
    test_source.insert_config(
        CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS,
        ALL_COMMIT_SYNC_CONFIG_V2,
        ModificationTime::UnixTimestamp(1),
    );
    ensure_all_updated();

    // Ensure an added version is recognized after a change
    let r1_v1 = live_commit_sync_config
        .get_commit_sync_config_by_version(repo_1, &v1)
        .await
        .unwrap();
    let r1_v2 = live_commit_sync_config
        .get_commit_sync_config_by_version(repo_1, &v2)
        .await
        .unwrap();

    assert_eq!(r1_v1.version_name, v1);
    assert_eq!(r1_v2.version_name, v2);

    // Let's make a change to our config source: revert to the previous current version
    test_source.insert_config(
        CONFIGERATOR_ALL_COMMIT_SYNC_CONFIGS,
        ALL_COMMIT_SYNC_CONFIG_V3,
        ModificationTime::UnixTimestamp(2),
    );
    ensure_all_updated();

    // Ensure new current version is picked up,
    // but v2 is still accessible
    let r1_v1 = live_commit_sync_config
        .get_commit_sync_config_by_version(repo_1, &v1)
        .await
        .unwrap();
    let r1_v2 = live_commit_sync_config
        .get_commit_sync_config_by_version(repo_1, &v2)
        .await
        .unwrap();

    assert_eq!(r1_v1.version_name, v1);
    assert_eq!(r1_v2.version_name, v2);
}
