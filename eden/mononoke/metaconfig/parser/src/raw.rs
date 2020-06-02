/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::{BTreeSet, HashMap};
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Result};
use cached_config::ConfigStore;
use fbinit::FacebookInit;
use maplit::hashmap;
use repos::{
    RawCommitSyncConfig, RawCommonConfig, RawRepoConfig, RawRepoConfigs, RawStorageConfig,
};

use crate::errors::ErrorKind;

const CONFIGERATOR_CRYPTO_PROJECT: &'static str = "SCM";
const CONFIGERATOR_PREFIX: &'static str = "configerator://";

pub(crate) fn read_raw_configs(fb: FacebookInit, config_path: &Path) -> Result<RawRepoConfigs> {
    if config_path.starts_with(CONFIGERATOR_PREFIX) {
        let cfg_path = config_path
            .strip_prefix(CONFIGERATOR_PREFIX)?
            .to_string_lossy()
            .into_owned();
        let arc_conf = ConfigStore::signed_configerator(
            fb,
            None,
            hashmap! {
                cfg_path.clone() => CONFIGERATOR_CRYPTO_PROJECT.to_owned(),
            },
            None,
            Duration::from_secs(30),
        )?
        .get_config_handle::<RawRepoConfigs>(cfg_path)?
        .get();
        Ok((*arc_conf).clone())
    } else if config_path.is_dir() {
        read_raw_configs_toml(config_path)
    } else if config_path.is_file() {
        let repo_configs = std::fs::read(config_path)?;
        Ok(serde_json::from_slice(&repo_configs)?)
    } else {
        Err(
            ErrorKind::InvalidFileStructure(format!("{} does not exist", config_path.display()))
                .into(),
        )
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

    let mut repos = HashMap::new();
    let repos_dir = config_path.join("repos");
    if !repos_dir.is_dir() {
        return Err(ErrorKind::InvalidFileStructure(format!(
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
                ErrorKind::InvalidFileStructure(format!("invalid repo path {:?}", repo_config_path))
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

        return Err(ErrorKind::InvalidFileStructure(format!(
            "{} should be a file",
            path.display()
        ))
        .into());
    }
    let content = std::fs::read(path)?;
    let res = read_toml::<T>(&content);
    res
}

/// Helper to read toml files which throws an error upon encountering
/// unknown keys
fn read_toml<T>(bytes: &[u8]) -> Result<T>
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

            if unused.len() > 0 {
                Err(anyhow!("unknown keys in config parsing: `{:?}`", unused))?;
            }

            Ok(t)
        }
        Err(e) => Err(anyhow!("error parsing toml: {}", e)),
    }
}
