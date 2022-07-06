/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use edenfs_utils::metadata::MetadataExt;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;
use subprocess::Exec;
use subprocess::Redirection as SubprocessRedirection;
use toml::value::Value;

use crate::checkout::EdenFsCheckout;
use crate::mounttable::read_mount_table;

const REPO_SOURCE: &str = ".eden-redirections";
const USER_REDIRECTION_SOURCE: &str = ".eden/client/config.toml:redirections";
const APFS_HELPER: &str = "/usr/local/libexec/eden/eden_apfs_mount_helper";

#[derive(Clone, Serialize, Copy, Debug, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RedirectionType {
    /// Linux: a bind mount to a mkscratch generated path
    /// macOS: a mounted dmg file in a mkscratch generated path
    /// Windows: equivalent to symlink type
    Bind,
    /// A symlink to a mkscratch generated path
    Symlink,
    Unknown,
}

impl FromStr for RedirectionType {
    type Err = EdenFsError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "bind" {
            Ok(RedirectionType::Bind)
        } else if s == "symlink" {
            Ok(RedirectionType::Symlink)
        } else {
            // deliberately did not implement "Unknown"
            Err(EdenFsError::ConfigurationError(format!(
                "Unknown redirection type: {}. Must be one of: bind, symlink",
                s
            )))
        }
    }
}

#[derive(Debug)]
enum RedirectionState {
    #[allow(dead_code)]
    /// Matches the expectations of our configuration as far as we can tell
    MatchesConfiguration,
    /// Something Mounted that we don't have configuration for
    UnknownMount,
    /// We Expected It To be mounted, but it isn't
    NotMounted,
    /// We Expected It To be a symlink, but it is not present
    SymlinkMissing,
    /// The Symlink Is Present but points to the wrong place
    SymlinkIncorrect,
}

impl fmt::Display for RedirectionState {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match *self {
                Self::MatchesConfiguration => "ok",
                Self::UnknownMount => "unknown-mount",
                Self::NotMounted => "not-mounted",
                Self::SymlinkMissing => "symlink-missing",
                Self::SymlinkIncorrect => "symlink-incorrect",
            }
        )
    }
}

#[derive(Debug)]
pub struct Redirection {
    repo_path: PathBuf,
    redir_type: RedirectionType,
    #[allow(dead_code)]
    target: Option<PathBuf>,
    #[allow(dead_code)]
    source: String,
    state: Option<RedirectionState>,
}

impl Redirection {
    pub fn repo_path(&self) -> PathBuf {
        self.repo_path.clone()
    }

