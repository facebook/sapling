/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Functions to load and parse Mononoke configuration.

use std::collections::HashMap;
use std::collections::HashSet;
use std::path::Path;
use std::str;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use cached_config::ConfigHandle;
use cached_config::ConfigStore;
use metaconfig_types::AsyncRequestsConfig;
use metaconfig_types::BlobConfig;
use metaconfig_types::CensoredScubaParams;
use metaconfig_types::CommonConfig;
use metaconfig_types::ObjectsCountMultiplier;
use metaconfig_types::Redaction;
use metaconfig_types::RedactionConfig;
use metaconfig_types::RepoConfig;
use metaconfig_types::RepoReadOnly;
use metaconfig_types::StorageConfig;
use mononoke_types::RepositoryId;
use repos::RawAclRegionConfig;
use repos::RawCommonConfig;
use repos::RawRepoConfig;
use repos::RawRepoConfigs;
use repos::RawRepoDefinition;
use repos::RawStorageConfig;

use crate::convert::Convert;
use crate::errors::ConfigurationError;

const LIST_KEYS_PATTERNS_MAX_DEFAULT: u64 = 500_000;
const HOOK_MAX_FILE_SIZE_DEFAULT: u64 = 8 * 1024 * 1024; // 8MiB

/// Load configuration common to all repositories.
pub fn load_common_config(
    config_path: impl AsRef<Path>,
    config_store: &ConfigStore,
) -> Result<CommonConfig> {
    let RawRepoConfigs {
        common, storage, ..
    } = crate::raw::read_raw_configs(config_path.as_ref(), config_store)?;
    parse_common_config(common, &storage)
}

/// Holds configuration for repositories.
#[derive(Clone, Debug, PartialEq)]
pub struct RepoConfigs {
    /// Configs for all repositories
    pub repos: HashMap<String, RepoConfig>,
    /// Common configs for all repos
    pub common: CommonConfig,
}

/// Provides an instance of ConfigHandle to the underlying
/// raw configuration if the config is backed by Configerator.
pub fn configerator_config_handle(
    config_path: &Path,
    config_store: &ConfigStore,
) -> Result<Option<ConfigHandle<RawRepoConfigs>>> {
    if config_path.starts_with(crate::raw::CONFIGERATOR_PREFIX) {
        let cfg_path = config_path
            .strip_prefix(crate::raw::CONFIGERATOR_PREFIX)?
            .to_string_lossy()
            .into_owned();
        let handle = config_store.get_config_handle::<RawRepoConfigs>(cfg_path)?;
        Ok(Some(handle))
    } else {
        Ok(None)
    }
}

/// Load configuration for repositories and storage.
pub fn load_repo_configs(
    config_path: impl AsRef<Path>,
    config_store: &ConfigStore,
) -> Result<RepoConfigs> {
    let raw_config = crate::raw::read_raw_configs(config_path.as_ref(), config_store)?;
    load_configs_from_raw(raw_config).map(|(repo_configs, _)| repo_configs)
}

/// Empty repo configs useful for testing purposes
pub fn load_empty_repo_configs() -> RepoConfigs {
    RepoConfigs {
        repos: HashMap::new(),
        common: CommonConfig::default(),
    }
}

/// Load configuration based on the provided raw configs.
pub fn load_configs_from_raw(
    raw_repo_configs: RawRepoConfigs,
) -> Result<(RepoConfigs, StorageConfigs)> {
    let RawRepoConfigs {
        commit_sync: _,
        common,
        repos,
        storage,
        acl_region_configs,
        repo_definitions,
    } = raw_repo_configs;
    let repo_definitions = repo_definitions.repo_definitions;
    let repo_configs = repos;
    let storage_configs = storage;

    let mut resolved_repo_configs = HashMap::new();
    let mut repoids = HashSet::new();

    for (reponame, raw_repo_definition) in repo_definitions.into_iter() {
        let repo_config = parse_with_repo_definition(
            raw_repo_definition,
            &repo_configs,
            &storage_configs,
            &acl_region_configs,
        )?;

        if !repoids.insert(repo_config.repoid) {
            return Err(ConfigurationError::DuplicatedRepoId(repo_config.repoid).into());
        }

        resolved_repo_configs.insert(reponame, repo_config);
    }

    let common = parse_common_config(common, &storage_configs)?;
    let storage = storage_configs
        .into_iter()
        .map(|(k, v)| Ok((k, v.convert()?)))
        .collect::<Result<_>>()?;
    Ok((
        RepoConfigs {
            repos: resolved_repo_configs,
            common,
        },
        StorageConfigs { storage },
    ))
}

