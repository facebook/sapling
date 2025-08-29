/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fmt::Write as _;
#[cfg(unix)]
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
use std::io::prelude::*;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;
use std::time::Duration;
use std::vec;

use anyhow::Context;
use anyhow::anyhow;
use atomicfile::atomic_write;
use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use edenfs_config::EdenFsConfig;
use edenfs_error::ConnectAndRequestError;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use edenfs_utils::path_from_bytes;
#[cfg(windows)]
use edenfs_utils::strip_unc_prefix;
use edenfs_utils::varint::decode_varint;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde::ser::SerializeMap;
use strum::EnumString;
use strum::VariantNames;
use thrift_types::edenfs::GlobParams;
use thrift_types::edenfs::MountInfo;
use thrift_types::edenfs::MountState;
use thrift_types::edenfs::PredictiveFetch;
use thrift_types::edenfs::PrefetchParams;
use thrift_types::edenfs_clients::errors::PrefetchFilesError;
use thrift_types::fbthrift::ApplicationExceptionErrorCode;
use toml::value::Value;
use uuid::Uuid;

use crate::client::Client;
use crate::instance::EdenFsInstance;
use crate::methods::EdenThriftMethod;
use crate::redirect::REPO_SOURCE;
use crate::redirect::Redirection;
use crate::redirect::RedirectionType;
use crate::redirect::deserialize_redirections;

// files in the client directory (aka data_dir aka state_dir)
const MOUNT_CONFIG: &str = "config.toml";
const SNAPSHOT: &str = "SNAPSHOT";

// Magical snapshot strings
const SNAPSHOT_MAGIC_1: &[u8] = b"eden\x00\x00\x00\x01";
const SNAPSHOT_MAGIC_2: &[u8] = b"eden\x00\x00\x00\x02";
const SNAPSHOT_MAGIC_3: &[u8] = b"eden\x00\x00\x00\x03";
const SNAPSHOT_MAGIC_4: &[u8] = b"eden\x00\x00\x00\x04";

// List of supported repository types. This should stay in sync with the list
// in the Python CLI at fs/cli_rs/edenfs-client/src/checkout.rs and the list in
// the Daemon's CheckoutConfig at fs/config/CheckoutConfig.h.
#[derive(Deserialize, Serialize, Debug, PartialEq, VariantNames, EnumString)]
#[serde(rename_all = "lowercase")]
enum RepositoryType {
    #[strum(serialize = "git")]
    Git,
    #[strum(serialize = "hg")]
    Hg,
    #[strum(serialize = "recas")]
    Recas,
    #[strum(serialize = "filteredhg")]
    FilteredHg,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, VariantNames, EnumString)]
#[serde(rename_all = "lowercase")]
enum MountProtocol {
    #[strum(serialize = "fuse")]
    Fuse,
    #[strum(serialize = "nfs")]
    Nfs,
    #[strum(serialize = "prjfs")]
    Prjfs,
}

#[derive(Deserialize, Serialize, Debug, PartialEq, VariantNames, EnumString)]
#[serde(rename_all = "lowercase")]
enum InodeCatalogType {
    #[strum(serialize = "legacy")]
    Legacy,
    #[strum(serialize = "sqlite")]
    Sqlite,
    #[strum(serialize = "inmemory")]
    InMemory,
    #[strum(serialize = "lmdb")]
    Lmdb,
    #[strum(serialize = "legacydev")]
    LegacyDev,
}

#[derive(Debug)]
pub enum PrefetchProfilesResult {
    Prefetched,
    Skipped(String),
}

#[derive(Deserialize, Serialize, Debug)]
struct Repository {
    path: PathBuf,

    #[serde(rename = "type", deserialize_with = "deserialize_repo_type")]
    repo_type: RepositoryType,

    #[serde(default = "default_guid")]
    guid: Uuid,

    #[serde(
        deserialize_with = "deserialize_protocol",
        default = "default_protocol"
    )]
    protocol: MountProtocol,

    #[serde(rename = "case-sensitive", default = "default_case_sensitive")]
    case_sensitive: bool,

    #[serde(rename = "require-utf8-path", default = "default_require_utf8_path")]
    require_utf8_path: bool,

    #[serde(
        rename = "enable-sqlite-overlay",
        default = "default_sqlite_overlay",
        deserialize_with = "deserialize_sqlite_overlay"
    )]
    enable_sqlite_overlay: bool,

    #[serde(rename = "use-write-back-cache", default)]
    use_write_back_cache: bool,

    #[serde(
        rename = "enable-windows-symlinks",
        default = "default_enable_windows_symlinks",
        deserialize_with = "deserialize_enable_windows_symlinks"
    )]
    enable_windows_symlinks: bool,

    #[serde(
        rename = "inode-catalog-type",
        default = "default_inode_catalog_type",
        deserialize_with = "deserialize_inode_catalog_type"
    )]
    inode_catalog_type: Option<InodeCatalogType>,

    #[serde(rename = "off-mount-repo-dir", default)]
    off_mount_repo_dir: bool,
}

fn default_enable_windows_symlinks() -> bool {
    cfg!(target_os = "windows")
}

fn deserialize_enable_windows_symlinks<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s = bool::deserialize(deserializer)?;
    Ok(s)
}

fn default_sqlite_overlay() -> bool {
    cfg!(target_os = "windows")
}

fn deserialize_sqlite_overlay<'de, D>(deserializer: D) -> Result<bool, D::Error>
where
    D: Deserializer<'de>,
{
    let s = bool::deserialize(deserializer)?;

    if cfg!(target_os = "windows") {
        Ok(true)
    } else {
        Ok(s)
    }
}

fn deserialize_repo_type<'de, D>(deserializer: D) -> Result<RepositoryType, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;

    match RepositoryType::from_str(&s) {
        Ok(t) => Ok(t),
        Err(_) => Err(serde::de::Error::custom(format!(
            "Unsupported value: `{}`. Must be one of: {}",
            s,
            RepositoryType::VARIANTS.join(", ")
        ))),
    }
}