    /// Determine if the APFS volume helper is installed with appropriate
    /// permissions such that we can use it to mount things
    fn have_apfs_helper() -> Result<bool> {
        match fs::symlink_metadata(APFS_HELPER) {
            Ok(metadata) => Ok(metadata.is_setuid_set()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(e) => Err(e),
        }
        .from_err()
    }

    fn mkscratch_bin() -> PathBuf {
        // mkscratch is provided by the hg deployment at facebook, which has a
        // different installation prefix on macOS vs Linux, so we need to resolve
        // it via the PATH.  In the integration test environment we'll set the
        // MKSCRATCH_BIN to point to the binary under test
        match std::env::var("MKSCRATCH_BIN") {
            Ok(s) => PathBuf::from(s),
            Err(_) => PathBuf::from("mkscratch"),
        }
    }

    fn make_scratch_dir(checkout: &EdenFsCheckout, subdir: &Path) -> Result<PathBuf> {
        let mkscratch = Redirection::mkscratch_bin();
        let checkout_path_str = checkout.path().to_string_lossy().into_owned();
        let subdir = PathBuf::from("edenfs/redirections")
            .join(subdir)
            .to_string_lossy()
            .into_owned();
        let args = &["path", &checkout_path_str, "--subdir", &subdir];
        let output = Exec::cmd(&mkscratch)
            .args(args)
            .stdout(SubprocessRedirection::Pipe)
            .stderr(SubprocessRedirection::Pipe)
            .capture()
            .from_err()?;
        if output.success() {
            Ok(PathBuf::from(output.stdout_str().trim()))
        } else {
            Err(EdenFsError::Other(anyhow!(
                "Failed to execute `{} {}`, stderr: {}, exit status: {:?}",
                &mkscratch.display(),
                args.join(" "),
                output.stderr_str(),
                output.exit_status,
            )))
        }
    }

    pub fn expand_target_abspath(&self, checkout: &EdenFsCheckout) -> Result<Option<PathBuf>> {
        match self.redir_type {
            RedirectionType::Bind => {
                if Redirection::have_apfs_helper()? {
                    // Ideally we'd return information about the backing, but
                    // it is a bit awkward to determine this in all contexts;
                    // prior to creating the volume we don't know anything
                    // about where it will reside.
                    // After creating it, we could potentially parse the APFS
                    // volume information and show something like the backing device.
                    // We also have a transitional case where there is a small
                    // population of users on disk image mounts; we actually don't
                    // have enough knowledge in this code to distinguish between
                    // a disk image and an APFS volume (but we can tell whether
                    // either of those is mounted elsewhere in this file, provided
                    // we have a MountTable to inspect).
                    // Given our small user base at the moment, it doesn't seem
                    // super critical to have this tool handle all these cases;
                    // the same information can be extracted by a human running
                    // `mount` and `diskutil list`.
                    // So we just return the mount point path when we believe
                    // that we can use APFS.
                    Ok(Some(checkout.path().join(&self.repo_path)))
                } else {
                    Ok(Some(Redirection::make_scratch_dir(
                        checkout,
                        &self.repo_path,
                    )?))
                }
            }
            RedirectionType::Symlink => Ok(Some(Redirection::make_scratch_dir(
                checkout,
                &self.repo_path,
            )?)),
            RedirectionType::Unknown => Ok(None),
        }
    }
}

/// Detect the most common form of a bind mount in the repo;
/// its parent directory will have a different device number than
/// the mount point itself.  This won't detect something funky like
/// bind mounting part of the repo to a different part.
fn is_bind_mount(path: PathBuf) -> Result<bool> {
    let parent = path.parent();
    if let Some(parent_path) = parent {
        let path_metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
        .from_err()?;
        let parent_metadata = match fs::symlink_metadata(parent_path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
        .from_err()?;

        match (path_metadata, parent_metadata) {
            (Some(m1), Some(m2)) => Ok(m1.eden_dev() != m2.eden_dev()),
            _ => Ok(false),
        }
    } else {
        Ok(false)
    }
}

#[derive(Deserialize)]
struct RedirectionsConfigInner {
    #[serde(flatten, deserialize_with = "deserialize_redirections")]
    redirections: BTreeMap<PathBuf, RedirectionType>,
}

#[derive(Deserialize)]
struct RedirectionsConfig {
    #[serde(rename = "redirections")]
    inner: RedirectionsConfigInner,
}

pub(crate) fn deserialize_redirections<'de, D>(
    deserializer: D,
) -> Result<BTreeMap<PathBuf, RedirectionType>, D::Error>
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
                        key.replace("/", "\\")
                    } else {
                        key
                    },
                ),
                RedirectionType::from_str(s).map_err(serde::de::Error::custom)?,
            );
        } else {
            return Err(serde::de::Error::custom(format!(
                "Unsupported redirection value type {}. Must be string.",
                value
            )));
        }
    }

    Ok(map)
}

/// Returns the explicitly configured redirection configuration.
/// This does not take into account how things are currently mounted;
/// use `get_effective_redirections` for that purpose.
fn get_configured_redirections(
    checkout: &EdenFsCheckout,
) -> Result<BTreeMap<PathBuf, Redirection>> {
    let mut redirs = BTreeMap::new();

    // Repo-specified settings have the lowest level of precedence
    let repo_redirection_config_file_name = checkout.path().join(".eden-redirections");
    if let Ok(contents) = std::fs::read(repo_redirection_config_file_name) {
        let s = String::from_utf8(contents).from_err()?;
        let config: RedirectionsConfig = toml::from_str(&s).from_err()?;
        for (repo_path, redir_type) in config.inner.redirections {
            redirs.insert(
                repo_path.clone(),
                Redirection {
                    repo_path,
                    redir_type,
                    target: None,
                    source: REPO_SOURCE.to_string(),
                    state: None,
                },
            );
        }
    }

    // User-specific things have the highest precedence
    if let Some(user_redirs) = &checkout.redirections {
        for (repo_path, redir_type) in user_redirs {
            redirs.insert(
                repo_path.clone(),
                Redirection {
                    repo_path: repo_path.clone(),
                    redir_type: *redir_type,
                    target: None,
                    source: USER_REDIRECTION_SOURCE.to_string(),
                    state: None,
                },
            );
        }
    }

    Ok(redirs)
}

