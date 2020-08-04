/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Functions to load and parse Mononoke configuration.

use std::{
    collections::{HashMap, HashSet},
    convert::TryInto,
    path::Path,
    str,
    time::Duration,
};

use crate::convert::Convert;
use crate::errors::ConfigurationError;
use anyhow::{anyhow, Result};
use fbinit::FacebookInit;
use metaconfig_types::{
    AllowlistEntry, CommitSyncConfig, CommonConfig, HgsqlGlobalrevsName, HgsqlName, Redaction,
    RepoConfig, RepoReadOnly, SegmentedChangelogConfig, StorageConfig,
};
use mononoke_types::RepositoryId;
use repos::{
    RawCommitSyncConfig, RawCommonConfig, RawRepoConfig, RawRepoConfigs, RawStorageConfig,
};

const LIST_KEYS_PATTERNS_MAX_DEFAULT: u64 = 500_000;
const HOOK_MAX_FILE_SIZE_DEFAULT: u64 = 8 * 1024 * 1024; // 8MiB

/// Load configuration common to all repositories.
pub fn load_common_config(fb: FacebookInit, config_path: impl AsRef<Path>) -> Result<CommonConfig> {
    let common = crate::raw::read_raw_configs(fb, config_path.as_ref())?.common;
    parse_common_config(common)
}

/// Holds configuration for repostories.
#[derive(Debug, PartialEq)]
pub struct RepoConfigs {
    /// Configs for all repositories
    pub repos: HashMap<String, RepoConfig>,
    /// Common configs for all repos
    pub common: CommonConfig,
}

/// Load configuration for repositories.
pub fn load_repo_configs(fb: FacebookInit, config_path: impl AsRef<Path>) -> Result<RepoConfigs> {
    let RawRepoConfigs {
        commit_sync,
        common,
        repos,
        storage,
    } = crate::raw::read_raw_configs(fb, config_path.as_ref())?;

    let commit_sync = parse_commit_sync_config(commit_sync)?;

    let mut repo_configs = HashMap::new();
    let mut repoids = HashSet::new();

    for (reponame, raw_repo_config) in repos.into_iter() {
        let config = parse_repo_config(&reponame, raw_repo_config, &storage, &commit_sync)?;

        if !repoids.insert(config.repoid) {
            return Err(ConfigurationError::DuplicatedRepoId(config.repoid).into());
        }

        repo_configs.insert(reponame.clone(), config);
    }

    let common = parse_common_config(common)?;

    Ok(RepoConfigs {
        repos: repo_configs,
        common,
    })
}

/// Holds configuration for storage.
#[derive(Debug, PartialEq)]
pub struct StorageConfigs {
    /// Configs for all storage
    pub storage: HashMap<String, StorageConfig>,
}

/// Load configuration for storage.
pub fn load_storage_configs(
    fb: FacebookInit,
    config_path: impl AsRef<Path>,
) -> Result<StorageConfigs> {
    let storage = crate::raw::read_raw_configs(fb, config_path.as_ref())?
        .storage
        .into_iter()
        .map(|(k, v)| Ok((k, v.convert()?)))
        .collect::<Result<_>>()?;

    Ok(StorageConfigs { storage })
}

fn parse_common_config(common: RawCommonConfig) -> Result<CommonConfig> {
    let mut tiers_num = 0;
    let security_config: Vec<_> = common
        .whitelist_entry
        .unwrap_or_default()
        .into_iter()
        .map(|allowlist_entry| {
            let has_tier = allowlist_entry.tier.is_some();
            let has_identity = {
                if allowlist_entry.identity_data.is_none() ^ allowlist_entry.identity_type.is_none()
                {
                    return Err(ConfigurationError::InvalidFileStructure(
                        "identity type and data must be specified".into(),
                    )
                    .into());
                }

                allowlist_entry.identity_type.is_some()
            };

            if has_tier && has_identity {
                return Err(ConfigurationError::InvalidFileStructure(
                    "tier and identity cannot be both specified".into(),
                )
                .into());
            }

            if !has_tier && !has_identity {
                return Err(ConfigurationError::InvalidFileStructure(
                    "tier or identity must be specified".into(),
                )
                .into());
            }

            if allowlist_entry.tier.is_some() {
                tiers_num += 1;
                Ok(AllowlistEntry::Tier(allowlist_entry.tier.unwrap()))
            } else {
                let identity_type = allowlist_entry.identity_type.unwrap();

                Ok(AllowlistEntry::HardcodedIdentity {
                    ty: identity_type,
                    data: allowlist_entry.identity_data.unwrap(),
                })
            }
        })
        .collect::<Result<_>>()?;

    if tiers_num > 1 {
        return Err(
            ConfigurationError::InvalidFileStructure("only one tier is allowed".into()).into(),
        );
    }

    let loadlimiter_category = common
        .loadlimiter_category
        .filter(|category| !category.is_empty());
    let scuba_censored_table = common.scuba_censored_table;

    Ok(CommonConfig {
        security_config,
        loadlimiter_category,
        scuba_censored_table,
    })
}