fn default_inode_catalog_type() -> Option<InodeCatalogType> {
    None
}

fn deserialize_inode_catalog_type<'de, D>(
    deserializer: D,
) -> Result<Option<InodeCatalogType>, D::Error>
where
    D: Deserializer<'de>,
{
    let s = Option::<String>::deserialize(deserializer)?;

    match s {
        None => Ok(None),
        Some(s) => match InodeCatalogType::from_str(&s) {
            Ok(t) => Ok(Some(t)),
            Err(_) => Err(serde::de::Error::custom(format!(
                "Unsupported value: `{}`. Must be one of: {}",
                s,
                InodeCatalogType::VARIANTS.join(", ")
            ))),
        },
    }
}

fn deserialize_protocol<'de, D>(deserializer: D) -> Result<MountProtocol, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;

    match MountProtocol::from_str(&s) {
        Ok(m) => Ok(m),
        Err(_) => Err(serde::de::Error::custom(format!(
            "Unsupported value: `{}`. Must be one of: {}",
            s,
            MountProtocol::VARIANTS.join(", ")
        ))),
    }
}

fn default_protocol() -> MountProtocol {
    if cfg!(windows) {
        MountProtocol::Prjfs
    } else {
        MountProtocol::Fuse
    }
}

fn default_guid() -> Uuid {
    Uuid::new_v4()
}

fn default_case_sensitive() -> bool {
    cfg!(target_os = "linux")
}

fn default_require_utf8_path() -> bool {
    // Existing repositories may have non-utf8 files, thus allow them by default
    true
}

#[derive(Deserialize, Serialize, Debug)]
struct PrefetchProfiles {
    #[serde(deserialize_with = "deserialize_active", default)]
    pub active: Vec<String>,
}

impl PrefetchProfiles {
    fn push(&mut self, profile: &str) {
        self.active.push(profile.into());
    }
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

#[derive(Deserialize, Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
struct PredictivePrefetch {
    #[serde(default)]
    predictive_prefetch_active: bool,

    #[serde(default)]
    predictive_prefetch_num_dirs: u32,
}

#[derive(Deserialize, Serialize, Debug)]
pub struct CheckoutConfig {
    repository: Repository,

    #[serde(
        deserialize_with = "deserialize_redirections",
        serialize_with = "serialize_path_map"
    )]
    redirections: BTreeMap<PathBuf, RedirectionType>,

    #[serde(
        rename = "redirection-targets",
        deserialize_with = "deserialize_redirection_targets",
        serialize_with = "serialize_path_map",
        default = "default_redirection_targets"
    )]
    redirection_targets: BTreeMap<PathBuf, PathBuf>,

    profiles: Option<PrefetchProfiles>,

    #[serde(rename = "predictive-prefetch", default)]
    predictive_prefetch: Option<PredictivePrefetch>,
}

// Initialize it to empty map to ensure backward compatibility
fn default_redirection_targets() -> BTreeMap<PathBuf, PathBuf> {
    BTreeMap::new()
}