fn is_symlink_correct(redir: &Redirection, checkout: &EdenFsCheckout) -> Result<bool> {
    if let Some(expected_target) = redir.expand_target_abspath(checkout)? {
        let expected_target = fs::canonicalize(expected_target).from_err()?;
        let symlink_path = checkout.path().join(redir.repo_path.clone());
        let target = fs::canonicalize(fs::read_link(symlink_path).from_err()?).from_err()?;
        Ok(target == expected_target)
    } else {
        Ok(false)
    }
}

/// Computes the complete set of redirections that are currently in effect.
/// This is based on the explicitly configured settings but also factors in
/// effective configuration by reading the mount table.
pub fn get_effective_redirections(
    checkout: &EdenFsCheckout,
) -> Result<BTreeMap<PathBuf, Redirection>> {
    let mut redirs = BTreeMap::new();
    let path_prefix = checkout.path();
    for mount_info in read_mount_table()? {
        let mount_point = mount_info.mount_point();
        if let Ok(rel_path) = mount_point.strip_prefix(&path_prefix) {
            // The is_bind_mount test may appear to be redundant but it is
            // possible for mounts to layer such that we have:
            //
            // /my/repo    <-- fuse at the top of the vfs
            // /my/repo/buck-out
            // /my/repo    <-- earlier generation fuse at bottom
            //
            // The buck-out bind mount in the middle is visible in the
            // mount table but is not visible via the VFS because there
            // is a different /my/repo mounted over the top.
            //
            // We test whether we can see a mount point at that location
            // before recording it in the effective redirection list so
            // that we don't falsely believe that the bind mount is up.
            if path_prefix != mount_point && is_bind_mount(mount_info.mount_point())? {
                redirs.insert(
                    rel_path.to_path_buf(),
                    Redirection {
                        repo_path: rel_path.to_path_buf(),
                        redir_type: RedirectionType::Unknown,
                        target: None,
                        source: "mount".to_string(),
                        state: Some(RedirectionState::UnknownMount),
                    },
                );
            }
        }
    }

    for (rel_path, mut redir) in get_configured_redirections(checkout)? {
        let is_in_mount_table = redirs.contains_key(&rel_path);
        if is_in_mount_table {
            // The configured redirection entries take precedence over the mount table entries.
            // We overwrite them in the `redirs` map.
            if redir.redir_type != RedirectionType::Bind {
                redir.state = Some(RedirectionState::UnknownMount);
            }
            // else: we expected them to be in the mount table and they were.
            // we don't know enough to tell whether the mount points where
            // we want it to point, so we just assume that it is in the right
            // state.
        } else if redir.redir_type == RedirectionType::Bind && !cfg!(windows) {
            // We expected both of these types to be visible in the
            // mount table, but they were not, so we consider them to
            // be in the NOT_MOUNTED state.
            redir.state = Some(RedirectionState::NotMounted);
        } else if redir.redir_type == RedirectionType::Symlink || cfg!(windows) {
            if let Ok(is_correct) = is_symlink_correct(&redir, checkout) {
                if !is_correct {
                    redir.state = Some(RedirectionState::SymlinkIncorrect);
                }
            } else {
                // We're considering a variety of errors that might
                // manifest around trying to read the symlink as meaning
                // that the symlink is effectively missing, even if it
                // isn't literally missing.  eg: EPERM means we can't
                // resolve it, so it is effectively no good.
                redir.state = Some(RedirectionState::SymlinkMissing);
            }
        }
        redirs.insert(rel_path, redir);
    }

    Ok(redirs)
}