fn parse_repo_config(
    reponame: &str,
    repo_config: RawRepoConfig,
    common_storage_config: &HashMap<String, RawStorageConfig>,
    commit_sync_config: &HashMap<String, CommitSyncConfig>,
) -> Result<RepoConfig> {
    let RawRepoConfig {
        repoid,
        storage_config,
        storage,
        enabled,
        readonly,
        bookmarks,
        bookmarks_cache_ttl,
        hook_manager_params,
        hooks,
        write_lock_db_address,
        redaction,
        generation_cache_size,
        scuba_table,
        scuba_table_hooks,
        delay_mean: _,
        delay_stddev: _,
        cache_warmup,
        push,
        pushrebase,
        lfs,
        wireproto_logging,
        hash_validation_percentage,
        skiplist_index_blobstore_key,
        bundle2_replay_params,
        infinitepush,
        list_keys_patterns_max,
        filestore,
        hook_max_file_size,
        hipster_acl,
        source_control_service,
        source_control_service_monitoring,
        name: _,
        derived_data_config,
        scuba_local_path,
        scuba_local_path_hooks,
        hgsql_name,
        hgsql_globalrevs_name,
        enforce_lfs_acl_check,
        repo_client_use_warm_bookmarks_cache,
        ..
    } = repo_config;

    let repoid =
        RepositoryId::new(repoid.ok_or_else(|| anyhow!("missing repoid from configuration"))?);

    let enabled = enabled.unwrap_or(true);

    let hooks: Vec<_> = hooks.unwrap_or_default().convert()?;

    let get_storage = move |name: &str| -> Result<StorageConfig> {
        let raw_storage_config = storage
            .as_ref()
            .and_then(|s| s.get(name))
            .or_else(|| common_storage_config.get(name))
            .cloned()
            .ok_or_else(|| {
                ConfigurationError::InvalidConfig(format!("Storage \"{}\" not defined", name))
            })?;

        raw_storage_config.convert()
    };

    let storage_config = get_storage(
        &storage_config.ok_or_else(|| anyhow!("missing storage_config from configuration"))?,
    )?;

    let wireproto_logging = wireproto_logging
        .map(|raw| crate::convert::repo::convert_wireproto_logging_config(raw, get_storage))
        .transpose()?
        .unwrap_or_default();

    let cache_warmup = cache_warmup.convert()?;

    let hook_manager_params = hook_manager_params.convert()?;

    let bookmarks = bookmarks.unwrap_or_default().convert()?;

    let bookmarks_cache_ttl = bookmarks_cache_ttl
        .map(|ttl| -> Result<_> { Ok(Duration::from_millis(ttl.try_into()?)) })
        .transpose()?;

    let push = push.convert()?.unwrap_or_default();

    let pushrebase = pushrebase.convert()?.unwrap_or_default();

    let bundle2_replay_params = bundle2_replay_params.convert()?.unwrap_or_default();

    let lfs = lfs.convert()?.unwrap_or_default();

    let hash_validation_percentage = hash_validation_percentage
        .map(|v| v.try_into())
        .transpose()?
        .unwrap_or(0);

    let readonly = if readonly.unwrap_or_default() {
        RepoReadOnly::ReadOnly("Set by config option".to_string())
    } else {
        RepoReadOnly::ReadWrite
    };

    let redaction = if redaction.unwrap_or(true) {
        Redaction::Enabled
    } else {
        Redaction::Disabled
    };

    let infinitepush = infinitepush.convert()?.unwrap_or_default();

    let generation_cache_size: usize = generation_cache_size
        .map(|v| v.try_into())
        .transpose()?
        .unwrap_or(10 * 1024 * 1024);

    let list_keys_patterns_max: u64 = list_keys_patterns_max
        .map(|v| v.try_into())
        .transpose()?
        .unwrap_or(LIST_KEYS_PATTERNS_MAX_DEFAULT);

    let hook_max_file_size: u64 = hook_max_file_size
        .map(|v| v.try_into())
        .transpose()?
        .unwrap_or(HOOK_MAX_FILE_SIZE_DEFAULT);

    let filestore = filestore.convert()?;

    let source_control_service = source_control_service.convert()?.unwrap_or_default();

    let source_control_service_monitoring = source_control_service_monitoring.convert()?;

    let derived_data_config = derived_data_config.convert()?.unwrap_or_default();

    let hgsql_name = HgsqlName(hgsql_name.unwrap_or_else(|| reponame.to_string()));

    let hgsql_globalrevs_name =
        HgsqlGlobalrevsName(hgsql_globalrevs_name.unwrap_or_else(|| hgsql_name.0.clone()));

    let relevant_commit_sync_configs: Vec<&CommitSyncConfig> = commit_sync_config
        .values()
        .filter(|config| is_commit_sync_config_relevant_to_repo(&repoid, config))
        .collect();
    let commit_sync_config = match relevant_commit_sync_configs.as_slice() {
        [] => None,
        [commit_sync_config] => Some((*commit_sync_config).clone()),
        _ => {
            return Err(anyhow!(
                "Repo {} participates in more than one commit sync config",
                repoid,
            ))
        }
    };

    let enforce_lfs_acl_check = enforce_lfs_acl_check.unwrap_or(false);
    let repo_client_use_warm_bookmarks_cache =
        repo_client_use_warm_bookmarks_cache.unwrap_or(false);

    let segmented_changelog_config = SegmentedChangelogConfig::default();

    Ok(RepoConfig {
        enabled,
        storage_config,
        generation_cache_size,
        repoid,
        scuba_table,
        scuba_local_path,
        scuba_table_hooks,
        scuba_local_path_hooks,
        cache_warmup,
        hook_manager_params,
        bookmarks,
        bookmarks_cache_ttl,
        hooks,
        push,
        pushrebase,
        lfs,
        wireproto_logging,
        hash_validation_percentage,
        readonly,
        redaction,
        skiplist_index_blobstore_key,
        bundle2_replay_params,
        write_lock_db_address,
        infinitepush,
        list_keys_patterns_max,
        filestore,
        commit_sync_config,
        hook_max_file_size,
        hipster_acl,
        source_control_service,
        source_control_service_monitoring,
        derived_data_config,
        hgsql_name,
        hgsql_globalrevs_name,
        enforce_lfs_acl_check,
        repo_client_use_warm_bookmarks_cache,
        segmented_changelog_config,
    })
}

fn is_commit_sync_config_relevant_to_repo(
    repoid: &RepositoryId,
    commit_sync_config: &CommitSyncConfig,
) -> bool {
    &commit_sync_config.large_repo_id == repoid
        || commit_sync_config
            .small_repos
            .iter()
            .any(|(k, _)| k == repoid)
}

/// Parse a collection of raw commit sync config into commit sync config and validate it.
fn parse_commit_sync_config(
    raw_commit_syncs: HashMap<String, RawCommitSyncConfig>,
) -> Result<HashMap<String, CommitSyncConfig>> {
    raw_commit_syncs
        .into_iter()
        .map(|(config_name, commit_sync_config)| {
            let commit_sync_config = commit_sync_config.convert()?;
            Ok((config_name, commit_sync_config))
        })
        .collect()
}