fn serialize_path_map<V, S>(map: &BTreeMap<PathBuf, V>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
    V: Serialize,
{
    let mut map_serializer = serializer.serialize_map(Some(map.len()))?;
    for (key, value) in map {
        let serialized_key = if cfg!(windows) {
            // On Windows, we need to escape backslashes in the path, since
            // TOML uses backslashes as an escape character.
            // temp_key is used to make sure that the path is not already escaped
            // from previous serialization, since we don't want to double-escape it.
            let temp_key = key.display().to_string().replace("\\\\", "\\");
            temp_key.replace('\\', "\\\\")
        } else {
            key.to_string_lossy().into_owned()
        };
        match map_serializer.serialize_entry(&serialized_key, value) {
            Ok(_) => continue,
            Err(e) => {
                return Err(serde::ser::Error::custom(format!(
                    "Unsupported redirection. Target must be string. Error: {}",
                    e
                )));
            }
        }
    }
    map_serializer.end()
}

fn deserialize_redirection_targets<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<PathBuf, PathBuf>, D::Error>
where
    D: Deserializer<'de>,
{
    let unvalidated_map: BTreeMap<String, Value> = BTreeMap::deserialize(deserializer)?;
    let mut map = BTreeMap::new();
    for (key, value) in unvalidated_map {
        if let Some(s) = value.as_str() {
            map.insert(
                PathBuf::from(
                    // Convert path separator to backslash on Windows
                    if cfg!(windows) {
                        key.replace("\\\\", "\\").replace('/', "\\")
                    } else {
                        key
                    },
                ),
                PathBuf::from_str(s).map_err(serde::de::Error::custom)?,
            );
        } else {
            return Err(serde::de::Error::custom(format!(
                "Unsupported redirection target {}. Must be string.",
                value
            )));
        }
    }

    Ok(map)
}

impl CheckoutConfig {
    /// Reads checkout config information from config.toml and
    /// returns an Err if it is not properly formatted or does not exist.
    pub fn parse_config(state_dir: PathBuf) -> Result<CheckoutConfig> {
        let config_path = state_dir.join(MOUNT_CONFIG);
        let content = String::from_utf8(std::fs::read(config_path).from_err()?).from_err()?;
        let config: CheckoutConfig = toml::from_str(&content).from_err()?;
        Ok(config)
    }

    pub fn get_prefetch_profiles(&self) -> Result<&Vec<String>> {
        if let Some(profiles) = &self.profiles {
            Ok(&profiles.active)
        } else {
            Err(EdenFsError::Other(anyhow!(
                "Cannot get active prefetch profiles for {}",
                self.repository.path.display()
            )))
        }
    }

    pub fn contains_prefetch_profile(&self, profile: &str) -> bool {
        if let Some(profiles) = &self.profiles {
            profiles.active.iter().any(|x| x == profile)
        } else {
            false
        }
    }

    pub fn predictive_prefetch_is_active(&self) -> bool {
        if let Some(config) = &self.predictive_prefetch {
            config.predictive_prefetch_active
        } else {
            false
        }
    }

    pub fn get_predictive_num_dirs(&self) -> u32 {
        if let Some(config) = &self.predictive_prefetch {
            config.predictive_prefetch_num_dirs
        } else {
            0
        }
    }

    pub fn remove_prefetch_profile(&mut self, profile: &str, config_dir: PathBuf) -> Result<()> {
        if let Some(profiles) = &mut self.profiles {
            if profiles.active.iter().any(|x| x == profile) {
                profiles.active.retain(|x| *x != *profile);
                self.save_config(config_dir.clone()).with_context(|| {
                    anyhow!(
                        "failed to save config in the given config_dir: {}",
                        &config_dir.display()
                    )
                })?;
            }
        };
        Ok(())
    }

    pub fn update_redirections(
        &mut self,
        config_dir: &Path,
        redirs: &BTreeMap<PathBuf, Redirection>,
    ) -> Result<()> {
        self.redirections.clear();
        self.redirection_targets.clear();
        for (_, redir) in redirs.iter() {
            if redir.source != REPO_SOURCE {
                self.redirections
                    .insert(redir.repo_path(), redir.redir_type);
                self.redirection_targets.insert(
                    redir.repo_path(),
                    redir.target.clone().unwrap_or_else(|| PathBuf::from("")),
                );
            }
        }
        self.save_config(config_dir.into())?;
        Ok(())
    }

    pub fn remove_redirection_target(&mut self, config_dir: &Path, repo_path: &Path) -> Result<()> {
        self.redirection_targets.remove_entry(repo_path);
        self.save_config(config_dir.into())?;
        Ok(())
    }

    pub fn remove_redirection_targets(&mut self, config_dir: &Path) -> Result<()> {
        self.redirection_targets.clear();
        self.save_config(config_dir.into())?;
        Ok(())
    }

    /// Store information about the mount in the config.toml file.
    pub fn save_config(&mut self, state_dir: PathBuf) -> Result<()> {
        let toml_out = &toml::to_string(&self).with_context(|| {
            anyhow!(
                "could not toml-ize checkout config for repo '{}'",
                self.repository.path.display()
            )
        })?;
        let config_path = state_dir.join(MOUNT_CONFIG);
        // set default permissions to 0o644 (420 in decimal)
        #[cfg(windows)]
        let perm = 0o664;

        #[cfg(not(windows))]
        let perm = std::fs::metadata(&config_path)
            .map(|meta| meta.permissions().mode())
            .unwrap_or(0o664);

        atomic_write(config_path.as_path(), perm, true, |f| {
            f.write_all(toml_out.as_bytes())?;
            Ok(())
        })
        .from_err()?;
        Ok(())
    }

    /// Add a profile to the config (read the config file and write it back
    /// with profile added).
    pub fn activate_profile(&mut self, profile: &str, config_dir: PathBuf) -> Result<()> {
        if let Some(profiles) = &mut self.profiles {
            if profiles.active.iter().any(|x| x == profile) {
                // The profile is already activated so we don't need to update the profile list
                eprintln!("{} is already an active prefetch profile", profile);
                return Ok(());
            }

            profiles.push(profile);
            self.save_config(config_dir.clone()).with_context(|| {
                anyhow!(
                    "failed to save config in the given config_dir: {}",
                    &config_dir.display()
                )
            })?;
            Ok(())
        } else {
            Err(EdenFsError::Other(anyhow!(
                "failed to activate prefetch profile '{}'; could not find active profile list",
                profile
            )))
        }
    }

    /// Switch on predictive prefetch profiles (read the config file and write
    /// it back with predictive_prefetch_profiles_active set to True, set or
    /// update predictive_prefetch_num_dirs if specified).
    pub fn activate_predictive_profile(
        &mut self,
        config_dir: PathBuf,
        num_dirs: u32,
    ) -> Result<()> {
        if let Some(profiles) = &mut self.predictive_prefetch {
            if profiles.predictive_prefetch_active
                && num_dirs == profiles.predictive_prefetch_num_dirs
            {
                return Err(EdenFsError::Other(anyhow!(
                    "Predictive prefetch profiles are already activated \
                            with {} directories configured.",
                    num_dirs
                )));
            }
            profiles.predictive_prefetch_active = true;
            profiles.predictive_prefetch_num_dirs = num_dirs;
            self.save_config(config_dir.clone()).with_context(|| {
                anyhow!(
                    "failed to save config in the given config_dir: {}",
                    &config_dir.display()
                )
            })?;
        }
        Ok(())
    }

    /// Remove a profile to the config (read the config file and write it back
    /// with profile added).
    pub fn deactivate_profile(&mut self, profile: &str, config_dir: PathBuf) -> Result<()> {
        if let Some(profiles) = &mut self.profiles {
            if !profiles.active.iter().any(|x| x == profile) {
                return Err(EdenFsError::Other(anyhow!(
                    "Profile {} was not deactivated since it wasn't active.",
                    profile
                )));
            }
            profiles.active.retain(|x| *x != *profile);
            self.save_config(config_dir.clone()).with_context(|| {
                anyhow!(
                    "failed to save config in the given config_dir: {}",
                    &config_dir.display()
                )
            })?;
        };
        Ok(())
    }

    /// Switch off predictive prefetch profiles (read the config file and write
    /// it back with predictive_profile_profiles_active set to false. Also
    /// set predictive_prefetch_num_dirs to 0).
    pub fn deactivate_predictive_profile(&mut self, config_dir: PathBuf) -> Result<()> {
        if let Some(profiles) = &mut self.predictive_prefetch {
            if !profiles.predictive_prefetch_active {
                return Err(EdenFsError::Other(anyhow!(
                    "Predictive prefetch profile was not deactivated since it \
                    wasn't active."
                )));
            }
            profiles.predictive_prefetch_active = false;
            profiles.predictive_prefetch_num_dirs = 0;
            self.save_config(config_dir.clone()).with_context(|| {
                anyhow!(
                    "failed to save config in the given config_dir: {}",
                    &config_dir.display()
                )
            })?;
        };
        Ok(())
    }
}

pub struct SnapshotState {
    pub working_copy_parent: String,
    pub last_checkout_hash: String,
    pub parent_filter_id: Option<String>,
    pub last_filter_id: Option<String>,
}

impl SnapshotState {
    fn new(
        working_copy_parent: String,
        last_checkout_hash: String,
        parent_filter_id: Option<String>,
        last_filter_id: Option<String>,
    ) -> Self {
        Self {
            working_copy_parent,
            last_checkout_hash,
            parent_filter_id,
            last_filter_id,
        }
    }
}

/// Represents an edenfs checkout with mount information as well as information from configuration
#[derive(Serialize)]
pub struct EdenFsCheckout {
    /// E.g., /data/sandcastle/boxes/fbsource
    #[serde(skip)]
    path: PathBuf,
    /// E.g., /home/unixname/local/.eden/clients/fbsource
    data_dir: PathBuf,
    /// This is None when it's just configured but not actively mounted in eden
    #[serde(serialize_with = "serialize_state")]
    state: Option<MountState>,
    /// If this is false, that means this model is only populated with mount info from edenfs
    /// As opposed to being populated with information from the configuration & live mount info.
    configured: bool,
    backing_repo: Option<PathBuf>,
    #[serde(skip)]
    pub(crate) redirections: Option<BTreeMap<PathBuf, RedirectionType>>,
    #[serde(skip)]
    pub(crate) redirection_targets: Option<BTreeMap<PathBuf, PathBuf>>,
}

impl EdenFsCheckout {
    pub fn path(&self) -> PathBuf {
        self.path.clone()
    }

    pub fn data_dir(&self) -> PathBuf {
        self.data_dir.clone()
    }

    pub fn fsck_dir(&self) -> PathBuf {
        self.data_dir.join("fsck")
    }

    fn encode_hex(bytes: &[u8]) -> String {
        let mut s = String::with_capacity(bytes.len() * 2);
        for &b in bytes {
            write!(&mut s, "{:02x}", b).unwrap();
        }
        s
    }

    /// Determines the hash and filter id for a given Snapshot component.
    pub fn parse_snapshot_component(
        &self,
        component_buf: &Vec<u8>,
    ) -> Result<(String, Option<String>)> {
        let checkout_config = CheckoutConfig::parse_config(self.data_dir.clone())?;

        if checkout_config.repository.repo_type == RepositoryType::FilteredHg {
            // FilteredRootIds are in the form: <VarInt><RootId><FilterId>. We first parse out the
            // VarInt to determine where the RootId ends.
            let cursor = std::io::Cursor::new(component_buf);
            let (component_hash_len, varint_len) = decode_varint(&mut BufReader::new(cursor))
                .context("Could not decode varint in Snapshot file")?;
            let filter_offset = varint_len + (component_hash_len as usize);

            // We can then parse out the RootId, FilterId, and convert them into strings.
            let decoded_hash = std::str::from_utf8(&component_buf[varint_len..filter_offset])
                .from_err()?
                .to_string();
            let decoded_filter = std::str::from_utf8(&component_buf[filter_offset..])
                .from_err()?
                .to_string();
            Ok((decoded_hash, Some(decoded_filter)))
        } else {
            // The entire buffer corresponds to the hash. There is no filter id present.
            let decoded_hash = std::str::from_utf8(component_buf).from_err()?.to_string();
            Ok((decoded_hash, None))
        }
    }

    /// Returns a SnapshotState representing EdenFS working copy parent as well as the last checked
    /// out revision.
    pub fn get_snapshot(&self) -> Result<SnapshotState> {
        let snapshot_path = self.data_dir.join(SNAPSHOT);
        let mut f = File::open(snapshot_path).from_err()?;
        let mut header = [0u8; 8];
        f.read(&mut header).from_err()?;

        if header == SNAPSHOT_MAGIC_1 {
            let mut snapshot = [0u8; 20];
            f.read(&mut snapshot).from_err()?;
            let decoded = EdenFsCheckout::encode_hex(&snapshot);
            Ok(SnapshotState::new(decoded.clone(), decoded, None, None))
        } else if header == SNAPSHOT_MAGIC_2 {
            // The first byte of the snapshot file is the length of the working copy parent.
            let body_length = f.read_u32::<BigEndian>().from_err()?;
            let mut buf = vec![0u8; body_length as usize];
            f.read_exact(&mut buf).from_err()?;

            // We must parse out the working copy parent hash. For Filtered repos, we also have to
            // parse out the active filter id.
            let (decoded_hash, decoded_filter) = self
                .parse_snapshot_component(&buf)
                .context("Could not parse snapshot component")?;
            Ok(SnapshotState::new(
                decoded_hash.clone(),
                decoded_hash,
                decoded_filter.clone(),
                decoded_filter,
            ))
        } else if header == SNAPSHOT_MAGIC_3 {
            let _pid = f.read_u32::<BigEndian>().from_err()?;

            let from_length = f.read_u32::<BigEndian>().from_err()?;
            let mut from_buf = vec![0u8; from_length as usize];
            f.read_exact(&mut from_buf).from_err()?;

            let to_length = f.read_u32::<BigEndian>().from_err()?;
            let mut to_buf = vec![0u8; to_length as usize];
            f.read_exact(&mut to_buf).from_err()?;

            let (from_hash, from_filter) = self
                .parse_snapshot_component(&from_buf)
                .context("Could not parse snapshot component")?;

            let (to_hash, to_filter) = self
                .parse_snapshot_component(&to_buf)
                .context("Could not parse snapshot component")?;

            // TODO(xavierd): return a proper object that the caller could use.
            Err(EdenFsError::Other(anyhow!(
                "A checkout operation is ongoing from {} (filter: {:?}) to {} (filter: {:?})",
                from_hash,
                from_filter,
                to_hash,
                to_filter,
            )))
        } else if header == SNAPSHOT_MAGIC_4 {
            let working_copy_parent_length = f.read_u32::<BigEndian>().from_err()?;
            let mut working_copy_parent_buf = vec![0u8; working_copy_parent_length as usize];
            f.read_exact(&mut working_copy_parent_buf).from_err()?;

            let checked_out_length = f.read_u32::<BigEndian>().from_err()?;
            let mut checked_out_buf = vec![0u8; checked_out_length as usize];
            f.read_exact(&mut checked_out_buf).from_err()?;

            let (parent_hash, parent_filter) = self
                .parse_snapshot_component(&working_copy_parent_buf)
                .context("Could not parse snapshot component")?;

            let (checked_out_hash, checked_out_filter) = self
                .parse_snapshot_component(&checked_out_buf)
                .context("Could not parse snapshot component")?;

            Ok(SnapshotState::new(
                parent_hash,
                checked_out_hash,
                parent_filter,
                checked_out_filter,
            ))
        } else {
            Err(EdenFsError::Other(anyhow!(
                "SNAPSHOT file has invalid header"
            )))
        }
    }

    pub fn backing_repo(&self) -> Option<PathBuf> {
        self.backing_repo.clone()
    }

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
            redirections: None,
            redirection_targets: None,
        })
    }

    fn from_config(path: PathBuf, data_dir: PathBuf, config: CheckoutConfig) -> EdenFsCheckout {
        EdenFsCheckout {
            path,
            data_dir,
            state: None,
            configured: true,
            backing_repo: Some(config.repository.path.clone()),
            redirections: Some(config.redirections),
            redirection_targets: Some(config.redirection_targets),
        }
    }

    fn update_with_config(&mut self, config: CheckoutConfig) {
        if self.backing_repo.is_none() {
            self.backing_repo = Some(config.repository.path.clone());
        }
        self.configured = true;
    }

    pub fn get_contents_for_profile(
        &self,
        profile: &String,
        silent: bool,
    ) -> Result<HashSet<String>> {
        const RELATIVE_PROFILES_LOCATION: &str = "xplat/scm/prefetch_profiles/profiles";
        let profile_path = self.path.join(RELATIVE_PROFILES_LOCATION).join(profile);

        if !profile_path.exists() {
            if !silent {
                eprintln!(
                    "Profile '{}' not found for checkout {}.",
                    profile,
                    self.path().display()
                );
            }
            return Ok(HashSet::new());
        }

        let file = File::open(&profile_path).with_context(|| {
            anyhow!("Sparse profile '{}' does not exist", profile_path.display())
        })?;
        Ok(BufReader::new(file)
            .lines()
            .collect::<std::io::Result<HashSet<_>>>()
            .with_context(|| {
                anyhow!(
                    "Cannot read contents for prefetch profile '{}'",
                    profile_path.display()
                )
            })?)
    }

    /// Function to actually cause the prefetch, can be called on a background
    /// process or in the main process.
    /// Only print here if silent is False, as that could send messages
    /// randomly to stdout.
    pub async fn make_prefetch_request(
        &self,
        instance: &EdenFsInstance,
        all_profile_contents: HashSet<String>,
        directories_only: bool,
        silent: bool,
        revisions: Option<&Vec<String>>,
        predict_revisions: bool,
        background: bool,
        predictive: bool,
        predictive_num_dirs: u32,
    ) -> Result<()> {
        let mut commit_vec = vec![];
        if predict_revisions {
            // The arc and hg commands need to be run in the mount mount, so we need
            // to change the working path if it is not within the mount.
            let cwd = env::current_dir().context("Unable to get current working directory")?;
            let mut changed_dir = false;
            if find_checkout(instance, &cwd).is_err() {
                println!("Setting the current working directory");
                env::set_current_dir(&self.path).with_context(|| {
                    anyhow!(
                        "failed to change working directory to '{}'",
                        self.path.display()
                    )
                })?;
                changed_dir = true;
            }

            let output = Command::new("arc")
                .arg("stable")
                .arg("best")
                .arg("--verbose")
                .arg("error")
                .output()
                .with_context(|| {
                    anyhow!("Failed to execute subprocess `arc stable best --verbose error`")
                })?;
            if !output.status.success() {
                return Err(EdenFsError::Other(anyhow!(
                    "Unable to predict commits to prefetch, error finding bookmark \
            to prefetch: {}",
                    String::from_utf8_lossy(output.stderr.as_slice())
                )));
            }

            let bookmark = String::from_utf8_lossy(output.stdout.as_slice());
            let bookmark = bookmark.trim();

            let output = Command::new("hg")
                .arg("log")
                .arg("-r")
                .arg(bookmark)
                .arg("-T")
                .arg("{node}")
                .output()
                .with_context(|| {
                    anyhow!(
                        "Failed to execute subprocess `hg log -r {} -T {{node}}`",
                        bookmark
                    )
                })?;

            if !output.status.success() {
                return Err(EdenFsError::Other(anyhow!(
                    "Unable to predict commits to prefetch, error converting \
                bookmark to commit: {}",
                    String::from_utf8_lossy(output.stderr.as_slice())
                )));
            }

            // If we changed directories to run the subcommands, we should switch
            // back to our previous location
            if changed_dir {
                env::set_current_dir(&cwd)
                    .context("failed to change back to old working directory")?;
            }

            let commit = String::from_utf8_lossy(output.stdout.as_slice());
            let commit = commit.trim().as_bytes().to_vec();
            commit_vec.push(commit);
        }

        if let Some(revs) = revisions {
            for rev in revs {
                let commit = rev.trim().as_bytes().to_vec();
                commit_vec.push(commit);
            }
        }

        let client = instance.get_client();

        let mnt_pt = self
            .path
            .to_str()
            .context("failed to get mount point as str")?
            .as_bytes()
            .to_vec();
        if predictive {
            let num_dirs = if predictive_num_dirs != 0 {
                predictive_num_dirs
                    .try_into()
                    .with_context(|| {
                        anyhow!("could not convert u32 ({}) to i32", predictive_num_dirs)
                    })
                    .ok()
            } else {
                None
            };
            let predictive_params = PredictiveFetch {
                numTopDirectories: num_dirs,
                ..Default::default()
            };
            let glob_params = GlobParams {
                mountPoint: mnt_pt,
                includeDotfiles: false,
                prefetchFiles: !directories_only,
                suppressFileList: silent,
                revisions: commit_vec,
                background,
                predictiveGlob: Some(predictive_params),
                ..Default::default()
            };
            client
                .with_thrift(|thrift| {
                    (
                        thrift.predictiveGlobFiles(&glob_params),
                        EdenThriftMethod::PredictiveGlobFiles,
                    )
                })
                .await
                .with_context(|| "Failed predictiveGlobFiles() thrift call")?;
            Ok(())
        } else {
            let profile_set = all_profile_contents.into_iter().collect::<Vec<_>>();
            let prefetch_params = PrefetchParams {
                mountPoint: mnt_pt.clone(),
                globs: profile_set.clone(),
                directoriesOnly: directories_only,
                revisions: commit_vec.clone(),
                background,
                ..Default::default()
            };
            let res = client
                .with_thrift(|thrift| {
                    (
                        thrift.prefetchFiles(&prefetch_params),
                        EdenThriftMethod::PrefetchFiles,
                    )
                })
                .await;

            match res {
                Ok(_) => Ok(()),
                Err(ConnectAndRequestError::RequestError(
                    PrefetchFilesError::ApplicationException(error),
                )) if error.type_ == ApplicationExceptionErrorCode::UnknownMethod => {
                    let glob_params = GlobParams {
                        mountPoint: mnt_pt,
                        globs: profile_set,
                        includeDotfiles: false,
                        prefetchFiles: !directories_only,
                        suppressFileList: silent,
                        revisions: commit_vec,
                        background,
                        ..Default::default()
                    };
                    client
                        .with_thrift(|thrift| {
                            (thrift.globFiles(&glob_params), EdenThriftMethod::GlobFiles)
                        })
                        .await
                        .with_context(|| "Failed globFiles() thrift call")?;
                    Ok(())
                }
                Err(err) => Err(EdenFsError::Other(err.into())),
            }
        }
    }

    pub fn should_prefetch_profiles(config: &EdenFsConfig) -> bool {
        config.prefetch_profiles.prefetching_enabled.unwrap_or(true)
    }

    pub fn should_prefetch_predictive_profiles(config: &EdenFsConfig) -> bool {
        config
            .prefetch_profiles
            .predictive_prefetching_enabled
            .unwrap_or(true)
    }

    pub async fn prefetch_profiles(
        &self,
        instance: &EdenFsInstance,
        profiles: &[String],
        background: bool,
        directories_only: bool,
        silent: bool,
        revisions: Option<&Vec<String>>,
        predict_revisions: bool,
        predictive: bool,
        predictive_num_dirs: u32,
    ) -> Result<PrefetchProfilesResult> {
        let mut profiles_to_fetch = profiles.to_owned();

        let config = instance
            .get_config()
            .context("unable to load configuration")?;

        if predictive && !EdenFsCheckout::should_prefetch_predictive_profiles(&config) {
            let reason = "Skipping Predictive Prefetch Profiles fetch due to global kill switch. \
                    This means prefetch-profiles.predictive-prefetching-enabled is not set in \
                    the EdenFS configs."
                .to_string();

            return Ok(PrefetchProfilesResult::Skipped(reason));
        }

        if !EdenFsCheckout::should_prefetch_profiles(&config) && !predictive {
            let reason = "Skipping Prefetch Profiles fetch due to global kill switch. \
                    This means prefetch-profiles.prefetching-enabled is not set in \
                    the EdenFS configs."
                .to_string();
            return Ok(PrefetchProfilesResult::Skipped(reason));
        }

        let mut profile_contents = HashSet::new();

        if !predictive {
            // special trees prefetch profile which fetches all of the trees in the repo, kick this
            // off before activating the rest of the prefetch profiles
            let tree_profile = "trees";
            // special trees-mobile prefetch profile which fetches a subset of trees in fbsource, kick this
            // off only if not fetching the overarching trees profile, and before activating the rest of the prefetch profiles
            let tree_mobile_profile = "trees-mobile";

            let mut trees_profile_set = HashSet::new();

            // Check for trees first, if it exists, then kick off the prefetch request.
            if profiles_to_fetch.iter().any(|x| x == tree_profile) {
                profiles_to_fetch.retain(|x| *x != *tree_profile);
                // also remove the trees-mobile profile if it exists, but don't fetch it because it is a subset of trees
                profiles_to_fetch.retain(|x| *x != *tree_mobile_profile);

                trees_profile_set.insert("**/*".to_owned());
            } else if profiles_to_fetch.iter().any(|x| x == tree_mobile_profile) {
                profiles_to_fetch.retain(|x| *x != *tree_mobile_profile);

                trees_profile_set.insert("arvr/**/*".to_owned());
                trees_profile_set.insert("fbandroid/**/*".to_owned());
                trees_profile_set.insert("fbcode/**/*".to_owned());
                trees_profile_set.insert("fbobjc/**/*".to_owned());
                trees_profile_set.insert("third-party/**/*".to_owned());
                trees_profile_set.insert("tools/**/*".to_owned());
                trees_profile_set.insert("xplat/**/*".to_owned());
                trees_profile_set.insert("whatsapp/**/*".to_owned());
            }

            if !trees_profile_set.is_empty() {
                self.make_prefetch_request(
                    instance,
                    trees_profile_set,
                    true, // only prefetch directories
                    silent,
                    revisions.clone(),
                    predict_revisions,
                    background,
                    predictive,
                    predictive_num_dirs,
                )
                .await
                .with_context(|| anyhow!("make_prefetch_request() failed, returning early"))?;
                if profiles_to_fetch.is_empty() {
                    return Ok(PrefetchProfilesResult::Prefetched);
                }
            }

            for profile in profiles_to_fetch {
                let res = self
                    .get_contents_for_profile(&profile, silent)
                    .with_context(|| {
                        anyhow!("failed to get contents of prefetch profile {}", &profile)
                    })?;
                profile_contents.extend(res);
            }
        }
        self.make_prefetch_request(
            instance,
            profile_contents,
            directories_only,
            silent,
            revisions,
            predict_revisions,
            background,
            predictive,
            predictive_num_dirs,
        )
        .await
        .with_context(|| anyhow!("make_prefetch_request() failed, returning early"))?;
        Ok(PrefetchProfilesResult::Prefetched)
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

/// Return information about all checkouts defined in EdenFS's configuration files
/// and all information about mounted checkouts from the eden daemon
pub async fn get_mounts(instance: &EdenFsInstance) -> Result<BTreeMap<PathBuf, EdenFsCheckout>> {
    // Get all configured checkout info (including not mounted / not active ones) from configs
    let mut configs: Vec<(PathBuf, PathBuf, CheckoutConfig)> = Vec::new();
    for (mount_path, client_name) in instance.get_configured_mounts_map()? {
        configs.push((
            mount_path,
            instance.config_directory(&client_name),
            CheckoutConfig::parse_config(instance.config_directory(&client_name))?,
        ));
    }

    // Get active mounted checkouts info from eden daemon
    let client = instance.get_client();
    let mounted_checkouts = match client
        .with_thrift_with_timeouts(Some(Duration::from_secs(3)), None, |thrift| {
            (thrift.listMounts(), EdenThriftMethod::ListMounts)
        })
        .await
    {
        Ok(result) => Some(result),
        Err(_) => None, // eden daemon is not running or not healthy
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

#[cfg(windows)]
#[derive(Deserialize)]
struct WindowsEdenConfigInner {
    root: PathBuf,
    client: PathBuf,
}

#[cfg(windows)]
#[derive(Deserialize)]
struct WindowsEdenConfig {
    #[serde(rename = "Config")]
    config: WindowsEdenConfigInner,
}

#[cfg(windows)]
fn get_checkout_root_state(path: &Path) -> Result<(Option<PathBuf>, Option<PathBuf>)> {
    let mut checkout_root = None;
    let mut checkout_state_dir = None;

    // On Windows, walk backwards through the path until you find the `.eden` folder
    let mut curr_dir = Some(path);
    while let Some(candidate_dir) = curr_dir {
        if candidate_dir.join(".eden").exists() {
            let config_file = candidate_dir.join(".eden").join("config");
            let config = std::fs::read_to_string(config_file).from_err()?;
            let config = toml::from_str::<WindowsEdenConfig>(&config).from_err()?;
            checkout_root = Some(config.config.root);
            checkout_state_dir = Some(config.config.client);
            break;
        } else {
            curr_dir = candidate_dir.parent();
        }
    }
    Ok((checkout_root, checkout_state_dir))
}

#[cfg(not(windows))]
fn get_checkout_root_state(path: &Path) -> Result<(Option<PathBuf>, Option<PathBuf>)> {
    // We will get an error if any of these symlinks do not exist
    let eden_socket_path = fs::read_link(path.join(".eden").join("socket"));
    if eden_socket_path.is_ok() {
        let checkout_root = fs::read_link(path.join(".eden").join("root")).ok();
        let checkout_state_dir = fs::read_link(path.join(".eden").join("client")).ok();
        Ok((checkout_root, checkout_state_dir))
    } else {
        Ok((None, None))
    }
}

/// If the path provided is an eden checkout, this returns an object representing that checkout.
/// Otherwise, if the path provided is not an eden checkout, this returns an EdenFsError.
pub fn find_checkout(instance: &EdenFsInstance, path: &Path) -> Result<EdenFsCheckout> {
    // Resolve symlinks and get absolute path
    let path = path.canonicalize().from_err()?;
    #[cfg(windows)]
    let path = strip_unc_prefix(path);

    // Check if it is a mounted checkout
    let (checkout_root, checkout_state_dir) = get_checkout_root_state(&path)?;

    if checkout_root.is_none() {
        // Find `checkout_path` that `path` is a sub path of
        let all_checkouts = instance.get_configured_mounts_map()?;
        if let Some(item) = all_checkouts
            .iter()
            .find(|&(checkout_path, _)| path.starts_with(checkout_path))
        {
            let (checkout_path, checkout_name) = item;
            let checkout_state_dir = instance.config_directory(checkout_name);
            Ok(EdenFsCheckout::from_config(
                PathBuf::from(checkout_path),
                checkout_state_dir.clone(),
                CheckoutConfig::parse_config(checkout_state_dir)?,
            ))
        } else {
            Err(EdenFsError::Other(anyhow!(
                "Checkout path {} is not handled by EdenFS",
                path.display()
            )))
        }
    } else if checkout_state_dir.is_none() {
        let all_checkouts = instance.get_configured_mounts_map()?;
        let checkout_path = checkout_root.unwrap();
        if let Some(checkout_name) = all_checkouts.get(&checkout_path) {
            let checkout_state_dir = instance.config_directory(checkout_name);
            Ok(EdenFsCheckout::from_config(
                checkout_path,
                checkout_state_dir.clone(),
                CheckoutConfig::parse_config(checkout_state_dir)?,
            ))
        } else {
            Err(EdenFsError::Other(anyhow!(
                "unknown checkout {}",
                checkout_path.display()
            )))
        }
    } else {
        Ok(EdenFsCheckout::from_config(
            checkout_root.unwrap(),
            checkout_state_dir.as_ref().unwrap().clone(),
            CheckoutConfig::parse_config(checkout_state_dir.unwrap())?,
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs::File;
    use std::io::Write;
    use std::path::Path;
    use std::path::PathBuf;

    use anyhow::Context;
    use anyhow::anyhow;
    use edenfs_error::EdenFsError;
    use edenfs_error::Result;
    use edenfs_error::ResultExt;
    use tempfile::TempDir;
    use tempfile::tempdir;

    use crate::checkout::CheckoutConfig;
    use crate::checkout::MOUNT_CONFIG;
    use crate::checkout::REPO_SOURCE;
    use crate::checkout::RedirectionType;
    use crate::redirect::Redirection;
    use crate::redirect::RedirectionState;

    // path and type are required... /tmp/ is probably the safest place to use as the path
    const DEFAULT_CHECKOUT_CONFIG: &str = r#"
    [repository]
    path = "/tmp/"
    type = "hg"

    [redirections]

    [profiles]

    [predictive-prefetch]"#;

    /// creates a checkout config and returns the tempdir in which the config is located
    fn create_test_checkout_config() -> Result<TempDir> {
        let temp_dir = tempdir().context("couldn't create temp dir")?;
        let mut config_file = File::create(temp_dir.path().join(MOUNT_CONFIG)).from_err()?;
        config_file
            .write_all(DEFAULT_CHECKOUT_CONFIG.as_bytes())
            .from_err()?;
        Ok(temp_dir)
    }

    /// Takes a (repo_path, redir_type) pair and checks if a checkout config contains that redirection
    fn checkout_config_contains_redirection(
        repo_path: &Path,
        redir_type: RedirectionType,
        config_dir: &Path,
    ) -> Result<()> {
        let config = CheckoutConfig::parse_config(config_dir.into())?;
        let config_redir_type = config.redirections.get(repo_path);
        match config_redir_type {
            Some(r_type) => {
                if *r_type != redir_type {
                    Err(EdenFsError::Other(anyhow!(
                        "Redirection type did not match the redirection type in the config"
                    )))
                } else {
                    Ok(())
                }
            }
            None => Err(EdenFsError::Other(anyhow!(
                "Did not find redirection in checkout config"
            ))),
        }
    }

    fn update_and_test_redirection(
        redir: Redirection,
        config_dir: &Path,
        should_be_inserted: bool,
    ) -> Result<()> {
        // parse config from test_path
        let mut config = CheckoutConfig::parse_config(config_dir.into())?;

        // create map of redirections we want to add to the checkout config
        let mut redir_map: BTreeMap<PathBuf, Redirection> = BTreeMap::new();

        // We need to track repo_path and redir type to confirm the redirection was added correctly
        let repo_path = redir.repo_path();
        let redir_type = redir.redir_type;

        // insert the redirection and update the checkout config
        redir_map.insert(repo_path.clone(), redir);
        config
            .update_redirections(config_dir, &redir_map)
            .expect("failed to update checkout config");

        // check if redirection was inserted (or not inserted) correctly
        if should_be_inserted {
            checkout_config_contains_redirection(&repo_path, redir_type, config_dir)
        } else {
            // In some cases, we don't want the checkout to be inserted (when source == REPO_SOURCE)
            // In that case, we should fail if we find the redirection in the checkout config
            match checkout_config_contains_redirection(&repo_path, redir_type, config_dir) {
                Ok(_) => Err(EdenFsError::Other(anyhow!(
                    "Redirection was present in config when it shouldn't have been present"
                ))),
                Err(_) => Ok(()),
            }
        }
    }

    #[test]
    fn test_update_redirections() {
        let config_test_dir =
            create_test_checkout_config().expect("failed to create test checkout config");
        let config_dir = config_test_dir.path();

        // test inserting a sylink redirection (this should be inserted because source != REPO_SOURCE)
        let redir1 = Redirection {
            repo_path: PathBuf::from("test_path"),
            redir_type: RedirectionType::Symlink,
            target: None,
            source: "NotARepoSource".into(),
            state: RedirectionState::UnknownMount,
        };
        assert!(update_and_test_redirection(redir1, config_dir, true).is_ok());

        // test inserting a symlink redirection whose source == REPO_SOURCE
        let redir2 = Redirection {
            repo_path: PathBuf::from("test_path2"),
            redir_type: RedirectionType::Symlink,
            target: None,
            source: REPO_SOURCE.into(),
            state: RedirectionState::UnknownMount,
        };
        assert!(update_and_test_redirection(redir2, config_dir, false).is_ok());

        // test inserting a bind redirection whose source != REPO_SOURCE
        let redir3 = Redirection {
            repo_path: PathBuf::from("test_path3"),
            redir_type: RedirectionType::Bind,
            target: None,
            source: "NotARepoSource".into(),
            state: RedirectionState::UnknownMount,
        };
        assert!(update_and_test_redirection(redir3, config_dir, true).is_ok());

        // test inserting a bind redirection whose source == REPO_SOURCE
        let redir4 = Redirection {
            repo_path: PathBuf::from("test_path4"),
            redir_type: RedirectionType::Bind,
            target: None,
            source: REPO_SOURCE.into(),
            state: RedirectionState::UnknownMount,
        };
        assert!(update_and_test_redirection(redir4, config_dir, false).is_ok());
    }
}
