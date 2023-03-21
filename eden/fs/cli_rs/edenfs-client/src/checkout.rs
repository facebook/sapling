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
use std::fs;
use std::fs::File;
use std::io::prelude::*;
use std::io::BufReader;
use std::io::Read;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;
use std::vec;

use anyhow::anyhow;
use anyhow::Context;
use atomicfile::atomic_write;
use byteorder::BigEndian;
use byteorder::ReadBytesExt;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use edenfs_utils::path_from_bytes;
#[cfg(windows)]
use edenfs_utils::strip_unc_prefix;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use thrift_types::edenfs::errors::eden_service::PrefetchFilesError;
use thrift_types::edenfs::types::Glob;
use thrift_types::edenfs::types::GlobParams;
use thrift_types::edenfs::types::MountInfo;
use thrift_types::edenfs::types::MountState;
use thrift_types::edenfs::types::PredictiveFetch;
use thrift_types::edenfs::types::PrefetchParams;
use thrift_types::fbthrift::ApplicationExceptionErrorCode;
use toml::value::Value;
use uuid::Uuid;

use crate::redirect::deserialize_redirections;
use crate::redirect::Redirection;
use crate::redirect::RedirectionType;
use crate::redirect::REPO_SOURCE;
use crate::EdenFsInstance;

// files in the client directory (aka data_dir aka state_dir)
const MOUNT_CONFIG: &str = "config.toml";
const SNAPSHOT: &str = "SNAPSHOT";

// Magical snapshot strings
const SNAPSHOT_MAGIC_1: &[u8] = b"eden\x00\x00\x00\x01";
const SNAPSHOT_MAGIC_2: &[u8] = b"eden\x00\x00\x00\x02";
const SNAPSHOT_MAGIC_3: &[u8] = b"eden\x00\x00\x00\x03";
const SNAPSHOT_MAGIC_4: &[u8] = b"eden\x00\x00\x00\x04";

const SUPPORTED_REPOS: &[&str] = &["git", "hg", "recas"];
const SUPPORTED_MOUNT_PROTOCOLS: &[&str] = &["fuse", "nfs", "prjfs"];

#[derive(Deserialize, Serialize, Debug)]
struct Repository {
    path: PathBuf,

    #[serde(rename = "type", deserialize_with = "deserialize_repo_type")]
    repo_type: String,

    #[serde(default = "default_guid")]
    guid: Uuid,