impl RepoConfigs {
    /// Get individual `RepoConfig`, given a repo_id
    pub fn get_repo_config(&self, repo_id: RepositoryId) -> Option<(&String, &RepoConfig)> {
        self.repos
            .iter()
            .find(|(_, repo_config)| repo_config.repoid == repo_id)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use ascii::AsciiString;
    use bookmarks_types::BookmarkName;
    use maplit::{btreemap, btreeset, hashmap};
    use metaconfig_types::{
        BlobConfig, BlobstoreId, BookmarkParams, Bundle2ReplayParams, CacheWarmupParams,
        CommitSyncConfigVersion, CommitSyncDirection, DatabaseConfig,
        DefaultSmallToLargeCommitSyncPathAction, DerivedDataConfig, FilestoreParams, HookBypass,
        HookConfig, HookManagerParams, HookParams, InfinitepushNamespace, InfinitepushParams,
        LfsParams, LocalDatabaseConfig, MetadataDatabaseConfig, MultiplexId, MultiplexedStoreType,
        PushParams, PushrebaseFlags, PushrebaseParams, RemoteDatabaseConfig,
        RemoteMetadataDatabaseConfig, SegmentedChangelogConfig, ShardableRemoteDatabaseConfig,
        ShardedRemoteDatabaseConfig, SmallRepoCommitSyncConfig, SourceControlServiceMonitoring,
        SourceControlServiceParams, UnodeVersion, WireprotoLoggingConfig,
    };
    use mononoke_types::MPath;
    use nonzero_ext::nonzero;
    use pretty_assertions::assert_eq;
    use regex::Regex;
    use std::fs::{create_dir_all, write};
    use std::num::NonZeroUsize;
    use std::str::FromStr;
    use tempdir::TempDir;

    fn write_files(
        files: impl IntoIterator<Item = (impl AsRef<Path>, impl AsRef<[u8]>)>,
    ) -> TempDir {
        let tmp_dir = TempDir::new("mononoke_test_config").expect("tmp_dir failed");

        // Always create repos directory
        create_dir_all(tmp_dir.path().join("repos")).expect("create repos failed");

        for (path, content) in files.into_iter() {
            let path = path.as_ref();
            let content = content.as_ref();

            let dir = path.parent().expect("missing parent");
            create_dir_all(tmp_dir.path().join(dir)).expect("create dir failed");
            write(tmp_dir.path().join(path), content).expect("write failed");
        }

        tmp_dir
    }

    #[fbinit::test]
    fn test_commit_sync_config_correct(fb: FacebookInit) {
        let commit_sync_config = r#"
            [mega]
            large_repo_id = 1
            common_pushrebase_bookmarks = ["master"]
            version_name = "TEST_VERSION_NAME"

                [[mega.small_repos]]
                repoid = 2
                default_action = "preserve"
                bookmark_prefix = "repo2"
                direction = "small_to_large"

                    [mega.small_repos.mapping]
                    "p1" = ".r2-legacy/p1"
                    "p5" = ".r2-legacy/p5"

                [[mega.small_repos]]
                repoid = 3
                bookmark_prefix = "repo3"
                default_action = "prepend_prefix"
                default_prefix = "subdir"
                direction = "small_to_large"

                    [mega.small_repos.mapping]
                    "p1" = "p1"
                    "p4" = "p5/p4"
        "#;

        let paths = btreemap! {
            "common/commitsyncmap.toml" => commit_sync_config
        };
        let tmp_dir = write_files(&paths);
        let raw_config =
            crate::raw::read_raw_configs(fb, tmp_dir.path()).expect("expect to read configs");
        let commit_sync = parse_commit_sync_config(raw_config.commit_sync)
            .expect("expected to get a commit sync config");

        let expected = hashmap! {
            "mega".to_owned() => CommitSyncConfig {
                large_repo_id: RepositoryId::new(1),
                common_pushrebase_bookmarks: vec![BookmarkName::new("master").unwrap()],
                small_repos: hashmap! {
                    RepositoryId::new(2) => SmallRepoCommitSyncConfig {
                        default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
                        bookmark_prefix: AsciiString::from_str("repo2").unwrap(),
                        map: hashmap! {
                            MPath::new("p1").unwrap() => MPath::new(".r2-legacy/p1").unwrap(),
                            MPath::new("p5").unwrap() => MPath::new(".r2-legacy/p5").unwrap(),
                        },
                        direction: CommitSyncDirection::SmallToLarge,
                    },
                    RepositoryId::new(3) => SmallRepoCommitSyncConfig {
                        default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(MPath::new("subdir").unwrap()),
                        bookmark_prefix: AsciiString::from_str("repo3").unwrap(),
                        map: hashmap! {
                            MPath::new("p1").unwrap() => MPath::new("p1").unwrap(),
                            MPath::new("p4").unwrap() => MPath::new("p5/p4").unwrap(),
                        },
                        direction: CommitSyncDirection::SmallToLarge,
                    }
                },
                version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
            }
        };

        assert_eq!(commit_sync, expected);
    }

    #[fbinit::test]
    fn test_commit_sync_config_large_is_small(fb: FacebookInit) {
        let commit_sync_config = r#"
            [mega]
            large_repo_id = 1
            common_pushrebase_bookmarks = ["master"]

                [[mega.small_repos]]
                repoid = 1
                bookmark_prefix = "repo2"
                default_action = "preserve"
                direction = "small_to_large"

                    [mega.small_repos.mapping]
                    "p1" = ".r2-legacy/p1"
                    "p5" = "subdir"
        "#;

        let paths = btreemap! {
            "common/commitsyncmap.toml" => commit_sync_config
        };
        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(fb, tmp_dir.path());
        let msg = format!("{:#?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("is one of the small repos too"));
    }

    #[fbinit::test]
    fn test_commit_sync_config_duplicated_small_repos(fb: FacebookInit) {
        let commit_sync_config = r#"
            [mega]
            large_repo_id = 1
            common_pushrebase_bookmarks = ["master"]

                [[mega.small_repos]]
                repoid = 2
                bookmark_prefix = "repo2"
                default_action = "preserve"
                direction = "small_to_large"

                [[mega.small_repos]]
                repoid = 2
                bookmark_prefix = "repo3"
                default_action = "prepend_prefix"
                default_prefix = "subdir"
                direction = "small_to_large"
        "#;

        let paths = btreemap! {
            "common/commitsyncmap.toml" => commit_sync_config
        };
        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(fb, tmp_dir.path());
        let msg = format!("{:#?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("present multiple times in the same CommitSyncConfig"));
    }

    #[fbinit::test]
    fn test_commit_sync_config_conflicting_path_prefixes_small_to_large(fb: FacebookInit) {
        let commit_sync_config = r#"
            [mega]
            large_repo_id = 1
            common_pushrebase_bookmarks = ["master"]

                [[mega.small_repos]]
                repoid = 2
                bookmark_prefix = "repo2"
                default_action = "preserve"
                direction = "small_to_large"

                    [mega.small_repos.mapping]
                    "p1" = ".r2-legacy/p1"
                    "p5" = "subdir"

                [[mega.small_repos]]
                repoid = 3
                bookmark_prefix = "repo3"
                default_action = "prepend_prefix"
                default_prefix = "subdir"
                direction = "small_to_large"

                    [mega.small_repos.mapping]
                    "p1" = "p1"
                    "p4" = "p5/p4"
        "#;

        let paths = btreemap! {
            "common/commitsyncmap.toml" => commit_sync_config
        };
        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(fb, tmp_dir.path());
        let msg = format!("{:#?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("is a prefix of MPath"));
    }

    #[fbinit::test]
    fn test_commit_sync_config_conflicting_path_prefixes_large_to_small(fb: FacebookInit) {
        // Purely identical prefixes, allowed in large-to-small
        let commit_sync_config = r#"
            [mega]
            large_repo_id = 1
            common_pushrebase_bookmarks = ["master"]

                [[mega.small_repos]]
                repoid = 2
                bookmark_prefix = "repo2"
                default_action = "preserve"
                direction = "large_to_small"

                    [mega.small_repos.mapping]
                    "p5" = "subdir"

                [[mega.small_repos]]
                repoid = 3
                bookmark_prefix = "repo3"
                default_action = "prepend_prefix"
                default_prefix = "subdir"
                direction = "large_to_small"

                    [mega.small_repos.mapping]
                    "p1" = "p1"
        "#;

        let paths = btreemap! {
            "common/commitsyncmap.toml" => commit_sync_config
        };
        let tmp_dir = write_files(&paths);
        let commit_sync_config = load_repo_configs(fb, tmp_dir.path());
        assert!(commit_sync_config.is_ok());

        // Overlapping, but not identical prefixes
        let commit_sync_config = r#"
            [mega]
            large_repo_id = 1
            common_pushrebase_bookmarks = ["master"]

                [[mega.small_repos]]
                repoid = 2
                bookmark_prefix = "repo2"
                default_action = "preserve"
                direction = "large_to_small"

                    [mega.small_repos.mapping]
                    "p5" = "subdir/bla"

                [[mega.small_repos]]
                repoid = 3
                bookmark_prefix = "repo3"
                default_action = "prepend_prefix"
                default_prefix = "subdir"
                direction = "large_to_small"

                    [mega.small_repos.mapping]
                    "p1" = "p1"
        "#;

        let paths = btreemap! {
            "common/commitsyncmap.toml" => commit_sync_config
        };
        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(fb, tmp_dir.path());
        let msg = format!("{:#?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("is a prefix of MPath"));
    }

    #[fbinit::test]
    fn test_commit_sync_config_conflicting_path_prefixes_mixed(fb: FacebookInit) {
        // Conflicting paths, should fail
        let commit_sync_config = r#"
            [mega]
            large_repo_id = 1
            common_pushrebase_bookmarks = ["master"]

                [[mega.small_repos]]
                repoid = 2
                bookmark_prefix = "repo2"
                default_action = "preserve"
                direction = "large_to_small"

                    [mega.small_repos.mapping]
                    "p5" = "subdir"

                [[mega.small_repos]]
                repoid = 3
                bookmark_prefix = "repo3"
                default_action = "prepend_prefix"
                default_prefix = "subdir"
                direction = "small_to_large"

                    [mega.small_repos.mapping]
                    "p1" = "p1"
        "#;

        let paths = btreemap! {
            "common/commitsyncmap.toml" => commit_sync_config
        };
        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(fb, tmp_dir.path());
        let msg = format!("{:#?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("is a prefix of MPath"));

        // Paths, identical between large-to-smalls, but
        // overlapping with small-to-large, should fail
        let commit_sync_config = r#"
            [mega]
            large_repo_id = 1
            common_pushrebase_bookmarks = ["master"]

                [[mega.small_repos]]
                repoid = 2
                bookmark_prefix = "repo2"
                default_action = "preserve"
                direction = "large_to_small"

                    [mega.small_repos.mapping]
                    "p5" = "subdir"

                [[mega.small_repos]]
                repoid = 3
                bookmark_prefix = "repo3"
                default_action = "prepend_prefix"
                default_prefix = "r3"
                direction = "small_to_large"

                    [mega.small_repos.mapping]
                    "p1" = "subdir/bla"

                [[mega.small_repos]]
                repoid = 4
                bookmark_prefix = "repo4"
                default_action = "prepend_prefix"
                default_prefix = "r4"
                direction = "large_to_small"

                    [mega.small_repos.mapping]
                    "p4" = "subdir"
        "#;

        let paths = btreemap! {
            "common/commitsyncmap.toml" => commit_sync_config
        };
        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(fb, tmp_dir.path());
        let msg = format!("{:#?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("is a prefix of MPath"));
    }

    #[fbinit::test]
    fn test_commit_sync_config_conflicting_bookmark_prefixes(fb: FacebookInit) {
        let commit_sync_config = r#"
            [mega]
            large_repo_id = 1
            common_pushrebase_bookmarks = ["master"]

                [[mega.small_repos]]
                repoid = 2
                bookmark_prefix = "repo3/bla"
                default_action = "preserve"
                direction = "small_to_large"

                    [mega.small_repos.mapping]
                    "p1" = ".r2-legacy/p1"

                [[mega.small_repos]]
                repoid = 3
                bookmark_prefix = "repo3"
                default_action = "prepend_prefix"
                default_prefix = "subdir"
                direction = "small_to_large"

                    [mega.small_repos.mapping]
                    "p1" = "p1"
                    "p4" = "p5/p4"
        "#;

        let paths = btreemap! {
            "common/commitsyncmap.toml" => commit_sync_config
        };
        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(fb, tmp_dir.path());
        let msg = format!("{:#?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("One bookmark prefix starts with another, which is prohibited"));
    }

    #[fbinit::test]
    fn test_duplicated_repo_ids(fb: FacebookInit) {
        let www_content = r#"
            repoid=1
            scuba_table="scuba_table"
            scuba_table_hooks="scm_hooks"
            storage_config="files"

            [storage.files.metadata.local]
            local_db_path = "/tmp/www"

            [storage.files.blobstore.blob_files]
            path = "/tmp/www"
        "#;
        let common_content = r#"
            loadlimiter_category="test-category"

            [[whitelist_entry]]
            tier = "tier1"

            [[whitelist_entry]]
            identity_type = "username"
            identity_data = "user"
        "#;

        let paths = btreemap! {
            "common/common.toml" => common_content,
            "common/commitsyncmap.toml" => "",
            "repos/www1/server.toml" => www_content,
            "repos/www2/server.toml" => www_content,
        };

        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(fb, tmp_dir.path());
        let msg = format!("{:#?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("DuplicatedRepoId"));
    }

    #[fbinit::test]
    fn test_read_manifest(fb: FacebookInit) {
        let fbsource_content = r#"
            write_lock_db_address="write_lock_db_address"
            generation_cache_size=1048576
            repoid=0
            scuba_table="scuba_table"
            scuba_table_hooks="scm_hooks"
            skiplist_index_blobstore_key="skiplist_key"
            bookmarks_cache_ttl=5000
            storage_config="main"
            list_keys_patterns_max=123
            hook_max_file_size=456
            hipster_acl="foo/test"
            repo_client_use_warm_bookmarks_cache=true

            [wireproto_logging]
            scribe_category="category"
            storage_config="main"

            [cache_warmup]
            bookmark="master"
            commit_limit=100
            [hook_manager_params]
            disable_acl_checker=false

            [derived_data_config]
            derived_data_types=["fsnodes"]
            override_blame_filesize_limit=101

            [derived_data_config.raw_unode_version]
            unode_version_v2 = {}

            [storage.main.metadata.remote]
            primary = { db_address = "db_address" }
            filenodes = { sharded = { shard_map = "db_address_shards", shard_num = 123 } }
            mutation = { db_address = "mutation_db_address" }

            [storage.main.blobstore.multiplexed]
            multiplex_id = 1
            scuba_table = "blobstore_scuba_table"
            components = [
                { blobstore_id = 0, blobstore = { manifold = { manifold_bucket = "bucket" } } },
                { blobstore_id = 1, blobstore = { blob_files = { path = "/tmp/foo" } } },
            ]
            queue_db = { remote = { db_address = "queue_db_address" } }
            minimum_successful_writes = 2

            [[bookmarks]]
            name="master"
            allowed_users="^(svcscm|twsvcscm)$"

            [[bookmarks.hooks]]
            hook_name="hook1"

            [[bookmarks.hooks]]
            hook_name="rust:rusthook"

            [[bookmarks]]
            regex="[^/]*/stable"

            [[hooks]]
            name="hook1"
            bypass_commit_string="@allow_hook1"

            [[hooks]]
            name="rust:rusthook"
            config_ints={ int1 = 44 }

            [push]
            pure_push_allowed = false
            commit_scribe_category = "cat"

            [pushrebase]
            rewritedates = false
            recursion_limit = 1024
            forbid_p2_root_rebases = false
            casefolding_check = false
            emit_obsmarkers = false

            [lfs]
            threshold = 1000
            rollout_percentage = 56
            generate_lfs_blob_in_hg_sync_job = true
            rollout_smc_tier = "smc_tier"

            [bundle2_replay_params]
            preserve_raw_bundle2 = true

            [infinitepush]
            allow_writes = true
            namespace_pattern = "foobar/.+"

            [filestore]
            chunk_size = 768
            concurrency = 48

            [source_control_service_monitoring]
            bookmarks_to_report_age= ["master", "master2"]
        "#;
        let www_content = r#"
            repoid=1
            scuba_table="scuba_table"
            scuba_table_hooks="scm_hooks"
            storage_config="files"
            hgsql_name = "www-foobar"
            hgsql_globalrevs_name = "www-barfoo"

            [storage.files.metadata.local]
            local_db_path = "/tmp/www"

            [storage.files.blobstore.blob_files]
            path = "/tmp/www"
        "#;
        let common_content = r#"
            loadlimiter_category="test-category"

            [[whitelist_entry]]
            tier = "tier1"

            [[whitelist_entry]]
            identity_type = "username"
            identity_data = "user"
        "#;

        let paths = btreemap! {
            "common/common.toml" => common_content,
            "common/commitsyncmap.toml" => "",
            "repos/fbsource/server.toml" => fbsource_content,
            "repos/www/server.toml" => www_content,
            "my_path/my_files" => "",
        };

        let tmp_dir = write_files(&paths);

        let repoconfig = load_repo_configs(fb, tmp_dir.path()).expect("failed to read configs");

        let multiplex = BlobConfig::Multiplexed {
            multiplex_id: MultiplexId::new(1),
            scuba_table: Some("blobstore_scuba_table".to_string()),
            scuba_sample_rate: nonzero!(100u64),
            blobstores: vec![
                (
                    BlobstoreId::new(0),
                    MultiplexedStoreType::Normal,
                    BlobConfig::Manifold {
                        bucket: "bucket".into(),
                        prefix: "".into(),
                    },
                ),
                (
                    BlobstoreId::new(1),
                    MultiplexedStoreType::Normal,
                    BlobConfig::Files {
                        path: "/tmp/foo".into(),
                    },
                ),
            ],
            minimum_successful_writes: nonzero!(2usize),
            queue_db: DatabaseConfig::Remote(RemoteDatabaseConfig {
                db_address: "queue_db_address".into(),
            }),
        };
        let main_storage_config = StorageConfig {
            blobstore: multiplex,
            metadata: MetadataDatabaseConfig::Remote(RemoteMetadataDatabaseConfig {
                primary: RemoteDatabaseConfig {
                    db_address: "db_address".into(),
                },
                filenodes: ShardableRemoteDatabaseConfig::Sharded(ShardedRemoteDatabaseConfig {
                    shard_map: "db_address_shards".into(),
                    shard_num: NonZeroUsize::new(123).unwrap(),
                }),
                mutation: RemoteDatabaseConfig {
                    db_address: "mutation_db_address".into(),
                },
            }),
        };

        let mut repos = HashMap::new();
        repos.insert(
            "fbsource".to_string(),
            RepoConfig {
                enabled: true,
                storage_config: main_storage_config.clone(),
                write_lock_db_address: Some("write_lock_db_address".into()),
                generation_cache_size: 1024 * 1024,
                repoid: RepositoryId::new(0),
                scuba_table: Some("scuba_table".to_string()),
                scuba_local_path: None,
                scuba_table_hooks: Some("scm_hooks".to_string()),
                scuba_local_path_hooks: None,
                cache_warmup: Some(CacheWarmupParams {
                    bookmark: BookmarkName::new("master").unwrap(),
                    commit_limit: 100,
                    microwave_preload: false,
                }),
                hook_manager_params: Some(HookManagerParams {
                    disable_acl_checker: false,
                }),
                bookmarks_cache_ttl: Some(Duration::from_millis(5000)),
                bookmarks: vec![
                    BookmarkParams {
                        bookmark: BookmarkName::new("master").unwrap().into(),
                        hooks: vec!["hook1".to_string(), "rust:rusthook".to_string()],
                        only_fast_forward: false,
                        allowed_users: Some(Regex::new("^(svcscm|twsvcscm)$").unwrap().into()),
                        rewrite_dates: None,
                    },
                    BookmarkParams {
                        bookmark: Regex::new("[^/]*/stable").unwrap().into(),
                        hooks: vec![],
                        only_fast_forward: false,
                        allowed_users: None,
                        rewrite_dates: None,
                    },
                ],
                hooks: vec![
                    HookParams {
                        name: "hook1".to_string(),
                        config: HookConfig {
                            bypass: Some(HookBypass::CommitMessage("@allow_hook1".into())),
                            strings: hashmap! {},
                            ints: hashmap! {},
                        },
                    },
                    HookParams {
                        name: "rust:rusthook".to_string(),
                        config: HookConfig {
                            bypass: None,
                            strings: hashmap! {},
                            ints: hashmap! {
                                "int1".into() => 44,
                            },
                        },
                    },
                ],
                push: PushParams {
                    pure_push_allowed: false,
                    commit_scribe_category: Some("cat".to_string()),
                },
                pushrebase: PushrebaseParams {
                    flags: PushrebaseFlags {
                        rewritedates: false,
                        recursion_limit: Some(1024),
                        forbid_p2_root_rebases: false,
                        casefolding_check: false,
                        not_generated_filenodes_limit: 500,
                    },
                    block_merges: false,
                    emit_obsmarkers: false,
                    commit_scribe_category: None,
                    assign_globalrevs: false,
                    populate_git_mapping: false,
                },
                lfs: LfsParams {
                    threshold: Some(1000),
                    rollout_percentage: 56,
                    generate_lfs_blob_in_hg_sync_job: true,
                    rollout_smc_tier: Some("smc_tier".to_string()),
                },
                wireproto_logging: WireprotoLoggingConfig {
                    scribe_category: Some("category".to_string()),
                    storage_config_and_threshold: Some((
                        main_storage_config,
                        crate::convert::repo::DEFAULT_ARG_SIZE_THRESHOLD,
                    )),
                    local_path: None,
                },
                hash_validation_percentage: 0,
                readonly: RepoReadOnly::ReadWrite,
                redaction: Redaction::Enabled,
                skiplist_index_blobstore_key: Some("skiplist_key".into()),
                bundle2_replay_params: Bundle2ReplayParams {
                    preserve_raw_bundle2: true,
                },
                infinitepush: InfinitepushParams {
                    allow_writes: true,
                    namespace: Some(InfinitepushNamespace::new(Regex::new("foobar/.+").unwrap())),
                    hydrate_getbundle_response: false,
                    populate_reverse_filler_queue: false,
                    commit_scribe_category: None,
                },
                list_keys_patterns_max: 123,
                hook_max_file_size: 456,
                filestore: Some(FilestoreParams {
                    chunk_size: 768,
                    concurrency: 48,
                }),
                commit_sync_config: None,
                hipster_acl: Some("foo/test".to_string()),
                source_control_service: SourceControlServiceParams {
                    permit_writes: false,
                    permit_service_writes: false,
                    service_write_hipster_acl: None,
                    service_write_restrictions: Default::default(),
                },
                source_control_service_monitoring: Some(SourceControlServiceMonitoring {
                    bookmarks_to_report_age: vec![
                        BookmarkName::new("master").unwrap(),
                        BookmarkName::new("master2").unwrap(),
                    ],
                }),
                derived_data_config: DerivedDataConfig {
                    derived_data_types: btreeset![String::from("fsnodes")],
                    scuba_table: None,
                    unode_version: UnodeVersion::V2,
                    override_blame_filesize_limit: Some(101),
                },
                hgsql_name: HgsqlName("fbsource".to_string()),
                hgsql_globalrevs_name: HgsqlGlobalrevsName("fbsource".to_string()),
                enforce_lfs_acl_check: false,
                repo_client_use_warm_bookmarks_cache: true,
                segmented_changelog_config: SegmentedChangelogConfig { enabled: false },
            },
        );

        repos.insert(
            "www".to_string(),
            RepoConfig {
                enabled: true,
                storage_config: StorageConfig {
                    metadata: MetadataDatabaseConfig::Local(LocalDatabaseConfig {
                        path: "/tmp/www".into(),
                    }),
                    blobstore: BlobConfig::Files {
                        path: "/tmp/www".into(),
                    },
                },
                write_lock_db_address: None,
                generation_cache_size: 10 * 1024 * 1024,
                repoid: RepositoryId::new(1),
                scuba_table: Some("scuba_table".to_string()),
                scuba_local_path: None,
                scuba_table_hooks: Some("scm_hooks".to_string()),
                scuba_local_path_hooks: None,
                cache_warmup: None,
                hook_manager_params: None,
                bookmarks: vec![],
                bookmarks_cache_ttl: None,
                hooks: vec![],
                push: Default::default(),
                pushrebase: Default::default(),
                lfs: Default::default(),
                wireproto_logging: Default::default(),
                hash_validation_percentage: 0,
                readonly: RepoReadOnly::ReadWrite,
                redaction: Redaction::Enabled,
                skiplist_index_blobstore_key: None,
                bundle2_replay_params: Bundle2ReplayParams::default(),
                infinitepush: InfinitepushParams::default(),
                list_keys_patterns_max: LIST_KEYS_PATTERNS_MAX_DEFAULT,
                hook_max_file_size: HOOK_MAX_FILE_SIZE_DEFAULT,
                filestore: None,
                commit_sync_config: None,
                hipster_acl: None,
                source_control_service: SourceControlServiceParams::default(),
                source_control_service_monitoring: None,
                derived_data_config: DerivedDataConfig::default(),
                hgsql_name: HgsqlName("www-foobar".to_string()),
                hgsql_globalrevs_name: HgsqlGlobalrevsName("www-barfoo".to_string()),
                enforce_lfs_acl_check: false,
                repo_client_use_warm_bookmarks_cache: false,
                segmented_changelog_config: SegmentedChangelogConfig { enabled: false },
            },
        );
        assert_eq!(
            repoconfig.common,
            CommonConfig {
                security_config: vec![
                    AllowlistEntry::Tier("tier1".to_string()),
                    AllowlistEntry::HardcodedIdentity {
                        ty: "username".to_string(),
                        data: "user".to_string(),
                    },
                ],
                loadlimiter_category: Some("test-category".to_string()),
                scuba_censored_table: None
            }
        );
        assert_eq!(
            repoconfig.repos.get("www"),
            repos.get("www"),
            "www mismatch\ngot {:#?}\nwant {:#?}",
            repoconfig.repos.get("www"),
            repos.get("www")
        );
        assert_eq!(
            repoconfig.repos.get("fbsource"),
            repos.get("fbsource"),
            "fbsource mismatch\ngot {:#?}\nwant {:#?}",
            repoconfig.repos.get("fbsource"),
            repos.get("fbsource")
        );

        assert_eq!(
            &repoconfig.repos, &repos,
            "Repo mismatch:\n\
             got:\n\
             {:#?}\n\
             Want:\n\
             {:#?}",
            repoconfig.repos, repos
        )
    }

    #[fbinit::test]
    fn test_broken_bypass_config(fb: FacebookInit) {
        let content = r#"
            repoid=0
            storage_config = "sqlite"

            [storage.sqlite.metadata.local]
            local_db_path = "/tmp/fbsource"

            [storage.sqlite.blobstore.blob_files]
            path = "/tmp/fbsource"

            [[bookmarks]]
            name="master"
            [[bookmarks.hooks]]
            hook_name="hook1"
            [[hooks]]
            name="hook1"
            bypass_commit_string="@allow_hook1"
            bypass_pushvar="var=val"
        "#;

        let paths = btreemap! {
            "common/commitsyncmap.toml" => "",
            "repos/fbsource/server.toml" => content,
        };

        let tmp_dir = write_files(&paths);

        let res = load_repo_configs(fb, tmp_dir.path());
        let msg = format!("{:#?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("TooManyBypassOptions"));

        // Incorrect bypass string
        let content = r#"
            repoid=0
            storage_config = "sqlite"

            [storage.sqlite.metadata.local]
            local_db_path = "/tmp/fbsource"

            [storage.sqlite.blobstore.blob_files]
            path = "/tmp/fbsource"

            [[bookmarks]]
            name="master"
            [[bookmarks.hooks]]
            hook_name="hook1"
            [[hooks]]
            name="hook1"
            bypass_pushvar="var"
        "#;

        let paths = btreemap! {
            "common/commitsyncmap.toml" => "",
            "repos/fbsource/server.toml" => content,
        };

        let tmp_dir = write_files(&paths);

        let res = load_repo_configs(fb, tmp_dir.path());
        let msg = format!("{:#?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("InvalidPushvar"));
    }

    #[fbinit::test]
    fn test_broken_common_config(fb: FacebookInit) {
        fn check_fails(fb: FacebookInit, common: &str, expect: &str) {
            let content = r#"
                repoid = 0
                storage_config = "storage"

                [storage.storage.metadata.local]
                local_db_path = "/tmp/fbsource"

                [storage.storage.blobstore.blob_sqlite]
                path = "/tmp/fbsource"
            "#;

            let paths = btreemap! {
                "common/common.toml" => common,
                "common/commitsyncmap.toml" => "",
                "repos/fbsource/server.toml" => content,
            };

            let tmp_dir = write_files(&paths);

            let res = load_repo_configs(fb, tmp_dir.path());
            println!("res = {:?}", res);
            let msg = format!("{:?}", res);
            assert!(res.is_err(), "unexpected success for {}", common);
            assert!(
                msg.contains(expect),
                "wrong failure, wanted \"{}\" in {}",
                expect,
                common
            );
        }

        let common = r#"
        [[whitelist_entry]]
        identity_type="user"
        "#;
        check_fails(fb, common, "identity type and data must be specified");

        let common = r#"
        [[whitelist_entry]]
        identity_data="user"
        "#;
        check_fails(fb, common, "identity type and data must be specified");

        let common = r#"
        [[whitelist_entry]]
        tier="user"
        identity_type="user"
        identity_data="user"
        "#;
        check_fails(fb, common, "tier and identity cannot be both specified");

        // Only one tier is allowed
        let common = r#"
        [[whitelist_entry]]
        tier="tier1"
        [[whitelist_entry]]
        tier="tier2"
        "#;
        check_fails(fb, common, "only one tier is allowed");
    }

    #[fbinit::test]
    fn test_common_storage(fb: FacebookInit) {
        const STORAGE: &str = r#"
        [multiplex_store.metadata.remote]
        primary = { db_address = "some_db" }
        filenodes = { sharded = { shard_map = "some-shards", shard_num = 123 } }
        mutation = { db_address = "some_db" }

        [multiplex_store.blobstore.multiplexed]
        multiplex_id = 1
        components = [
            { blobstore_id = 1, blobstore = { blob_files = { path = "/tmp/foo" } } },
        ]
        queue_db = { remote = { db_address = "queue_db_address" } }
        "#;

        const REPO: &str = r#"
        repoid = 123
        storage_config = "multiplex_store"

        # Not overriding common store
        [storage.some_other_store.metadata.remote]
        primary = { db_address = "other_db" }
        filenodes = { sharded = { shard_map = "other-shards", shard_num = 20 } }

        [storage.some_other_store.blobstore]
        disabled = {}
        "#;

        let paths = btreemap! {
            "common/storage.toml" => STORAGE,
            "common/commitsyncmap.toml" => "",
            "repos/test/server.toml" => REPO,
        };

        let tmp_dir = write_files(&paths);

        let res = load_repo_configs(fb, tmp_dir.path()).expect("read configs failed");

        let expected = hashmap! {
            "test".into() => RepoConfig {
                enabled: true,
                storage_config: StorageConfig {
                    blobstore: BlobConfig::Multiplexed {
                        multiplex_id: MultiplexId::new(1),
                        scuba_table: None,
                        scuba_sample_rate: nonzero!(100u64),
                        blobstores: vec![
                            (BlobstoreId::new(1), MultiplexedStoreType::Normal, BlobConfig::Files {
                                path: "/tmp/foo".into()
                            })
                        ],
                        minimum_successful_writes: nonzero!(1usize),
                        queue_db: DatabaseConfig::Remote(
                            RemoteDatabaseConfig {
                                db_address: "queue_db_address".into(),
                            }
                        ),
                    },
                    metadata: MetadataDatabaseConfig::Remote(RemoteMetadataDatabaseConfig {
                        primary: RemoteDatabaseConfig {
                            db_address: "some_db".into(),
                        },
                        filenodes: ShardableRemoteDatabaseConfig::Sharded(ShardedRemoteDatabaseConfig {
                            shard_map: "some-shards".into(), shard_num: NonZeroUsize::new(123).unwrap()
                        }),
                        mutation: RemoteDatabaseConfig {
                            db_address: "some_db".into(),
                        },
                    }),
                },
                repoid: RepositoryId::new(123),
                generation_cache_size: 10 * 1024 * 1024,
                list_keys_patterns_max: LIST_KEYS_PATTERNS_MAX_DEFAULT,
                hook_max_file_size: HOOK_MAX_FILE_SIZE_DEFAULT,
                hgsql_name: HgsqlName("test".to_string()),
                hgsql_globalrevs_name: HgsqlGlobalrevsName("test".to_string()),
                ..Default::default()
            }
        };

        assert_eq!(
            res.repos, expected,
            "Got: {:#?}\nWant: {:#?}",
            &res.repos, expected
        )
    }

    #[fbinit::test]
    fn test_common_blobstores_local_override(fb: FacebookInit) {
        const STORAGE: &str = r#"
        [multiplex_store.metadata.remote]
        primary = { db_address = "some_db" }
        filenodes = { sharded = { shard_map = "some-shards", shard_num = 123 } }

        [multiplex_store.blobstore.multiplexed]
        components = [
            { blobstore_id = 1, blobstore = { blob_files = { path = "/tmp/foo" } } },
        ]

        [manifold_store.metadata.remote]
        primary = { db_address = "other_db" }
        filenodes = { sharded = { shard_map = "other-shards", shard_num = 456 } }
        mutation = { db_address = "other_mutation_db" }

        [manifold_store.blobstore.manifold]
        manifold_bucket = "bucketybucket"
        "#;

        const REPO: &str = r#"
        repoid = 123
        storage_config = "multiplex_store"

        # Override common store
        [storage.multiplex_store.metadata.remote]
        primary = { db_address = "other_other_db" }
        filenodes = { sharded = { shard_map = "other-other-shards", shard_num = 789 } }
        mutation = { db_address = "other_other_mutation_db" }

        [storage.multiplex_store.blobstore]
        disabled = {}
        "#;

        let paths = btreemap! {
            "common/storage.toml" => STORAGE,
            "common/commitsyncmap.toml" => "",
            "repos/test/server.toml" => REPO,
        };

        let tmp_dir = write_files(&paths);

        let res = load_repo_configs(fb, tmp_dir.path()).expect("read configs failed");

        let expected = hashmap! {
            "test".into() => RepoConfig {
                enabled: true,
                storage_config: StorageConfig {
                    blobstore: BlobConfig::Disabled,
                    metadata: MetadataDatabaseConfig::Remote( RemoteMetadataDatabaseConfig {
                        primary: RemoteDatabaseConfig { db_address: "other_other_db".into(), },
                        filenodes: ShardableRemoteDatabaseConfig::Sharded(ShardedRemoteDatabaseConfig { shard_map: "other-other-shards".into(), shard_num: NonZeroUsize::new(789).unwrap() }),
                        mutation: RemoteDatabaseConfig { db_address: "other_other_mutation_db".into(), },
                    }),

                },
                repoid: RepositoryId::new(123),
                generation_cache_size: 10 * 1024 * 1024,
                list_keys_patterns_max: LIST_KEYS_PATTERNS_MAX_DEFAULT,
                hook_max_file_size: HOOK_MAX_FILE_SIZE_DEFAULT,
                hgsql_name: HgsqlName("test".to_string()),
                hgsql_globalrevs_name: HgsqlGlobalrevsName("test".to_string()),
                ..Default::default()
            }
        };

        assert_eq!(
            res.repos, expected,
            "Got: {:#?}\nWant: {:#?}",
            &res.repos, expected
        )
    }

    #[fbinit::test]
    fn test_stray_fields(fb: FacebookInit) {
        const REPO: &str = r#"
        repoid = 123
        storage_config = "randomstore"

        [storage.randomstore.metadata.remote]
        primary = { db_address = "other_other_db" }

        [storage.randomstore.blobstore.blob_files]
        path = "/tmp/foo"

        # Should be above
        readonly = true
        "#;

        let paths = btreemap! {
            "common/commitsyncmap.toml" => "",
            "repos/test/server.toml" => REPO,
        };

        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(fb, tmp_dir.path());
        let msg = format!("{:#?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("unknown keys in config parsing"));
    }

    #[fbinit::test]
    fn test_multiplexed_store_types(fb: FacebookInit) {
        const STORAGE: &str = r#"
        [multiplex_store.metadata.remote]
        primary = { db_address = "some_db" }
        filenodes = { sharded = { shard_map = "some-shards", shard_num = 123 } }

        [multiplex_store.blobstore.multiplexed]
        multiplex_id = 1
        components = [
            { blobstore_id = 1, blobstore = { blob_files = { path = "/tmp/foo1" } } },
            { blobstore_id = 2, store_type = { normal = {}}, blobstore = { blob_files = { path = "/tmp/foo2" } } },
            { blobstore_id = 3, store_type = { write_mostly = {}}, blobstore = { blob_files = { path = "/tmp/foo3" } } },
        ]
        queue_db = { remote = { db_address = "queue_db_address" } }
        "#;

        const REPO: &str = r#"
        repoid = 123
        storage_config = "multiplex_store"
        "#;

        let paths = btreemap! {
            "common/storage.toml" => STORAGE,
            "common/commitsyncmap.toml" => "",
            "repos/test/server.toml" => REPO,
        };

        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(fb, tmp_dir.path()).expect("Read configs failed");

        if let BlobConfig::Multiplexed { blobstores, .. } =
            &res.repos["test"].storage_config.blobstore
        {
            let expected_blobstores = vec![
                (
                    BlobstoreId::new(1),
                    MultiplexedStoreType::Normal,
                    BlobConfig::Files {
                        path: "/tmp/foo1".into(),
                    },
                ),
                (
                    BlobstoreId::new(2),
                    MultiplexedStoreType::Normal,
                    BlobConfig::Files {
                        path: "/tmp/foo2".into(),
                    },
                ),
                (
                    BlobstoreId::new(3),
                    MultiplexedStoreType::WriteMostly,
                    BlobConfig::Files {
                        path: "/tmp/foo3".into(),
                    },
                ),
            ];

            assert_eq!(
                blobstores, &expected_blobstores,
                "Blobstores parsed from config are wrong"
            );
        } else {
            panic!("Multiplexed config is not a multiplexed blobstore");
        }
    }
}
