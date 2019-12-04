/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

struct RawWhitelistEntry {
    1: optional string tier,
    2: optional string identity_data,
    3: optional string identity_type,
}

struct RawCommonConfig {
    1: optional list<RawWhitelistEntry> whitelist_entry,
    2: optional string loadlimiter_category,

    // Scuba table for logging redacted file access attempts
    3: optional string scuba_censored_table,
}

struct RawCacheWarmupConfig {
    1: string bookmark,
    2: optional i64 commit_limit,
}

struct RawBookmarkHook {
    1: string hook_name,
}

struct RawHookConfig {
    1: string name,
    2: optional string path,
    3: string hook_type,
    4: optional string bypass_commit_string,
    5: optional string bypass_pushvar,
    6: optional map<string, string> (rust.type = "HashMap") config_strings,
    7: optional map<string, i32> (rust.type = "HashMap") config_ints,
}

struct RawLfsParams {
    1: optional i64 threshold,
}

struct RawBundle2ReplayParams {
    1: optional bool preserve_raw_bundle2,
}

struct RawShardedFilenodesParams {
    1: string shard_map,
    2: i32 shard_num,
}

struct RawInfinitepushParams {
    1: bool allow_writes,
    2: optional string namespace_pattern,
}

struct RawFilestoreParams {
    1: i64 chunk_size,
    2: i32 concurrency,
}

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
