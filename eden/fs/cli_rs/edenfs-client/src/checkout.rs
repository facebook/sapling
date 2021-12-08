/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use edenfs_error::{EdenFsError, Result, ResultExt};
use edenfs_utils::path_from_bytes;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;
use thrift_types::edenfs::types::{MountInfo, MountState};
use toml::value::Value;
use uuid::Uuid;

use crate::EdenFsInstance;

// files in the client directory
const MOUNT_CONFIG: &str = "config.toml";

const SUPPORTED_REPOS: &[&str] = &["git", "hg", "recas"];
const SUPPORTED_MOUNT_PROTOCOLS: &[&str] = &["fuse", "nfs", "prjfs"];

#[derive(Debug)]
enum RedirectionType {
    /// Linux: a bind mount to a mkscratch generated path
    /// macOS: a mounted dmg file in a mkscratch generated path
    /// Windows: equivalent to symlink type
    Bind,
    /// A symlink to a mkscratch generated path
    Symlink,
}

impl FromStr for RedirectionType {
    type Err = EdenFsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "bind" {
            Ok(RedirectionType::Bind)
        } else if s == "symlink" {
            Ok(RedirectionType::Symlink)
        } else {
            Err(EdenFsError::ConfigurationError(format!(
                "Unknown redirection type: {}. Must be one of: bind, symlink",
                s
            )))
        }
    }
}

#[derive(Deserialize, Debug)]
struct Repository {
    path: PathBuf,

    #[serde(rename = "type", deserialize_with = "deserialize_repo_type")]
    repo_type: String,

    #[serde(
        deserialize_with = "deserialize_protocol",
        default = "default_protocol"
    )]
    protocol: String,

    #[serde(default = "default_guid")]
    guid: Uuid,

    #[serde(rename = "case-sensitive", default = "default_case_sensitive")]
    case_sensitive: bool,

    #[serde(rename = "require-utf8-path", default = "default_require_utf8_path")]
    require_utf8_path: bool,

    #[serde(rename = "enable-tree-overlay", default)]
    enable_tree_overlay: bool,
}

fn deserialize_repo_type<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;

    if SUPPORTED_REPOS.iter().any(|v| v == &s) {
        Ok(s)
    } else {
        Err(serde::de::Error::custom(format!(
            "Unsupported value: `{}`. Must be one of: {}",
            s,
            SUPPORTED_REPOS.join(", ")
        )))
    }
}

fn deserialize_protocol<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;

    if SUPPORTED_MOUNT_PROTOCOLS.iter().any(|v| v == &s) {
        Ok(s)
    } else {
        Err(serde::de::Error::custom(format!(
            "Unsupported value: `{}`. Must be one of: {}",
            s,
            SUPPORTED_MOUNT_PROTOCOLS.join(", ")
        )))
    }
}

fn default_protocol() -> String {
    if cfg!(windows) {
        "prjfs".to_string()
    } else {
        "fuse".to_string()
    }
}

fn default_guid() -> Uuid {
    Uuid::new_v4()
}

fn default_case_sensitive() -> bool {
    cfg!(linux)
}

fn default_require_utf8_path() -> bool {
    // Existing repositories may have non-utf8 files, thus allow them by default
    true
}

#[derive(Deserialize, Debug)]
struct PrefetchProfiles {
    #[serde(deserialize_with = "deserialize_active", default)]
    active: Vec<String>,
}

fn deserialize_active<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let unvalidated_arr: Vec<Value> = Vec::deserialize(deserializer)?;
    let mut arr = Vec::new();
    for val in unvalidated_arr {
        if let Some(s) = val.as_str() {
            arr.push(s.to_string());
        } else {
            return Err(serde::de::Error::custom(format!(
                "Unsupported [profiles] active type {}. Must be string.",
                val
            )));
        }
    }

    Ok(arr)
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct PredictivePrefetch {
    #[serde(default)]
    predictive_prefetch_active: bool,

    #[serde(default)]
    predictive_prefetch_num_dirs: u32,
}

#[derive(Deserialize, Debug)]
struct CheckoutConfig {
    repository: Repository,

    #[serde(deserialize_with = "deserialize_redirections", default)]
    redirections: Option<BTreeMap<String, RedirectionType>>,

    profiles: Option<PrefetchProfiles>,

    #[serde(rename = "predictive-prefetch", default)]
    predictive_prefetch: Option<PredictivePrefetch>,
}

impl CheckoutConfig {
    /// Reads checkout config information from config.toml and
    /// returns an Err if it is not properly formatted or does not exist.
    fn parse_config(state_dir: PathBuf) -> Result<CheckoutConfig> {
        let config_path = state_dir.join(MOUNT_CONFIG);
        let content = String::from_utf8(std::fs::read(config_path).from_err()?).from_err()?;
        let config: CheckoutConfig = toml::from_str(&content).from_err()?;
        Ok(config)
    }
}