fn parse_with_repo_definition(
    repo_definition: RawRepoDefinition,
    named_repo_configs: &HashMap<String, RawRepoConfig>,
    named_storage_configs: &HashMap<String, RawStorageConfig>,
    named_acl_region_configs: &HashMap<String, RawAclRegionConfig>,
) -> Result<RepoConfig> {
    let RawRepoDefinition {
        repo_id: repoid,
        repo_name,
        repo_config,
        hipster_acl,
        enabled,
        readonly,
        needs_backup: _,
        external_repo_id: _,
        backup_source_repo_name: _,
        acl_region_config,
        default_commit_identity_scheme,
        enable_git_bundle_uri,
    } = repo_definition;

    let enable_git_bundle_uri = enable_git_bundle_uri.unwrap_or(false);

    let default_commit_identity_scheme = default_commit_identity_scheme
        .convert()?
        .unwrap_or_default();

    let named_repo_config_name = repo_config
        .ok_or_else(|| ConfigurationError::InvalidConfig("No named_repo_config".to_string()))?;

    let named_repo_config = named_repo_configs
        .get(named_repo_config_name.as_str())
        .ok_or_else(|| {
            ConfigurationError::InvalidConfig(format!(
                "no named_repo_config \"{}\" for repo \"{:?}\".",
                named_repo_config_name, repo_name
            ))
        })?
        .clone();

    let RawRepoConfig {
        storage_config,
        storage,
        bookmarks,
        hook_manager_params,
        hooks,
        redaction,
        generation_cache_size,
        scuba_table_hooks,
        cache_warmup,
        push,
        pushrebase,
        lfs,
        hash_validation_percentage,
        infinitepush,
        list_keys_patterns_max,
        filestore,
        hook_max_file_size,
        source_control_service,
        source_control_service_monitoring,
        derived_data_config,
        scuba_local_path_hooks,
        enforce_lfs_acl_check,
        repo_client_use_warm_bookmarks_cache,
        repo_client_knobs,
        phabricator_callsign,
        walker_config,
        cross_repo_commit_validation_config,
        sparse_profiles_config,
        update_logging_config,
        commit_graph_config,
        deep_sharding_config,
        everstore_local_path,
        metadata_logger_config,
        commit_cloud_config,
        zelos_config,
        bookmark_name_for_objects_count,
        default_objects_count,
        override_objects_count,
        objects_count_multiplier,
        x_repo_sync_source_mapping,
        mononoke_cas_sync_config,
        git_configs,
        modern_sync_config,
        log_repo_stats,
        metadata_cache_config,
        ..
    } = named_repo_config;

    let named_storage_config = storage_config;

    let repoid = RepositoryId::new(repoid.context("missing repoid from configuration")?);

    let enabled = enabled.unwrap_or(true);

    let hooks: Vec<_> = hooks.unwrap_or_default().convert()?;

    let get_storage = move |name: &str| -> Result<StorageConfig> {
        let raw_storage_config = storage
            .as_ref()
            .and_then(|s| s.get(name))
            .or_else(|| named_storage_configs.get(name))
            .cloned()
            .ok_or_else(|| {
                ConfigurationError::InvalidConfig(format!("Storage \"{}\" not defined", name))
            })?;

        raw_storage_config.convert()
    };

    let storage_config = get_storage(
        &named_storage_config
            .ok_or_else(|| anyhow!("missing storage_config from configuration"))?,
    )?;

    let walker_config = walker_config.convert()?;

    let cache_warmup = cache_warmup.convert()?;

    let hook_manager_params = hook_manager_params.convert()?;

    let bookmarks = bookmarks.unwrap_or_default().convert()?;

    let push = push.convert()?.unwrap_or_default();

    let pushrebase = pushrebase.convert()?.unwrap_or_default();

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

    let enforce_lfs_acl_check = enforce_lfs_acl_check.unwrap_or(false);
    let repo_client_use_warm_bookmarks_cache =
        repo_client_use_warm_bookmarks_cache.unwrap_or(false);

    let repo_client_knobs = repo_client_knobs.convert()?.unwrap_or_default();

    let acl_region_config = acl_region_config
        .map(|key| {
            named_acl_region_configs.get(&key).cloned().ok_or_else(|| {
                ConfigurationError::InvalidConfig(format!(
                    "ACL region config \"{}\" not defined",
                    key
                ))
            })
        })
        .transpose()?
        .convert()?;

    let cross_repo_commit_validation_config = cross_repo_commit_validation_config.convert()?;

    let sparse_profiles_config = sparse_profiles_config.convert()?;

    let update_logging_config = update_logging_config.convert()?.unwrap_or_default();

    let commit_graph_config = commit_graph_config.convert()?.unwrap_or_default();
    let deep_sharding_config = deep_sharding_config.convert()?;
    let metadata_logger_config = metadata_logger_config.convert()?.unwrap_or_default();
    let zelos_config = zelos_config.convert()?;
    let x_repo_sync_source_mapping = x_repo_sync_source_mapping.convert()?;

    let raw_git_configs = git_configs.unwrap_or_default();

    let git_configs = raw_git_configs.convert()?;

    let commit_cloud_config = commit_cloud_config.convert()?.unwrap_or_default();
    let mononoke_cas_sync_config = mononoke_cas_sync_config.convert()?;
    let modern_sync_config = modern_sync_config.convert()?;
    let log_repo_stats = log_repo_stats.unwrap_or(false);
    let objects_count_multiplier = objects_count_multiplier.map(ObjectsCountMultiplier::new);
    let metadata_cache_config = metadata_cache_config
        .map(|cache_config| cache_config.convert())
        .transpose()?;
    Ok(RepoConfig {
        enabled,
        storage_config,
        generation_cache_size,
        repoid,
        scuba_table_hooks,
        scuba_local_path_hooks,
        cache_warmup,
        hook_manager_params,
        bookmarks,
        hooks,
        push,
        pushrebase,
        lfs,
        hash_validation_percentage,
        readonly,
        redaction,
        infinitepush,
        list_keys_patterns_max,
        filestore,
        hook_max_file_size,
        hipster_acl,
        source_control_service,
        source_control_service_monitoring,
        derived_data_config,
        enforce_lfs_acl_check,
        repo_client_use_warm_bookmarks_cache,
        repo_client_knobs,
        phabricator_callsign,
        acl_region_config,
        walker_config,
        cross_repo_commit_validation_config,
        sparse_profiles_config,
        update_logging_config,
        commit_graph_config,
        default_commit_identity_scheme,
        deep_sharding_config,
        everstore_local_path,
        metadata_logger_config,
        zelos_config,
        bookmark_name_for_objects_count,
        default_objects_count,
        override_objects_count,
        objects_count_multiplier,
        x_repo_sync_source_mapping,
        commit_cloud_config,
        mononoke_cas_sync_config,
        git_configs,
        modern_sync_config,
        log_repo_stats,
        metadata_cache_config,
        enable_git_bundle_uri,
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
    config_path: impl AsRef<Path>,
    config_store: &ConfigStore,
) -> Result<StorageConfigs> {
    let raw_config = crate::raw::read_raw_configs(config_path.as_ref(), config_store)?;
    load_configs_from_raw(raw_config).map(|(_, storage_configs)| storage_configs)
}

fn parse_common_config(
    common: RawCommonConfig,
    common_storage_config: &HashMap<String, RawStorageConfig>,
) -> Result<CommonConfig> {
    let trusted_parties_hipster_tier = common
        .trusted_parties_hipster_tier
        .filter(|tier| !tier.is_empty());
    let trusted_parties_allowlist = common
        .trusted_parties_allowlist
        .unwrap_or_default()
        .into_iter()
        .map(Convert::convert)
        .collect::<Result<Vec<_>>>()?;
    let global_allowlist = common
        .global_allowlist
        .unwrap_or_default()
        .into_iter()
        .map(Convert::convert)
        .collect::<Result<Vec<_>>>()?;
    let loadlimiter_category = common
        .loadlimiter_category
        .filter(|category| !category.is_empty());
    let scuba_censored_table = common.scuba_censored_table;
    let scuba_censored_local_path = common.scuba_local_path_censored;
    let internal_identity = common.internal_identity.convert()?;
    let git_memory_upper_bound = common
        .git_memory_upper_bound
        .map(|bound| bound.try_into())
        .transpose()?;
    let edenapi_dumper_scuba_table = common.edenapi_dumper_scuba_table;

    let censored_scuba_params = CensoredScubaParams {
        table: scuba_censored_table,
        local_path: scuba_censored_local_path,
    };

    let get_blobstore = |name| -> Result<BlobConfig> {
        Ok(common_storage_config
            .get(name)
            .cloned()
            .ok_or_else(|| {
                ConfigurationError::InvalidConfig(format!(
                    "Storage \"{}\" not defined for redaction config",
                    name
                ))
            })?
            .convert()?
            .blobstore)
    };

    let redaction_config = common.redaction_config;
    let redaction_config = RedactionConfig {
        blobstore: get_blobstore(&redaction_config.blobstore)?,
        redaction_sets_location: redaction_config.redaction_sets_location,
    };

    let async_requests_config = match common.async_requests_config {
        Some(config) => AsyncRequestsConfig {
            db_config: Some(config.db_config.convert()?),
            blobstore: Some(config.blobstore_config.convert()?),
        },
        None => AsyncRequestsConfig::default(),
    };

    Ok(CommonConfig {
        trusted_parties_hipster_tier,
        trusted_parties_allowlist,
        global_allowlist,
        loadlimiter_category,
        enable_http_control_api: common.enable_http_control_api,
        censored_scuba_params,
        redaction_config,
        internal_identity,
        git_memory_upper_bound,
        edenapi_dumper_scuba_table,
        async_requests_config,
    })
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
    use std::fs::create_dir_all;
    use std::fs::write;
    use std::num::NonZeroUsize;
    use std::sync::Arc;
    use std::time::Duration;

    use bookmarks_types::BookmarkKey;
    use cached_config::TestSource;
    use maplit::btreemap;
    use maplit::hashmap;
    use maplit::hashset;
    use metaconfig_types::AclRegion;
    use metaconfig_types::AclRegionConfig;
    use metaconfig_types::AclRegionRule;
    use metaconfig_types::Address;
    use metaconfig_types::BlameVersion;
    use metaconfig_types::BlobConfig;
    use metaconfig_types::BlobstoreId;
    use metaconfig_types::BookmarkParams;
    use metaconfig_types::BubbleDeletionMode;
    use metaconfig_types::CacheWarmupParams;
    use metaconfig_types::CommitCloudConfig;
    use metaconfig_types::CommitGraphConfig;
    use metaconfig_types::CommitIdentityScheme;
    use metaconfig_types::CommitSyncConfig;
    use metaconfig_types::CommitSyncConfigVersion;
    use metaconfig_types::CrossRepoCommitValidation;
    use metaconfig_types::DatabaseConfig;
    use metaconfig_types::DefaultSmallToLargeCommitSyncPathAction;
    use metaconfig_types::DerivedDataConfig;
    use metaconfig_types::DerivedDataTypesConfig;
    use metaconfig_types::EphemeralBlobstoreConfig;
    use metaconfig_types::FilestoreParams;
    use metaconfig_types::GitConcurrencyParams;
    use metaconfig_types::GitConfigs;
    use metaconfig_types::HookBypass;
    use metaconfig_types::HookConfig;
    use metaconfig_types::HookManagerParams;
    use metaconfig_types::HookParams;
    use metaconfig_types::Identity;
    use metaconfig_types::InfinitepushNamespace;
    use metaconfig_types::InfinitepushParams;
    use metaconfig_types::LfsParams;
    use metaconfig_types::LocalDatabaseConfig;
    use metaconfig_types::LoggingDestination;
    use metaconfig_types::MetadataCacheConfig;
    use metaconfig_types::MetadataCacheUpdateMode;
    use metaconfig_types::MetadataDatabaseConfig;
    use metaconfig_types::MetadataLoggerConfig;
    use metaconfig_types::MultiplexId;
    use metaconfig_types::MultiplexedStoreType;
    use metaconfig_types::PushParams;
    use metaconfig_types::PushrebaseFlags;
    use metaconfig_types::PushrebaseParams;
    use metaconfig_types::PushrebaseRemoteMode;
    use metaconfig_types::RemoteDatabaseConfig;
    use metaconfig_types::RemoteMetadataDatabaseConfig;
    use metaconfig_types::RepoClientKnobs;
    use metaconfig_types::ShardableRemoteDatabaseConfig;
    use metaconfig_types::ShardedDatabaseConfig;
    use metaconfig_types::ShardedRemoteDatabaseConfig;
    use metaconfig_types::ShardingModeConfig;
    use metaconfig_types::SmallRepoCommitSyncConfig;
    use metaconfig_types::SourceControlServiceMonitoring;
    use metaconfig_types::SourceControlServiceParams;
    use metaconfig_types::SparseProfilesConfig;
    use metaconfig_types::UnodeVersion;
    use metaconfig_types::UpdateLoggingConfig;
    use metaconfig_types::WalkerConfig;
    use metaconfig_types::XRepoSyncSourceConfig;
    use metaconfig_types::XRepoSyncSourceConfigMapping;
    use mononoke_macros::mononoke;
    use mononoke_types::DerivableType;
    use mononoke_types::NonRootMPath;
    use mononoke_types::path::MPath;
    use mononoke_types_mocks::changesetid::ONES_CSID;
    use mononoke_types_mocks::changesetid::THREES_CSID;
    use nonzero_ext::nonzero;
    use pretty_assertions::assert_eq;
    use regex::Regex;
    use repos::RawCommitSyncConfig;
    use tempfile::TempDir;

    use super::*;

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

    fn write_files(
        files: impl IntoIterator<Item = (impl AsRef<Path>, impl AsRef<[u8]>)>,
    ) -> TempDir {
        let tmp_dir = TempDir::with_prefix("mononoke_test_config.").expect("tmp_dir failed");

        // Always create repos directory and repo_definitions directory
        create_dir_all(tmp_dir.path().join("repos")).expect("create repos failed");
        create_dir_all(tmp_dir.path().join("repo_definitions"))
            .expect("create repo_definitions failed");

        for (path, content) in files.into_iter() {
            let path = path.as_ref();
            let content = content.as_ref();

            let dir = path.parent().expect("missing parent");
            create_dir_all(tmp_dir.path().join(dir)).expect("create dir failed");
            write(tmp_dir.path().join(path), content).expect("write failed");
        }

        tmp_dir
    }

    #[mononoke::test]
    fn test_commit_sync_config_correct() {
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
        let config_store = ConfigStore::new(Arc::new(TestSource::new()), None, None);
        let tmp_dir = write_files(&paths);
        let raw_config = crate::raw::read_raw_configs(tmp_dir.path(), &config_store)
            .expect("expect to read configs");
        let commit_sync = parse_commit_sync_config(raw_config.commit_sync)
            .expect("expected to get a commit sync config");

        let expected = hashmap! {
            "mega".to_owned() => CommitSyncConfig {
                large_repo_id: RepositoryId::new(1),
                common_pushrebase_bookmarks: vec![BookmarkKey::new("master").unwrap()],
                small_repos: hashmap! {
                    RepositoryId::new(2) => SmallRepoCommitSyncConfig {
                        default_action: DefaultSmallToLargeCommitSyncPathAction::Preserve,
                        map: hashmap! {
                            NonRootMPath::new("p1").unwrap() => NonRootMPath::new(".r2-legacy/p1").unwrap(),
                            NonRootMPath::new("p5").unwrap() => NonRootMPath::new(".r2-legacy/p5").unwrap(),
                        },
                        submodule_config: Default::default(),
                    },
                    RepositoryId::new(3) => SmallRepoCommitSyncConfig {
                        default_action: DefaultSmallToLargeCommitSyncPathAction::PrependPrefix(NonRootMPath::new("subdir").unwrap()),
                        map: hashmap! {
                            NonRootMPath::new("p1").unwrap() => NonRootMPath::new("p1").unwrap(),
                            NonRootMPath::new("p4").unwrap() => NonRootMPath::new("p5/p4").unwrap(),
                        },
                        submodule_config: Default::default(),
                    }
                },
                version_name: CommitSyncConfigVersion("TEST_VERSION_NAME".to_string()),
            }
        };

        assert_eq!(commit_sync, expected);
    }

    #[mononoke::test]
    fn test_commit_sync_config_large_is_small() {
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
        let config_store = ConfigStore::new(Arc::new(TestSource::new()), None, None);
        let RawRepoConfigs { commit_sync, .. } =
            crate::raw::read_raw_configs(tmp_dir.path(), &config_store).unwrap();
        for (_config_name, commit_sync_config) in commit_sync {
            let res = commit_sync_config.convert();
            let msg = format!("{:#?}", res);
            println!("res = {}", msg);
            assert!(res.is_err());
            assert!(msg.contains("is one of the small repos too"));
        }
    }

    #[mononoke::test]
    fn test_commit_sync_config_duplicated_small_repos() {
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
        let config_store = ConfigStore::new(Arc::new(TestSource::new()), None, None);
        let RawRepoConfigs { commit_sync, .. } =
            crate::raw::read_raw_configs(tmp_dir.path(), &config_store).unwrap();
        for (_config_name, commit_sync_config) in commit_sync {
            let res = commit_sync_config.convert();
            let msg = format!("{:#?}", res);
            println!("res = {}", msg);
            assert!(res.is_err());
            assert!(msg.contains("present multiple times in the same CommitSyncConfig"));
        }
    }
    #[mononoke::test]
    fn test_duplicated_repo_ids() {
        let www_content = r#"
            scuba_table_hooks="scm_hooks"
            storage_config="files"

            [storage.files.metadata.local]
            local_db_path = "/tmp/www"

            [storage.files.blobstore.blob_files]
            path = "/tmp/www"

            [storage.files.mutable_blobstore.blob_files]
            path = "/tmp/www_mutable"
        "#;
        let common_content = r#"
            loadlimiter_category="test-category"
            trusted_parties_hipster_tier = "tier1"

            [[global_allowlist]]
            identity_type = "username"
            identity_data = "user"
        "#;

        let www1_repo_def = r#"
            repo_id=1
            repo_name="www1"
            repo_config="www1"
        "#;

        let www2_repo_def = r#"
            repo_id=1
            repo_name="www2"
            repo_config="www2"
        "#;

        let paths = btreemap! {
            "common/common.toml" => common_content,
            "common/commitsyncmap.toml" => "",
            "repos/www1/server.toml" => www_content,
            "repos/www2/server.toml" => www_content,
            "repo_definitions/www1/server.toml" => www1_repo_def,
            "repo_definitions/www2/server.toml" => www2_repo_def,
        };

        let config_store = ConfigStore::new(Arc::new(TestSource::new()), None, None);
        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(tmp_dir.path(), &config_store);
        let msg = format!("{:#?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("DuplicatedRepoId"));
    }

    #[mononoke::test]
    fn test_read_manifest() {
        let fbsource_content = r#"
            generation_cache_size=1048576
            scuba_table_hooks="scm_hooks"
            storage_config="main"
            list_keys_patterns_max=123
            hook_max_file_size=456
            repo_client_use_warm_bookmarks_cache=true
            phabricator_callsign="FBS"

            [cache_warmup]
            bookmark="master"
            commit_limit=100
            [hook_manager_params]
            disable_acl_checker=false
            all_hooks_bypassed=false
            bypassed_commits_scuba_table="commits_bypassed_hooks"

            [derived_data_config]
            enabled_config_name = "default"

            [derived_data_config.available_configs.default]
            types = ["fsnodes", "unodes", "blame"]
            unode_version = 2
            blame_filesize_limit = 101
            derivation_batch_sizes = { "fsnodes" = 20, "unodes" = 20, "blame" = 20 }

            [derived_data_config.blocked_derivation.changesets]
            "3333333333333333333333333333333333333333333333333333333333333333" = { blocked_derived_data_types = ["unodes"] }

            [[bookmarks]]
            name="master"
            allowed_users="^(svcscm|twsvcscm)$"

            [[bookmarks.hooks]]
            hook_name="hook1"

            [[bookmarks.hooks]]
            hook_name="hook2a"

            [[bookmarks]]
            regex="[^/]*/stable"
            ensure_ancestor_of="master"
            allow_move_to_public_commits_without_hooks=true

            [[hooks]]
            name="hook1"
            bypass_commit_string="@allow_hook1"
            config_json = "{\"test\": \"abcde\"}"

            [[hooks]]
            name="hook2a"
            implementation="hook2"
            log_only=true
            config_ints={ int1 = 44 }
            config_ints_64={ int2 = 42 }
            [hooks.config_string_lists]
                list1 = ["val1", "val2"]

            [push]
            pure_push_allowed = false

            [pushrebase]
            rewritedates = false
            recursion_limit = 1024
            forbid_p2_root_rebases = false
            casefolding_check = false
            emit_obsmarkers = false
            allow_change_xrepo_mapping_extra = true

            [pushrebase.remote_mode]
            remote_land_service = { tier = "my-tier" }

            [lfs]
            threshold = 1000
            rollout_percentage = 56
            use_upstream_lfs_server = false

            [infinitepush]
            allow_writes = true
            namespace_pattern = "foobar/.+"

            [filestore]
            chunk_size = 768
            concurrency = 48

            [source_control_service_monitoring]
            bookmarks_to_report_age= ["master", "master2"]

            [repo_client_knobs]
            allow_short_getpack_history = true

            [walker_config]
            scrub_enabled = true
            validate_enabled = true

            [git_configs.git_concurrency]
            trees_and_blobs = 500
            commits = 1000
            tags = 1000
            shallow = 100

            [cross_repo_commit_validation_config]
            skip_bookmarks = ["weirdy"]

            [sparse_profiles_config]
            sparse_profiles_location = "sparse"

            [update_logging_config]
            new_commit_logging_destination = { scribe = { scribe_category = "cat" } }
            git_content_refs_logging_destination = { logger = {} }

            [commit_graph_config]
            scuba_table = "commit_graph"

            [metadata_logger_config]
            bookmarks = ["master", "release"]
            sleep_interval_secs = 100

            [metadata_cache_config]
            wbc_update_mode = { tailing = { category = "scribe_category" } }
            tags_update_mode = { polling = {} }

            [x_repo_sync_source_mapping.mapping.aros]
            bookmark_regex = "master"
            backsync_enabled = true

            [deep_sharding_config.status]
        "#;
        let fbsource_repo_def = r#"
            repo_id=0
            repo_name="fbsource"
            hipster_acl="foo/test"
            repo_config="fbsource"
            needs_backup=false
            acl_region_config="fbsource"
        "#;
        let www_content = r#"
            scuba_table_hooks="scm_hooks"
            storage_config="files"
            phabricator_callsign="WWW"
        "#;
        let www_repo_def = r#"
            repo_id=1
            repo_name="www"
            repo_config="www"
        "#;
        let common_content = r#"
            loadlimiter_category="test-category"
            scuba_censored_table="censored_table"
            scuba_local_path_censored="censored_local_path"
            trusted_parties_hipster_tier="tier1"
            git_memory_upper_bound=100
            edenapi_dumper_scuba_table="dumped_requests"

            [internal_identity]
            identity_type = "SERVICE_IDENTITY"
            identity_data = "internal"

            [redaction_config]
            blobstore="main"
            redaction_sets_location="loc"

            [[global_allowlist]]
            identity_type = "username"
            identity_data = "user"
        "#;

        let storage = r#"
        [main.metadata.remote]
        primary = { db_address = "db_address" }
        filenodes = { sharded = { shard_map = "db_address_shards", shard_num = 123 } }
        mutation = { db_address = "mutation_db_address" }
        sparse_profiles = { db_address = "sparse_profiles_db_address" }
        bonsai_blob_mapping = { sharded = { shard_map = "blob_mapping_shards", shard_num = 12 } }
        deletion_log = { db_address = "deletion_log" }
        commit_cloud = { db_address = "commit_cloud_db_address" }
        git_bundles = { db_address = "git_bundles" }
        repo_metadata = { db_address = "repo_metadata" }

        [main.blobstore.multiplexed_wal]
        multiplex_id = 1
        inner_blobstores_scuba_table = "blobstore_scuba_table"
        multiplex_scuba_table = "multiplex_scuba_table"
        write_quorum = 1
        components = [
            { blobstore_id = 0, blobstore = { manifold = { manifold_bucket = "bucket" } } },
            { blobstore_id = 1, blobstore = { blob_files = { path = "/tmp/foo" } } },
        ]
        queue_db = { remote = { shard_map = "queue_db_address", shard_num = 13 } }

        [files.metadata.local]
        local_db_path = "/tmp/www"

        [files.blobstore.blob_files]
        path = "/tmp/www"

        [main.mutable_blobstore.multiplexed_wal]
        multiplex_id = 1
        inner_blobstores_scuba_table = "blobstore_scuba_table"
        multiplex_scuba_table = "multiplex_scuba_table"
        write_quorum = 1
        components = [
            { blobstore_id = 0, blobstore = { manifold = { manifold_bucket = "bucket" } } },
            { blobstore_id = 1, blobstore = { blob_files = { path = "/tmp/foo" } } },
        ]
        queue_db = { remote = { shard_map = "queue_db_address", shard_num = 13 } }

        [files.mutable_blobstore.blob_files]
        path = "/tmp/www"

        [files.ephemeral_blobstore]
        initial_bubble_lifespan_secs = 86400
        bubble_expiration_grace_secs = 3600
        bubble_deletion_mode = 1

        [files.ephemeral_blobstore.metadata.local]
        local_db_path = "/tmp/www-ephemeral"

        [files.ephemeral_blobstore.blobstore.blob_files]
        path = "/tmp/www-ephemeral"
        "#;

        let acl_region_configs = r#"
        [[fbsource.allow_rules]]
        name = "name_test"
        hipster_acl = "acl_test"
        [[fbsource.allow_rules.regions]]
        roots = ["1111111111111111111111111111111111111111111111111111111111111111"]
        heads = []
        path_prefixes = ["test/prefix", ""]
        "#;

        let paths = btreemap! {
            "common/storage.toml" => storage,
            "common/common.toml" => common_content,
            "common/commitsyncmap.toml" => "",
            "common/acl_regions.toml" => acl_region_configs,
            "repos/fbsource/server.toml" => fbsource_content,
            "repos/www/server.toml" => www_content,
            "repo_definitions/fbsource/server.toml" => fbsource_repo_def,
            "repo_definitions/www/server.toml" => www_repo_def,
            "my_path/my_files" => "",
        };

        let config_store = ConfigStore::new(Arc::new(TestSource::new()), None, None);
        let tmp_dir = write_files(&paths);
        let repoconfig =
            load_repo_configs(tmp_dir.path(), &config_store).expect("Read configs failed");

        let multiplex = BlobConfig::MultiplexedWal {
            multiplex_id: MultiplexId::new(1),
            inner_blobstores_scuba_table: Some("blobstore_scuba_table".to_string()),
            multiplex_scuba_table: Some("multiplex_scuba_table".to_string()),
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
            write_quorum: 1,
            queue_db: ShardedDatabaseConfig::Sharded(ShardedRemoteDatabaseConfig {
                shard_map: "queue_db_address".into(),
                shard_num: nonzero!(13usize),
            }),
        };
        let main_storage_config = StorageConfig {
            blobstore: multiplex.clone(),
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
                sparse_profiles: RemoteDatabaseConfig {
                    db_address: "sparse_profiles_db_address".into(),
                },
                bonsai_blob_mapping: Some(ShardableRemoteDatabaseConfig::Sharded(
                    ShardedRemoteDatabaseConfig {
                        shard_map: "blob_mapping_shards".into(),
                        shard_num: NonZeroUsize::new(12).unwrap(),
                    },
                )),
                deletion_log: Some(RemoteDatabaseConfig {
                    db_address: "deletion_log".into(),
                }),
                commit_cloud: Some(RemoteDatabaseConfig {
                    db_address: "commit_cloud_db_address".into(),
                }),
                git_bundle_metadata: Some(RemoteDatabaseConfig {
                    db_address: "git_bundles".into(),
                }),
                repo_metadata: Some(RemoteDatabaseConfig {
                    db_address: "repo_metadata".into(),
                }),
            }),
            ephemeral_blobstore: None,
            mutable_blobstore: multiplex,
        };

        let mut repos = HashMap::new();
        repos.insert(
            "fbsource".to_string(),
            RepoConfig {
                enable_git_bundle_uri: false,
                enabled: true,
                default_commit_identity_scheme: CommitIdentityScheme::default(),
                storage_config: main_storage_config.clone(),
                generation_cache_size: 1024 * 1024,
                repoid: RepositoryId::new(0),
                scuba_table_hooks: Some("scm_hooks".to_string()),
                scuba_local_path_hooks: None,
                cache_warmup: Some(CacheWarmupParams {
                    bookmark: BookmarkKey::new("master").unwrap(),
                    commit_limit: 100,
                    microwave_preload: false,
                }),
                hook_manager_params: Some(HookManagerParams {
                    disable_acl_checker: false,
                    all_hooks_bypassed: false,
                    bypassed_commits_scuba_table: Some("commits_bypassed_hooks".to_string()),
                }),
                bookmarks: vec![
                    BookmarkParams {
                        bookmark: BookmarkKey::new("master").unwrap().into(),
                        hooks: vec!["hook1".to_string(), "hook2a".to_string()],
                        only_fast_forward: false,
                        allowed_users: Some(Regex::new("^(svcscm|twsvcscm)$").unwrap().into()),
                        allowed_hipster_group: None,
                        rewrite_dates: None,
                        hooks_skip_ancestors_of: vec![],
                        ensure_ancestor_of: None,
                        allow_move_to_public_commits_without_hooks: false,
                    },
                    BookmarkParams {
                        bookmark: Regex::new("[^/]*/stable").unwrap().into(),
                        hooks: vec![],
                        only_fast_forward: false,
                        allowed_users: None,
                        allowed_hipster_group: None,
                        rewrite_dates: None,
                        hooks_skip_ancestors_of: vec![],
                        ensure_ancestor_of: Some(BookmarkKey::new("master").unwrap()),
                        allow_move_to_public_commits_without_hooks: true,
                    },
                ],
                hooks: vec![
                    HookParams {
                        name: "hook1".to_string(),
                        implementation: "hook1".to_string(),
                        config: HookConfig {
                            bypass: Some(HookBypass::new_with_commit_msg("@allow_hook1".into())),
                            options: Some(r#"{"test": "abcde"}"#.to_string()),
                            log_only: false,
                            strings: hashmap! {},
                            ints: hashmap! {},
                            ints_64: hashmap! {},
                            string_lists: hashmap! {},
                            int_lists: hashmap! {},
                            int_64_lists: hashmap! {},
                        },
                    },
                    HookParams {
                        name: "hook2a".to_string(),
                        implementation: "hook2".to_string(),
                        config: HookConfig {
                            bypass: None,
                            options: None,
                            log_only: true,
                            strings: hashmap! {},
                            ints: hashmap! {
                                "int1".into() => 44,
                            },
                            ints_64: hashmap! {
                                "int2".into() => 42,
                            },
                            string_lists: hashmap! {
                                "list1".into() => vec!("val1".to_owned(), "val2".to_owned()),
                            },
                            int_lists: hashmap! {},
                            int_64_lists: hashmap! {},
                        },
                    },
                ],
                push: PushParams {
                    pure_push_allowed: false,
                    unbundle_commit_limit: None,
                },
                pushrebase: PushrebaseParams {
                    flags: PushrebaseFlags {
                        rewritedates: false,
                        recursion_limit: Some(1024),
                        forbid_p2_root_rebases: false,
                        casefolding_check: false,
                        casefolding_check_excluded_paths: Default::default(),
                        not_generated_filenodes_limit: 500,
                        monitoring_bookmark: None,
                    },
                    block_merges: false,
                    emit_obsmarkers: false,
                    globalrev_config: None,
                    populate_git_mapping: false,
                    allow_change_xrepo_mapping_extra: true,
                    remote_mode: PushrebaseRemoteMode::RemoteLandService(Address::Tier(
                        "my-tier".to_string(),
                    )),
                },
                lfs: LfsParams {
                    threshold: Some(1000),
                    rollout_percentage: 56,
                    use_upstream_lfs_server: false,
                },
                hash_validation_percentage: 0,
                readonly: RepoReadOnly::ReadWrite,
                redaction: Redaction::Enabled,
                infinitepush: InfinitepushParams {
                    allow_writes: true,
                    namespace: Some(InfinitepushNamespace::new(Regex::new("foobar/.+").unwrap())),
                    hydrate_getbundle_response: false,
                },
                list_keys_patterns_max: 123,
                hook_max_file_size: 456,
                filestore: Some(FilestoreParams {
                    chunk_size: 768,
                    concurrency: 48,
                }),
                hipster_acl: Some("foo/test".to_string()),
                source_control_service: SourceControlServiceParams {
                    permit_writes: false,
                    permit_service_writes: false,
                    service_write_hipster_acl: None,
                    permit_commits_without_parents: false,
                    service_write_restrictions: Default::default(),
                },
                source_control_service_monitoring: Some(SourceControlServiceMonitoring {
                    bookmarks_to_report_age: vec![
                        BookmarkKey::new("master").unwrap(),
                        BookmarkKey::new("master2").unwrap(),
                    ],
                }),
                derived_data_config: DerivedDataConfig {
                    enabled_config_name: "default".to_string(),
                    available_configs: hashmap!["default".to_string() => DerivedDataTypesConfig {
                        types: hashset! {
                            DerivableType::Fsnodes,
                            DerivableType::Unodes,
                            DerivableType::BlameV2,
                        },
                        ephemeral_bubbles_disabled_types: Default::default(),
                        mapping_key_prefixes: hashmap! {},
                        unode_version: UnodeVersion::V2,
                        blame_filesize_limit: Some(101),
                        hg_set_committer_extra: false,
                        blame_version: BlameVersion::V2,
                        git_delta_manifest_version: Default::default(),
                        git_delta_manifest_v2_config: Default::default(),
                        git_delta_manifest_v3_config: Default::default(),
                        derivation_batch_sizes: hashmap! {
                            DerivableType::Fsnodes => 20,
                            DerivableType::Unodes => 20,
                            DerivableType::BlameV2 => 20,
                        },
                        inferred_copy_from_config: Default::default(),
                    },],
                    scuba_table: None,
                    derivation_queue_scuba_table: None,
                    remote_derivation_config: None,
                    blocked_derivation: hashmap! {
                        THREES_CSID => Some(hashset! { DerivableType::Unodes, }),
                    },
                },
                enforce_lfs_acl_check: false,
                repo_client_use_warm_bookmarks_cache: true,
                repo_client_knobs: RepoClientKnobs {
                    allow_short_getpack_history: true,
                },
                phabricator_callsign: Some("FBS".to_string()),
                acl_region_config: Some(AclRegionConfig {
                    allow_rules: vec![AclRegionRule {
                        name: "name_test".to_string(),
                        regions: vec![AclRegion {
                            roots: vec![ONES_CSID],
                            heads: vec![],
                            path_prefixes: vec![MPath::new("test/prefix").unwrap(), MPath::ROOT],
                        }],
                        hipster_acl: "acl_test".to_string(),
                    }],
                }),
                walker_config: Some(WalkerConfig {
                    scrub_enabled: true,
                    validate_enabled: true,
                    params: None,
                }),
                cross_repo_commit_validation_config: Some(CrossRepoCommitValidation {
                    skip_bookmarks: [BookmarkKey::new("weirdy").unwrap()].into(),
                }),
                sparse_profiles_config: Some(SparseProfilesConfig {
                    sparse_profiles_location: "sparse".to_string(),
                    excluded_paths: vec![],
                    monitored_profiles: vec![],
                }),
                update_logging_config: UpdateLoggingConfig {
                    bookmark_logging_destination: None,
                    new_commit_logging_destination: Some(LoggingDestination::Scribe {
                        scribe_category: "cat".to_string(),
                    }),
                    git_content_refs_logging_destination: Some(LoggingDestination::Logger),
                },
                commit_graph_config: CommitGraphConfig {
                    scuba_table: Some("commit_graph".to_string()),
                    preloaded_commit_graph_blobstore_key: None,
                    disable_commit_graph_v2_with_empty_common: false,
                },
                deep_sharding_config: Some(ShardingModeConfig { status: hashmap!() }),
                everstore_local_path: None,

                metadata_logger_config: MetadataLoggerConfig {
                    bookmarks: vec![
                        BookmarkKey::new("master").unwrap(),
                        BookmarkKey::new("release").unwrap(),
                    ],
                    sleep_interval_secs: 100,
                },
                x_repo_sync_source_mapping: Some(XRepoSyncSourceConfigMapping {
                    mapping: hashmap! {
                        "aros".to_string() => XRepoSyncSourceConfig {
                            bookmark_regex: "master".to_string(),
                            backsync_enabled: true,
                        }
                    }
                    .into_iter()
                    .collect(),
                }),
                zelos_config: None,
                bookmark_name_for_objects_count: None,
                default_objects_count: None,
                override_objects_count: None,
                objects_count_multiplier: None,
                commit_cloud_config: CommitCloudConfig {
                    mocked_employees: Vec::new(),
                    disable_interngraph_notification: false,
                },
                mononoke_cas_sync_config: None,
                git_configs: GitConfigs {
                    git_concurrency: Some(GitConcurrencyParams {
                        trees_and_blobs: 500,
                        commits: 1000,
                        tags: 1000,
                        shallow: 100,
                    }),
                    git_lfs_interpret_pointers: false,
                    fetch_message: None,
                    git_bundle_uri: None,
                },
                modern_sync_config: None,
                log_repo_stats: false,
                metadata_cache_config: Some(MetadataCacheConfig {
                    wbc_update_mode: Some(MetadataCacheUpdateMode::Tailing {
                        category: "scribe_category".to_string(),
                    }),
                    tags_update_mode: Some(MetadataCacheUpdateMode::Polling),
                    content_refs_update_mode: None,
                }),
            },
        );

        repos.insert(
            "www".to_string(),
            RepoConfig {
                enable_git_bundle_uri: false,
                default_commit_identity_scheme: CommitIdentityScheme::default(),
                enabled: true,
                storage_config: StorageConfig {
                    metadata: MetadataDatabaseConfig::Local(LocalDatabaseConfig {
                        path: "/tmp/www".into(),
                    }),
                    blobstore: BlobConfig::Files {
                        path: "/tmp/www".into(),
                    },
                    ephemeral_blobstore: Some(EphemeralBlobstoreConfig {
                        blobstore: BlobConfig::Files {
                            path: "/tmp/www-ephemeral".into(),
                        },
                        metadata: DatabaseConfig::Local(LocalDatabaseConfig {
                            path: "/tmp/www-ephemeral".into(),
                        }),
                        initial_bubble_lifespan: Duration::from_secs(86400),
                        bubble_expiration_grace: Duration::from_secs(3600),
                        bubble_deletion_mode: BubbleDeletionMode::MarkOnly,
                    }),
                    mutable_blobstore: BlobConfig::Files {
                        path: "/tmp/www".into(),
                    },
                },
                generation_cache_size: 10 * 1024 * 1024,
                repoid: RepositoryId::new(1),
                scuba_table_hooks: Some("scm_hooks".to_string()),
                scuba_local_path_hooks: None,
                cache_warmup: None,
                hook_manager_params: None,
                bookmarks: vec![],
                hooks: vec![],
                push: Default::default(),
                pushrebase: Default::default(),
                lfs: Default::default(),
                hash_validation_percentage: 0,
                readonly: RepoReadOnly::ReadWrite,
                redaction: Redaction::Enabled,
                infinitepush: InfinitepushParams::default(),
                list_keys_patterns_max: LIST_KEYS_PATTERNS_MAX_DEFAULT,
                hook_max_file_size: HOOK_MAX_FILE_SIZE_DEFAULT,
                filestore: None,
                hipster_acl: None,
                source_control_service: SourceControlServiceParams::default(),
                source_control_service_monitoring: None,
                derived_data_config: DerivedDataConfig::default(),
                enforce_lfs_acl_check: false,
                repo_client_use_warm_bookmarks_cache: false,
                repo_client_knobs: RepoClientKnobs::default(),
                phabricator_callsign: Some("WWW".to_string()),
                acl_region_config: None,
                walker_config: None,
                cross_repo_commit_validation_config: None,
                sparse_profiles_config: None,
                update_logging_config: UpdateLoggingConfig::default(),
                commit_graph_config: CommitGraphConfig::default(),
                deep_sharding_config: None,
                everstore_local_path: None,
                metadata_logger_config: MetadataLoggerConfig::default(),
                zelos_config: None,
                bookmark_name_for_objects_count: None,
                default_objects_count: None,
                override_objects_count: None,
                objects_count_multiplier: None,
                x_repo_sync_source_mapping: None,
                commit_cloud_config: CommitCloudConfig::default(),
                mononoke_cas_sync_config: None,
                git_configs: GitConfigs {
                    git_concurrency: None,
                    git_lfs_interpret_pointers: false,
                    fetch_message: None,
                    git_bundle_uri: None,
                },
                modern_sync_config: None,
                log_repo_stats: false,
                metadata_cache_config: None,
            },
        );
        assert_eq!(
            repoconfig.common,
            CommonConfig {
                trusted_parties_hipster_tier: Some("tier1".to_string()),
                trusted_parties_allowlist: vec![],
                global_allowlist: vec![Identity {
                    id_type: "username".to_string(),
                    id_data: "user".to_string()
                }],
                loadlimiter_category: Some("test-category".to_string()),
                enable_http_control_api: false,
                censored_scuba_params: CensoredScubaParams {
                    table: Some("censored_table".to_string()),
                    local_path: Some("censored_local_path".to_string()),
                },
                redaction_config: RedactionConfig {
                    blobstore: main_storage_config.blobstore,
                    redaction_sets_location: "loc".to_string(),
                },
                internal_identity: Identity {
                    id_type: "SERVICE_IDENTITY".to_string(),
                    id_data: "internal".to_string(),
                },
                git_memory_upper_bound: Some(100),
                edenapi_dumper_scuba_table: Some("dumped_requests".to_string()),
                async_requests_config: AsyncRequestsConfig {
                    db_config: None,
                    blobstore: None
                },
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

    #[mononoke::test]
    fn test_broken_bypass_config() {
        // Incorrect bypass string
        let content = r#"
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

        let content_def = r#"
            repo_id = 0
            repo_name = "fbsource"
            repo_config = "fbsource"
        "#;

        let paths = btreemap! {
            "common/commitsyncmap.toml" => "",
            "repos/fbsource/server.toml" => content,
            "repo_definitions/fbsource/server.toml" => content_def,
        };

        let config_store = ConfigStore::new(Arc::new(TestSource::new()), None, None);
        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(tmp_dir.path(), &config_store);
        let msg = format!("{:#?}", res);
        println!("res = {}", msg);
        assert!(res.is_err());
        assert!(msg.contains("InvalidPushvar"));
    }

    #[mononoke::test]
    fn test_broken_common_config() {
        fn check_fails(common: &str, expect: &str) {
            let content = r#"
                storage_config = "storage"

                [storage.storage.metadata.local]
                local_db_path = "/tmp/fbsource"

                [storage.storage.blobstore.blob_sqlite]
                path = "/tmp/fbsource"

                [storage.storage.mutable_blobstore.blob_files]
                path = "/tmp/foo1"
            "#;

            let content_def = r#"
                repo_id = 0
                repo_name = "fbsource"
                repo_config = "fbsource"
            "#;

            let paths = btreemap! {
                "common/common.toml" => common,
                "common/commitsyncmap.toml" => "",
                "repos/fbsource/server.toml" => content,
                "repo_definitions/fbsource/server.toml" => content_def,
            };

            let config_store = ConfigStore::new(Arc::new(TestSource::new()), None, None);
            let tmp_dir = write_files(&paths);
            let res = load_repo_configs(tmp_dir.path(), &config_store);
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
        [[global_allowlist]]
        identity_type="user"
        "#;
        check_fails(common, "identity type and data must be specified");

        let common = r#"
        [[global_allowlist]]
        identity_data="user"
        "#;
        check_fails(common, "identity type and data must be specified");
    }

    #[mononoke::test]
    fn test_common_storage() {
        const STORAGE: &str = r#"
        [multiplex_store.metadata.remote]
        primary = { db_address = "some_db" }
        filenodes = { sharded = { shard_map = "some-shards", shard_num = 123 } }
        mutation = { db_address = "some_db" }
        sparse_profiles = { db_address = "some_db" }
        commit_cloud = { db_address = "some_db" }
        git_bundles = { db_address = "git_bundles" }
        repo_metadata = { db_address = "repo_metadata" }

        [multiplex_store.blobstore.multiplexed_wal]
        multiplex_id = 1
        components = [
            { blobstore_id = 1, blobstore = { blob_files = { path = "/tmp/foo" } } },
        ]
        queue_db = { remote = { shard_map = "queue_db_address", shard_num = 1 } }
        write_quorum = 1

        [multiplex_store.mutable_blobstore.multiplexed_wal]
        multiplex_id = 1
        components = [
            { blobstore_id = 1, blobstore = { blob_files = { path = "/tmp/foo" } } },
        ]
        queue_db = { remote = { shard_map = "queue_db_address", shard_num = 1 } }
        write_quorum = 1
        "#;

        const REPO: &str = r#"
        storage_config = "multiplex_store"

        # Not overriding common store
        [storage.some_other_store.metadata.remote]
        primary = { db_address = "other_db" }
        filenodes = { sharded = { shard_map = "other-shards", shard_num = 20 } }

        [storage.some_other_store.blobstore]
        disabled = {}
        "#;

        const REPO_DEF: &str = r#"
            repo_id = 123
            repo_config = "test"
            repo_name = "test"
        "#;

        const COMMON: &str = r#"
        [redaction_config]
        blobstore = "multiplex_store"
        redaction_sets_location = "loc"

        [internal_identity]
        identity_type = "SERVICE_IDENTITY"
        identity_data = "internal"
        "#;

        let paths = btreemap! {
            "common/storage.toml" => STORAGE,
            "common/common.toml" => COMMON,
            "common/commitsyncmap.toml" => "",
            "repos/test/server.toml" => REPO,
            "repo_definitions/test/server.toml" => REPO_DEF,
        };

        let config_store = ConfigStore::new(Arc::new(TestSource::new()), None, None);
        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(tmp_dir.path(), &config_store).expect("Read configs failed");

        let expected = hashmap! {
            "test".into() => RepoConfig {
                enabled: true,
                storage_config: StorageConfig {
                    blobstore: BlobConfig::MultiplexedWal {
                        multiplex_id: MultiplexId::new(1),
                        inner_blobstores_scuba_table: None,
                        multiplex_scuba_table: None,
                        scuba_sample_rate: nonzero!(100u64),
                        blobstores: vec![
                            (BlobstoreId::new(1), MultiplexedStoreType::Normal, BlobConfig::Files {
                                path: "/tmp/foo".into()
                            })
                        ],
                        write_quorum: 1,
                        queue_db: ShardedDatabaseConfig::Sharded(
                            ShardedRemoteDatabaseConfig {
                                shard_map: "queue_db_address".into(),
                                shard_num: nonzero!(1usize),
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
                        sparse_profiles: RemoteDatabaseConfig {
                            db_address: "some_db".into(),
                        },
                        bonsai_blob_mapping: None,
                        deletion_log: None,
                        commit_cloud:  Some(RemoteDatabaseConfig {
                            db_address: "some_db".into(),
                        }),
                        git_bundle_metadata:  Some(RemoteDatabaseConfig {
                            db_address: "git_bundles".into(),
                        }),
                        repo_metadata: Some(RemoteDatabaseConfig {
                            db_address: "repo_metadata".into(),
                        })
                    }),
                    ephemeral_blobstore: None,
                    mutable_blobstore: BlobConfig::MultiplexedWal {
                        multiplex_id: MultiplexId::new(1),
                        inner_blobstores_scuba_table: None,
                        multiplex_scuba_table: None,
                        scuba_sample_rate: nonzero!(100u64),
                        blobstores: vec![
                            (BlobstoreId::new(1), MultiplexedStoreType::Normal, BlobConfig::Files {
                                path: "/tmp/foo".into()
                            })
                        ],
                        write_quorum: 1,
                        queue_db: ShardedDatabaseConfig::Sharded(
                            ShardedRemoteDatabaseConfig {
                                shard_map: "queue_db_address".into(),
                                shard_num: nonzero!(1usize),
                            }
                        ),
                    },
                },
                repoid: RepositoryId::new(123),
                generation_cache_size: 10 * 1024 * 1024,
                list_keys_patterns_max: LIST_KEYS_PATTERNS_MAX_DEFAULT,
                hook_max_file_size: HOOK_MAX_FILE_SIZE_DEFAULT,
                ..Default::default()
            }
        };

        assert_eq!(
            res.repos, expected,
            "Got: {:#?}\nWant: {:#?}",
            &res.repos, expected
        )
    }

    #[mononoke::test]
    fn test_common_blobstores_local_override() {
        const STORAGE: &str = r#"
        [multiplex_store.metadata.remote]
        primary = { db_address = "some_db" }
        filenodes = { sharded = { shard_map = "some-shards", shard_num = 123 } }

        [multiplex_store.blobstore.multiplexed_wal]
        multiplex_id = 1
        components = [
            { blobstore_id = 1, blobstore = { blob_files = { path = "/tmp/foo" } } },
        ]
        queue_db = { remote = { shard_map = "queue_db_address", shard_num = 1 } }
        write_quorum = 1

        [multiplex_store.mutable_blobstore.multiplexed_wal]
        multiplex_id = 1
        components = [
            { blobstore_id = 1, blobstore = { blob_files = { path = "/tmp/foo" } } },
        ]
        queue_db = { remote = { shard_map = "queue_db_address", shard_num = 1 } }
        write_quorum = 1



        [manifold_store.metadata.remote]
        primary = { db_address = "other_db" }
        filenodes = { sharded = { shard_map = "other-shards", shard_num = 456 } }
        mutation = { db_address = "other_mutation_db" }

        [manifold_store.blobstore.manifold]
        manifold_bucket = "bucketybucket"

        [manifold_store.mutable_blobstore.manifold]
        manifold_bucket = "mutable_bucketybucket"
        "#;

        const REPO: &str = r#"
        storage_config = "multiplex_store"

        # Override common store
        [storage.multiplex_store.metadata.remote]
        primary = { db_address = "other_other_db" }
        filenodes = { sharded = { shard_map = "other-other-shards", shard_num = 789 } }
        mutation = { db_address = "other_other_mutation_db" }
        sparse_profiles = { db_address = "test_db" }
        commit_cloud = { db_address = "other_other_other_mutation_db" }
        git_bundles = { db_address = "git_bundles" }
        repo_metadata = { db_address = "repo_metadata" }

        [storage.multiplex_store.blobstore]
        disabled = {}

        [storage.multiplex_store.mutable_blobstore]
        disabled = {}
        "#;

        const REPO_DEF: &str = r#"
        repo_id = 123
        repo_config = "test"
        repo_name = "test"
        "#;

        const COMMON: &str = r#"
        [redaction_config]
        blobstore = "multiplex_store"
        redaction_sets_location = "loc"

        [internal_identity]
        identity_type = "SERVICE_IDENTITY"
        identity_data = "internal"
        "#;

        let paths = btreemap! {
            "common/storage.toml" => STORAGE,
            "common/common.toml" => COMMON,
            "common/commitsyncmap.toml" => "",
            "repos/test/server.toml" => REPO,
            "repo_definitions/test/server.toml" => REPO_DEF,
        };

        let config_store = ConfigStore::new(Arc::new(TestSource::new()), None, None);
        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(tmp_dir.path(), &config_store).expect("Read configs failed");

        let expected = hashmap! {
            "test".into() => RepoConfig {
                enabled: true,
                storage_config: StorageConfig {
                    blobstore: BlobConfig::Disabled,
                    metadata: MetadataDatabaseConfig::Remote( RemoteMetadataDatabaseConfig {
                        primary: RemoteDatabaseConfig { db_address: "other_other_db".into(), },
                        filenodes: ShardableRemoteDatabaseConfig::Sharded(ShardedRemoteDatabaseConfig { shard_map: "other-other-shards".into(), shard_num: NonZeroUsize::new(789).unwrap() }),
                        mutation: RemoteDatabaseConfig { db_address: "other_other_mutation_db".into(), },
                        sparse_profiles: RemoteDatabaseConfig { db_address: "test_db".into(), },
                        bonsai_blob_mapping: None,
                        deletion_log: None,
                        git_bundle_metadata: Some(RemoteDatabaseConfig { db_address: "git_bundles".into(), }),
                        commit_cloud: Some(RemoteDatabaseConfig { db_address: "other_other_other_mutation_db".into(), }),
                        repo_metadata: Some(RemoteDatabaseConfig { db_address: "repo_metadata".into() })
                    }),

                    ephemeral_blobstore: None,

                    mutable_blobstore: BlobConfig::Disabled,
                },
                repoid: RepositoryId::new(123),
                generation_cache_size: 10 * 1024 * 1024,
                list_keys_patterns_max: LIST_KEYS_PATTERNS_MAX_DEFAULT,
                hook_max_file_size: HOOK_MAX_FILE_SIZE_DEFAULT,
                ..Default::default()
            }
        };

        assert_eq!(
            res.repos, expected,
            "Got: {:#?}\nWant: {:#?}",
            &res.repos, expected
        )
    }

    #[mononoke::test]
    fn test_multiplexed_store_types() {
        const STORAGE: &str = r#"
        [multiplex_store.metadata.remote]
        primary = { db_address = "some_db" }
        filenodes = { sharded = { shard_map = "some-shards", shard_num = 123 } }

        [multiplex_store.blobstore.multiplexed_wal]
        multiplex_id = 1
        components = [
            { blobstore_id = 1, blobstore = { blob_files = { path = "/tmp/foo1" } } },
            { blobstore_id = 2, store_type = { normal = {}}, blobstore = { blob_files = { path = "/tmp/foo2" } } },
            { blobstore_id = 3, store_type = { write_only = {}}, blobstore = { blob_files = { path = "/tmp/foo3" } } },
        ]
        queue_db = { remote = { shard_map = "queue_db_address", shard_num = 1 } }
        write_quorum = 2

        [multiplex_store.mutable_blobstore.blob_files]
        path = "/tmp/foo4"

        "#;

        const REPO: &str = r#"
        storage_config = "multiplex_store"
        "#;

        const REPO_DEF: &str = r#"
        repo_id = 123
        repo_name = "test"
        repo_config = "test"
        "#;

        const COMMON: &str = r#"
        [redaction_config]
        blobstore = "multiplex_store"
        redaction_sets_location = "loc"

        [internal_identity]
        identity_type = "SERVICE_IDENTITY"
        identity_data = "internal"
        "#;

        let paths = btreemap! {
            "common/storage.toml" => STORAGE,
            "common/common.toml" => COMMON,
            "common/commitsyncmap.toml" => "",
            "repos/test/server.toml" => REPO,
            "repo_definitions/test/server.toml" => REPO_DEF,
        };

        let config_store = ConfigStore::new(Arc::new(TestSource::new()), None, None);
        let tmp_dir = write_files(&paths);
        let res = load_repo_configs(tmp_dir.path(), &config_store).expect("Read configs failed");

        if let BlobConfig::MultiplexedWal { blobstores, .. } =
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
                    MultiplexedStoreType::WriteOnly,
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
