/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::path::Path;

use anyhow::anyhow;
use anyhow::Result;
use cached_config::ConfigStore;
use repos::RawAclRegionConfig;
use repos::RawCommitSyncConfig;
use repos::RawCommonConfig;
use repos::RawRepoConfig;
use repos::RawRepoConfigs;
use repos::RawRepoDefinition;
use repos::RawRepoDefinitions;
use repos::RawStorageConfig;

use crate::errors::ConfigurationError;

const CONFIGERATOR_PREFIX: &str = "configerator://";

pub(crate) fn read_raw_configs(
    config_path: &Path,
    config_store: &ConfigStore,
) -> Result<RawRepoConfigs> {
    if config_path.starts_with(CONFIGERATOR_PREFIX) {
        let cfg_path = config_path
            .strip_prefix(CONFIGERATOR_PREFIX)?
            .to_string_lossy()
            .into_owned();
        let arc_conf = config_store
            .get_config_handle::<RawRepoConfigs>(cfg_path)?
            .get();
        Ok((*arc_conf).clone())
    } else if config_path.is_dir() {
        read_raw_configs_toml(config_path)
    } else if config_path.is_file() {
        let repo_configs = std::fs::read(config_path)?;
        Ok(serde_json::from_slice(&repo_configs)?)
    } else {
        Err(ConfigurationError::InvalidFileStructure(format!(
            "{} does not exist",
            config_path.display()
        ))
        .into())
    }
}

fn read_raw_configs_toml(config_path: &Path) -> Result<RawRepoConfigs> {
    let commit_sync = read_toml_path::<HashMap<String, RawCommitSyncConfig>>(
        config_path
            .join("common")
            .join("commitsyncmap.toml")
            .as_path(),
        false,
    )?;
    let common = read_toml_path::<RawCommonConfig>(
        config_path.join("common").join("common.toml").as_path(),
        true,
    )?;
    let storage = read_toml_path::<HashMap<String, RawStorageConfig>>(
        config_path.join("common").join("storage.toml").as_path(),
        true,
    )?;
    let acl_region_configs = read_toml_path::<HashMap<String, RawAclRegionConfig>>(
        config_path
            .join("common")
            .join("acl_regions.toml")
            .as_path(),
        true,
    )?;

    let mut repo_definitions_map = HashMap::new();
    let repo_definitions_dir = config_path.join("repo_definitions");
    if !repo_definitions_dir.is_dir() {
        return Err(ConfigurationError::InvalidFileStructure(format!(
            "expected 'repo_definitions' directory under {}",
            config_path.display()
        ))
        .into());
    }

    for entry in repo_definitions_dir.read_dir()? {
        let repo_definition_path = entry?.path();
        let reponame = repo_definition_path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| {
                ConfigurationError::InvalidFileStructure(format!(
                    "invalid repo path {:?}",
                    repo_definition_path
                ))
            })?
            .to_string();

        let repo_definition = read_toml_path::<RawRepoDefinition>(
            repo_definition_path.join("server.toml").as_path(),
            false,
        )?;
        repo_definitions_map.insert(reponame, repo_definition);
    }

    let repo_definitions = RawRepoDefinitions {
        repo_definitions: repo_definitions_map,
    };

    let mut repos = HashMap::new();
    let repos_dir = config_path.join("repos");
    if !repos_dir.is_dir() {
        return Err(ConfigurationError::InvalidFileStructure(format!(
            "expected 'repos' directory under {}",
            config_path.display()
        ))
        .into());
    }
    for entry in repos_dir.read_dir()? {
        let repo_config_path = entry?.path();
        let reponame = repo_config_path
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or_else(|| {
                ConfigurationError::InvalidFileStructure(format!(
                    "invalid repo path {:?}",
                    repo_config_path
                ))
            })?
            .to_string();

        let repo_config =
            read_toml_path::<RawRepoConfig>(repo_config_path.join("server.toml").as_path(), false)?;
        repos.insert(reponame, repo_config);
    }

    Ok(RawRepoConfigs {
        commit_sync,
        common,
        repos,
        storage,
        acl_region_configs,
        repo_definitions,
    })
}

fn read_toml_path<T>(path: &Path, defaults: bool) -> Result<T>
where
    T: serde::de::DeserializeOwned + Default,
{
    if !path.is_file() {
        if defaults && !path.exists() {
            return Ok(Default::default());
        }

        return Err(ConfigurationError::InvalidFileStructure(format!(
            "{} should be a file",
            path.display()
        ))
        .into());
    }
    let content = std::fs::read(path)?;
    read_toml::<T>(&content)
}

/// Helper to read toml files which throws an error upon encountering
/// unknown keys
pub(crate) fn read_toml<T>(bytes: &[u8]) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    match std::str::from_utf8(bytes) {
        Ok(s) => {
            let mut unused = BTreeSet::new();
            let de = &mut toml::de::Deserializer::new(s);
            let t: T = serde_ignored::deserialize(de, |path| {
                unused.insert(path.to_string());
            })?;

            if !unused.is_empty() {
                return Err(anyhow!("unknown keys in config parsing: `{:?}`", unused));
            }

            Ok(t)
        }
        Err(e) => Err(anyhow!("error parsing toml: {}", e)),
    }
}