    #[serde(
        deserialize_with = "deserialize_protocol",
        default = "default_protocol"
    )]
    protocol: String,

    #[serde(rename = "case-sensitive", default = "default_case_sensitive")]
    case_sensitive: bool,

    #[serde(rename = "require-utf8-path", default = "default_require_utf8_path")]
    require_utf8_path: bool,

    #[serde(rename = "enable-sqlite-overlay", default)]
    enable_sqlite_overlay: bool,

    #[serde(rename = "use-write-back-cache", default)]
    use_write_back_cache: bool,
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

    #[serde(deserialize_with = "deserialize_redirections")]
    redirections: BTreeMap<PathBuf, RedirectionType>,

    profiles: Option<PrefetchProfiles>,

    #[serde(rename = "predictive-prefetch", default)]
    predictive_prefetch: Option<PredictivePrefetch>,
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

    pub fn print_prefetch_profiles(&self) {
        if let Some(profiles) = &self.profiles {
            for s in profiles.active.iter() {
                println!("{}", s);
            }
        }
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
        for (_, redir) in redirs.iter() {
            if redir.source != REPO_SOURCE {
                self.redirections
                    .insert(redir.repo_path(), redir.redir_type);
            }
        }
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
    /// with profile added). Returns true if we should fetch, false otherwise.
    pub fn activate_profile(
        &mut self,
        profile: &str,
        config_dir: PathBuf,
        force_fetch: &bool,
    ) -> Result<bool> {
        if let Some(profiles) = &mut self.profiles {
            if profiles.active.iter().any(|x| x == profile) {
                // The profile is already activated so we don't need to update the profile list,
                // but we want to return a success so we continue with the fetch
                if *force_fetch {
                    return Ok(true);
                }
                eprintln!("{} is already an active prefetch profile", profile);
                return Ok(false);
            }
            profiles.push(profile);
            self.save_config(config_dir.clone()).with_context(|| {
                anyhow!(
                    "failed to save config in the given config_dir: {}",
                    &config_dir.display()
                )
            })?;
        }
        Ok(true)
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
}

impl SnapshotState {
    fn new(working_copy_parent: String, last_checkout_hash: String) -> Self {
        Self {
            working_copy_parent,
            last_checkout_hash,
        }
    }
}

fn is_unknown_method_error(error: &PrefetchFilesError) -> bool {
    if let PrefetchFilesError::ApplicationException(ref e) = error {
        e.type_ == ApplicationExceptionErrorCode::UnknownMethod
    } else {
        false
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

    /// Returns a SnapshotState representing EdenFS working copy parent as well as the last checked
    /// out revision.
    pub fn get_snapshot(&self) -> Result<SnapshotState> {
        let snapshot_path = self.data_dir.join(SNAPSHOT);
        let mut f = File::open(&snapshot_path).from_err()?;
        let mut header = [0u8; 8];
        f.read(&mut header).from_err()?;
        if header == SNAPSHOT_MAGIC_1 {
            let mut snapshot = [0u8; 20];
            f.read(&mut snapshot).from_err()?;
            let decoded = EdenFsCheckout::encode_hex(&snapshot);
            Ok(SnapshotState::new(decoded.clone(), decoded))
        } else if header == SNAPSHOT_MAGIC_2 {
            let body_length = f.read_u32::<BigEndian>().from_err()?;
            let mut buf = vec![0u8; body_length as usize];
            f.read_exact(&mut buf).from_err()?;
            let decoded = std::str::from_utf8(&buf).from_err()?.to_string();
            Ok(SnapshotState::new(decoded.clone(), decoded))
        } else if header == SNAPSHOT_MAGIC_3 {
            let _pid = f.read_u32::<BigEndian>().from_err()?;

            let from_length = f.read_u32::<BigEndian>().from_err()?;
            let mut from_buf = vec![0u8; from_length as usize];
            f.read_exact(&mut from_buf).from_err()?;

            let to_length = f.read_u32::<BigEndian>().from_err()?;
            let mut to_buf = vec![0u8; to_length as usize];
            f.read_exact(&mut to_buf).from_err()?;

            // TODO(xavierd): return a proper object that the caller could use.
            Err(EdenFsError::Other(anyhow!(
                "A checkout operation is ongoing from {} to {}",
                std::str::from_utf8(&from_buf).from_err()?,
                std::str::from_utf8(&to_buf).from_err()?
            )))
        } else if header == SNAPSHOT_MAGIC_4 {
            let working_copy_parent_length = f.read_u32::<BigEndian>().from_err()?;
            let mut working_copy_parent_buf = vec![0u8; working_copy_parent_length as usize];
            f.read_exact(&mut working_copy_parent_buf).from_err()?;

            let checked_out_length = f.read_u32::<BigEndian>().from_err()?;
            let mut checked_out_buf = vec![0u8; checked_out_length as usize];
            f.read_exact(&mut checked_out_buf).from_err()?;

            Ok(SnapshotState::new(
                std::str::from_utf8(&working_copy_parent_buf)
                    .from_err()?
                    .to_string(),
                std::str::from_utf8(&checked_out_buf)
                    .from_err()?
                    .to_string(),
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
                    "Cannot read conents for prefetch profile '{}'",
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
    ) -> Result<Glob> {
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

        let client = instance.connect(None).await?;
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
            let res = client.predictiveGlobFiles(&glob_params).await;
            Ok(res.context("Failed predictiveGlobFiles() thrift call")?)
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
            let res = client.prefetchFiles(&prefetch_params).await;

            match res {
                Ok(_) => Ok(Glob::default()),
                Err(error) => {
                    if is_unknown_method_error(&error) {
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
                        let glob_res = client.globFiles(&glob_params).await;
                        Ok(glob_res.context("Failed globFiles() thrift call")?)
                    } else {
                        Err(EdenFsError::Other(error.into()))
                    }
                }
            }
        }
    }

    pub async fn prefetch_profiles(
        &self,
        instance: &EdenFsInstance,
        profiles: &Vec<String>,
        background: bool,
        directories_only: bool,
        silent: bool,
        revisions: Option<&Vec<String>>,
        predict_revisions: bool,
        predictive: bool,
        predictive_num_dirs: u32,
    ) -> Result<Vec<Glob>> {
        let mut profiles_to_fetch = profiles.clone();

        let config = instance
            .get_config()
            .context("unable to load configuration")?;
        if predictive && !config.prefetch_profiles.predictive_prefetching_enabled {
            if !silent {
                eprintln!(
                    "Skipping Predictive Prefetch Profiles fetch due to global kill switch. \
                    This means prefetch-profiles.predictive-prefetching-enabled is not set in \
                    the EdenFS configs.",
                );
            } else {
                return Ok(vec![Glob::default()]);
            }
        }

        if !config.prefetch_profiles.prefetching_enabled && !predictive {
            if !silent {
                eprintln!(
                    "Skipping Prefetch Profiles fetch due to global kill switch. \
                    This means prefetch-profiles.prefetching-enabled is not set in \
                    the EdenFS configs."
                );
            }
            return Ok(vec![Glob::default()]);
        }

        let mut profile_contents = HashSet::new();
        let mut glob_results = vec![];

        if !predictive {
            // special trees prefetch profile which fetches all of the trees in the repo, kick this
            // off before activating the rest of the prefetch profiles
            let tree_profile = "trees";
            if profiles_to_fetch.iter().any(|x| x == tree_profile) {
                profiles_to_fetch.retain(|x| *x != *tree_profile);
                let mut profile_set = HashSet::new();
                profile_set.insert("**/*".to_owned());

                let blob_res = self
                    .make_prefetch_request(
                        instance,
                        profile_set,
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
                glob_results.push(blob_res);
                if profiles_to_fetch.is_empty() {
                    return Ok(glob_results);
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
        let blob_res = self
            .make_prefetch_request(
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
        glob_results.push(blob_res);
        Ok(glob_results)
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

#[cfg(windows)]
#[derive(Deserialize)]
struct WindowsEdenConfigInner {
    socket: PathBuf,
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
    let mut curr_dir = Some(path.clone());
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

    use anyhow::anyhow;
    use anyhow::Context;
    use edenfs_error::EdenFsError;
    use edenfs_error::Result;
    use edenfs_error::ResultExt;
    use tempfile::tempdir;
    use tempfile::TempDir;

    use crate::checkout::CheckoutConfig;
    use crate::checkout::RedirectionType;
    use crate::checkout::MOUNT_CONFIG;
    use crate::checkout::REPO_SOURCE;
    use crate::redirect::Redirection;

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
            state: None,
        };
        assert!(update_and_test_redirection(redir1, config_dir, true).is_ok());

        // test inserting a symlink redirection whose source == REPO_SOURCE
        let redir2 = Redirection {
            repo_path: PathBuf::from("test_path2"),
            redir_type: RedirectionType::Symlink,
            target: None,
            source: REPO_SOURCE.into(),
            state: None,
        };
        assert!(update_and_test_redirection(redir2, config_dir, false).is_ok());

        // test inserting a bind redirection whose source != REPO_SOURCE
        let redir3 = Redirection {
            repo_path: PathBuf::from("test_path3"),
            redir_type: RedirectionType::Bind,
            target: None,
            source: "NotARepoSource".into(),
            state: None,
        };
        assert!(update_and_test_redirection(redir3, config_dir, true).is_ok());

        // test inserting a bind redirection whose source == REPO_SOURCE
        let redir4 = Redirection {
            repo_path: PathBuf::from("test_path4"),
            redir_type: RedirectionType::Bind,
            target: None,
            source: REPO_SOURCE.into(),
            state: None,
        };
        assert!(update_and_test_redirection(redir4, config_dir, false).is_ok());
    }
}
