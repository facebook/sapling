/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

struct RawCommitSyncSmallRepoConfig {
    1: i32 repoid,
    2: string default_action,
    3: optional string default_prefix,
    4: string bookmark_prefix,
    5: map<string, string> mapping,
    6: string direction,
}

struct RawCommitSyncConfig {
    1: i32 large_repo_id,
    2: list<string> common_pushrebase_bookmarks,
    3: list<RawCommitSyncSmallRepoConfig> small_repos,
}

 struct RawWireprotoLoggingConfig {
     1: string scribe_category;
     2: string storage_config;
 }

// Raw configuration for health monitoring of the
// source-control-as-a-service solutions
struct RawSourceControlServiceMonitoring {
    1: list<string> bookmarks_to_report_age,
}
