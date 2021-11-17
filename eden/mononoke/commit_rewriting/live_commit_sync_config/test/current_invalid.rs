/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use fbinit::FacebookInit;
use live_commit_sync_config::{ErrorKind, LiveCommitSyncConfig};
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