fn deserialize_redirections<'de, D>(
    deserializer: D,
) -> Result<Option<BTreeMap<String, RedirectionType>>, D::Error>
where
    D: Deserializer<'de>,
{
    let unvalidated_map: BTreeMap<String, Value> = BTreeMap::deserialize(deserializer)?;
    let mut map = BTreeMap::new();
    for (key, value) in unvalidated_map {
        if let Some(s) = value.as_str() {
            map.insert(
                key,
                RedirectionType::from_str(s).map_err(serde::de::Error::custom)?,
            );
        } else {
            return Err(serde::de::Error::custom(format!(
                "Unsupported redirection value type {}. Must be string.",
                value
            )));
        }
    }

    Ok(Some(map))
}

/// Represents an edenfs checkout with mount information as well as information from configuration
#[derive(Serialize)]
pub struct EdenFsCheckout {
    #[serde(skip)]
    path: PathBuf,
    data_dir: PathBuf,
    /// This is None when it's just configured but not actively mounted in eden
    #[serde(serialize_with = "serialize_state")]
    state: Option<MountState>,
    /// If this is false, that means this model is only populated with mount info from edenfs
    /// As opposed to being populated with information from the configuration & live mount info.
    configured: bool,
    backing_repo: Option<PathBuf>,
}

impl EdenFsCheckout {
    fn from_mount_info(path: PathBuf, thrift_mount: MountInfo) -> Result<EdenFsCheckout> {
        Ok(EdenFsCheckout {
            path,
            data_dir: path_from_bytes(&thrift_mount.edenClientPath)?,
            state: Some(thrift_mount.state),
            configured: false,
            backing_repo: match thrift_mount.backingRepoPath {
                Some(path_string) => Some(path_from_bytes(&path_string)?),
                None => None,
            },
        })
    }

    fn from_config(path: PathBuf, data_dir: PathBuf, config: CheckoutConfig) -> EdenFsCheckout {
        EdenFsCheckout {
            path,
            data_dir,
            state: None,
            configured: true,
            backing_repo: Some(config.repository.path.clone()),
        }
    }

    fn update_with_config(&mut self, config: CheckoutConfig) {
        if self.backing_repo.is_none() {
            self.backing_repo = Some(config.repository.path.clone());
        }
        self.configured = true;
    }
}

fn serialize_state<S>(field: &Option<MountState>, s: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    s.serialize_str(&match *field {
        Some(state) => {
            format!("{}", state)
        }
        None => "NOT_RUNNING".to_string(),
    })
}

impl fmt::Display for EdenFsCheckout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let suffix = if self.configured {
            ""
        } else {
            " (unconfigured)"
        };

        let state_str = match self.state {
            Some(state) => {
                if state == MountState::RUNNING {
                    String::new()
                } else {
                    format!(" ({})", state)
                }
            }
            None => " (not mounted)".to_string(),
        };

        write!(f, "{}{}{}", self.path.display(), state_str, suffix)
    }
}

fn config_directory(instance: &EdenFsInstance, client_name: &String) -> PathBuf {
    instance.clients_dir().join(client_name.clone())
}

/// Return information about all checkouts defined in EdenFS's configuration files
/// and all information about mounted checkouts from the eden daemon
pub async fn get_mounts(instance: &EdenFsInstance) -> Result<BTreeMap<PathBuf, EdenFsCheckout>> {
    // Get all configured checkout info (including not mounted / not active ones) from configs
    let mut configs: Vec<(PathBuf, PathBuf, CheckoutConfig)> = Vec::new();
    for (mount_path, client_name) in instance.get_configured_mounts_map()? {
        configs.push((
            PathBuf::from(mount_path),
            config_directory(instance, &client_name),
            CheckoutConfig::parse_config(config_directory(instance, &client_name))?,
        ));
    }

    // Get active mounted checkouts info from eden daemon
    let client = instance.connect(Some(Duration::from_secs(3))).await;
    let mounted_checkouts = match client {
        Ok(client) => Some(client.listMounts().await.from_err()?),
        Err(_) => None, // eden daemon not running
    };

    // Combine mount info from active mounts and mount info from config files
    let mut mount_points = BTreeMap::new();
    if let Some(mounts) = mounted_checkouts {
        for thrift_mount in mounts {
            let path = path_from_bytes(&thrift_mount.mountPoint)?;
            mount_points.insert(
                path.clone(),
                EdenFsCheckout::from_mount_info(path.clone(), thrift_mount)?,
            );
        }
    }

    for (path, data_dir, config) in configs {
        match mount_points.get_mut(&path) {
            Some(mount_info) => {
                mount_info.update_with_config(config);
            }
            None => {
                mount_points.insert(
                    path.clone(),
                    EdenFsCheckout::from_config(path.clone(), data_dir, config),
                );
            }
        };
    }

    Ok(mount_points)
}
