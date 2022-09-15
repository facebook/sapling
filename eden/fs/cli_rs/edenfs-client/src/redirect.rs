/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::path::Path;
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;
#[cfg(target_os = "macos")]
use std::process::Stdio;
use std::str::FromStr;

use anyhow::anyhow;
#[cfg(target_os = "linux")]
use anyhow::Context;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
use edenfs_utils::metadata::MetadataExt;
#[cfg(target_os = "windows")]
use edenfs_utils::remove_symlink;
#[cfg(target_os = "windows")]
use mkscratch::zzencode;
#[cfg(target_os = "macos")]
use nix::sys::stat::stat;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use subprocess::Exec;
use subprocess::Redirection as SubprocessRedirection;
use toml::value::Value;

use crate::checkout::EdenFsCheckout;
use crate::instance::EdenFsInstance;
use crate::mounttable::read_mount_table;

pub(crate) const REPO_SOURCE: &str = ".eden-redirections";
const USER_REDIRECTION_SOURCE: &str = ".eden/client/config.toml:redirections";
const APFS_HELPER: &str = "/usr/local/libexec/eden/eden_apfs_mount_helper";

#[derive(Clone, Serialize, Copy, Debug, PartialEq, PartialOrd)]
#[serde(rename_all = "lowercase")]
pub enum RedirectionType {
    /// Linux: a bind mount to a mkscratch generated path
    /// macOS: a mounted dmg file in a mkscratch generated path
    /// Windows: equivalent to symlink type
    Bind,
    /// A symlink to a mkscratch generated path
    Symlink,
    Unknown,
}

impl fmt::Display for RedirectionType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match *self {
                RedirectionType::Bind => "bind",
                RedirectionType::Symlink => "symlink",
                RedirectionType::Unknown => "unknown",
            }
        )
    }
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

#[derive(Debug, Serialize)]
pub enum RedirectionState {
    #[serde(rename = "ok")]
    /// Matches the expectations of our configuration as far as we can tell
    MatchesConfiguration,
    #[serde(rename = "unknown-mount")]
    /// Something Mounted that we don't have configuration for
    UnknownMount,
    #[serde(rename = "not-mounted")]
    /// We Expected It To be mounted, but it isn't
    NotMounted,
    #[serde(rename = "symlink-missing")]
    /// We Expected It To be a symlink, but it is not present
    SymlinkMissing,
    #[serde(rename = "symlink-incorrect")]
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

#[derive(Debug, Serialize)]
pub struct Redirection {
    pub repo_path: PathBuf,
    #[serde(rename = "type")]
    pub redir_type: RedirectionType,
    pub source: String,
    pub state: Option<RedirectionState>,
    /// This field is lazily calculated and it is only populated after
    /// [`Redirection::update_target_abspath`] is called.
    pub target: Option<PathBuf>,
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

    pub fn mkscratch_bin() -> PathBuf {
        // mkscratch is provided by the hg deployment at facebook, which has a
        // different installation prefix on macOS vs Linux, so we need to resolve
        // it via the PATH.  In the integration test environment we'll set the
        // MKSCRATCH_BIN to point to the binary under test
        match std::env::var("MKSCRATCH_BIN") {
            Ok(s) => PathBuf::from(s),
            Err(_) => PathBuf::from("mkscratch"),
        }
    }

    pub fn scratch_subdir() -> PathBuf {
        PathBuf::from("edenfs/redirections")
    }

    fn make_scratch_dir(checkout: &EdenFsCheckout, subdir: &Path) -> Result<PathBuf> {
        // TODO(zeyi): we can probably embed the logic from mkscratch here directly, without asking the CLI
        let mkscratch = Redirection::mkscratch_bin();
        let checkout_path_str = checkout.path().to_string_lossy().into_owned();
        let subdir = Redirection::scratch_subdir()
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

    pub fn update_target_abspath(&mut self, checkout: &EdenFsCheckout) -> Result<()> {
        self.target = self.expand_target_abspath(checkout)?;
        Ok(())
    }

    fn _dmg_file_name(&self, target: &Path) -> PathBuf {
        target.join("image.dmg.sparseimage")
    }

    #[cfg(target_os = "linux")]
    async fn _bind_mount_linux(&self, checkout_path: &Path, target: &Path) -> Result<()> {
        let abs_mount_path_in_repo = checkout_path.join(target);
        if abs_mount_path_in_repo.exists() {
            // To deal with the case where someone has manually unmounted
            // a bind mount and left the privhelper confused about the
            // list of bind mounts, we first speculatively try asking the
            // eden daemon to unmount it first, ignoring any error that
            // might raise.
            _remove_bind_mount_thrift_call(checkout_path, &self.repo_path).await?;
        }
        // Ensure that the client directory exists before we try to mount over it
        std::fs::create_dir_all(abs_mount_path_in_repo).from_err()?;
        std::fs::create_dir_all(target).from_err()?;
        _add_bind_mount_thrift_call(checkout_path, &self.repo_path, target).await?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    /// Attempt to use an APFS volume for a bind redirection.
    /// The heavy lifting is part of the APFS_HELPER utility found
    /// in `eden/scm/exec/eden_apfs_mount_helper/`
    fn _bind_mount_darwin_apfs(&self, checkout_path: &Path) -> Result<()> {
        let mount_path = checkout_path.join(&self.repo_path);
        std::fs::create_dir_all(&mount_path).from_err()?;
        let mount_path = checkout_path.join(&self.repo_path);
        let status = Exec::cmd(APFS_HELPER)
            .args(&["mount", &mount_path.to_string_lossy()])
            .stdout(SubprocessRedirection::Pipe)
            .stderr(SubprocessRedirection::Pipe)
            .capture()
            .from_err()?;
        if status.success() {
            Ok(())
        } else {
            Err(EdenFsError::Other(anyhow!(
                "failed to add bind mount for mount {}. stderr: {}\n stdout: {}",
                checkout_path.display(),
                status.stderr_str(),
                status.stdout_str()
            )))
        }
    }

    #[cfg(target_os = "macos")]
    fn _bind_mount_darwin_dmg(&self, checkout_path: &Path, target: &Path) -> Result<()> {
        // Since we don't have bind mounts, we set up a disk image file
        // and mount that instead.
        let image_file_path = self._dmg_file_name(target);
        let target_stat = stat(target).from_err()?;

        // Specify the size in kb because the disk utilities have weird
        // defaults if the units are unspecified, and `b` doesn't mean
        // bytes!
        let total_kb = target_stat.st_size / 1024;
        let mount_path = checkout_path.join(&self.repo_path());

        if !image_file_path.exists() {
            // We need to convert paths -> strings for the hdiutil commands
            let image_file_name = image_file_path.to_string_lossy();
            let mount_name = mount_path.to_string_lossy();

            let create_status = Exec::cmd("hdiutil")
                .args(&[
                    "create",
                    "--size",
                    &format!("{}k", total_kb),
                    "--type",
                    "SPARSE",
                    "--fs",
                    "HFS+",
                    "--volname",
                    &format!("EdenFS redirection for {}", &mount_name),
                    &image_file_name,
                ])
                .stdout(SubprocessRedirection::Pipe)
                .stderr(SubprocessRedirection::Pipe)
                .capture()
                .from_err()?;
            if !create_status.success() {
                return Err(EdenFsError::Other(anyhow!(
                    "failed to create dmg volume {} for mount {}. stderr: {}\n stdout: {}",
                    &image_file_name,
                    &mount_name,
                    create_status.stderr_str(),
                    create_status.stdout_str()
                )));
            }

            let attach_status = Exec::cmd("hdiutil")
                .args(&[
                    "attach",
                    &image_file_name,
                    "--nobrowse",
                    "--mountpoint",
                    &mount_name,
                ])
                .stdout(SubprocessRedirection::Pipe)
                .stderr(SubprocessRedirection::Pipe)
                .capture()
                .from_err()?;
            if !attach_status.success() {
                return Err(EdenFsError::Other(anyhow!(
                    "failed to attach dmg volume {} for mount {}. stderr: {}\n stdout: {}",
                    &image_file_name,
                    &mount_name,
                    attach_status.stderr_str(),
                    attach_status.stdout_str()
                )));
            }
        }
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn _bind_mount_darwin(&self, checkout_path: &Path, target: &Path) -> Result<()> {
        if Redirection::have_apfs_helper()? {
            self._bind_mount_darwin_apfs(checkout_path)
        } else {
            self._bind_mount_darwin_dmg(checkout_path, target)
        }
    }

    #[cfg(target_os = "windows")]
    fn _bind_mount_windows(&self, checkout_path: &Path, target: &Path) -> Result<()> {
        self._apply_symlink(checkout_path, target)
    }

    #[cfg(target_os = "linux")]
    async fn _bind_mount(&self, checkout: &Path, target: &Path) -> Result<()> {
        self._bind_mount_linux(checkout, target).await
    }

    #[cfg(target_os = "macos")]
    async fn _bind_mount(&self, checkout: &Path, target: &Path) -> Result<()> {
        self._bind_mount_darwin(checkout, target)
    }

    #[cfg(target_os = "windows")]
    async fn _bind_mount(&self, checkout: &Path, target: &Path) -> Result<()> {
        self._bind_mount_windows(checkout, target)
    }

    #[cfg(all(not(unix), not(windows)))]
    async fn _bind_mount(&self, checkout: &Path, target: &Path) -> Result<()> {
        Err(EdenFsError::Other(anyhow!(
            "could not complete bind mount: unsupported platform"
        )))
    }

    #[cfg(target_os = "linux")]
    async fn _bind_unmount_linux(&self, checkout: &EdenFsCheckout) -> Result<()> {
        _remove_bind_mount_thrift_call(&checkout.path(), &self.repo_path).await?;
        Ok(())
    }

    pub fn expand_repo_path(&self, checkout: &EdenFsCheckout) -> PathBuf {
        checkout.path().join(&self.repo_path)
    }

    #[cfg(target_os = "macos")]
    fn _bind_unmount_darwin(&self, checkout: &EdenFsCheckout) -> Result<()> {
        let mount_path = checkout.path().join(&self.repo_path);
        let status = Exec::cmd("diskutil")
            .args(&["unmount", "force", &mount_path.to_string_lossy()])
            .stdout(SubprocessRedirection::Pipe)
            .stderr(SubprocessRedirection::Pipe)
            .capture()
            .from_err()?;
        if status.success() {
            Ok(())
        } else {
            Err(EdenFsError::Other(anyhow!(format!(
                "failed to remove bind mount. stderr: {}\n stdout: {}",
                status.stderr_str(),
                status.stdout_str()
            ))))
        }
    }

    #[cfg(target_os = "windows")]
    fn _bind_unmount_windows(&self, checkout: &EdenFsCheckout) -> Result<()> {
        let repo_path = self.expand_repo_path(checkout);
        remove_symlink(&repo_path)?;
        Ok(())
    }

    #[cfg(target_os = "windows")]
    async fn _bind_unmount(&self, checkout: &EdenFsCheckout) -> Result<()> {
        self._bind_unmount_windows(checkout)
    }

    #[cfg(target_os = "macos")]
    async fn _bind_unmount(&self, checkout: &EdenFsCheckout) -> Result<()> {
        self._bind_unmount_darwin(checkout)
    }

    #[cfg(target_os = "linux")]
    async fn _bind_unmount(&self, checkout: &EdenFsCheckout) -> Result<()> {
        self._bind_unmount_linux(checkout).await
    }

    /// Attempts to create a symlink at checkout_path/self.repo_path that points to target.
    /// This will fail if checkout_path/self.repo_path already exists
    fn _apply_symlink(&self, checkout_path: &Path, target: &Path) -> Result<()> {
        let symlink_path = checkout_path.join(&self.repo_path);

        // If .parent() resolves to None or parent().exists() == true, we skip directory creation
        if !symlink_path.parent().map_or(true, |parent| parent.exists()) {
            symlink_path.parent().map(std::fs::create_dir_all);
        }

        #[cfg(not(windows))]
        std::os::unix::fs::symlink(target, &symlink_path).from_err()?;

        #[cfg(windows)]
        {
            // Creating a symlink on Windows is non-atomic, and thus when EdenFS
            // gets the notification about a file being created and then goes on
            // testing what's on disk, it may either find a symlink, or a directory.
            //
            // This is bad for EdenFS for a number of reason. The main one being
            // that EdenFS will attempt to recursively add all the childrens of
            // that directory to the inode hierarchy. If the symlinks points to
            // a very large directory, this can be extremely slow, leading to a
            // very poor user experience.
            //
            // Since these symlinks are created for redirections, we can expect
            // the above to be true.
            //
            // To fix this in a generic way is hard to impossible. One of the
            // approach would be to hack in the PrjfsDispatcherImpl.cpp and
            // sleep a bit when we detect a directory, to make sure that we
            // retest it if this was a symlink. This wouldn't work if the system
            // is overloaded, and it would add a small delay to update/status
            // operation due to these waiting on all pending notifications to be
            // handled.
            //
            // Instead, we chose here to handle it in a local way by forcing the
            // redirection to be created atomically. We first create the symlink
            // in the parent directory of the repository, and then move it
            // inside, which is atomic.
            let repo_and_symlink_path = checkout_path.join(&self.repo_path);
            if let Some(temp_symlink_path) = checkout_path.parent().and_then(|co_parent| {
                Some(co_parent.join(zzencode(&repo_and_symlink_path.to_string_lossy())))
            }) {
                // These files should be created by EdenFS only, let's just remove
                // it if it's there.
                if temp_symlink_path.exists() {
                    std::fs::remove_file(&temp_symlink_path).from_err()?;
                }
                std::os::windows::fs::symlink_dir(target, &temp_symlink_path).from_err()?;
                std::fs::rename(&temp_symlink_path, symlink_path).from_err()?;
            } else {
                return Err(EdenFsError::Other(anyhow!(
                    "failed to create symlink for {}",
                    self.repo_path.display()
                )));
            }
        }
        Ok(())
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

#[cfg(target_os = "linux")]
async fn _add_bind_mount_thrift_call(
    mount_path: &Path,
    repo_path: &Path,
    target: &Path,
) -> Result<()> {
    let client = EdenFsInstance::global().connect(None).await?;
    let co_path = mount_path
        .to_str()
        .context("failed to get mount point as str")?
        .as_bytes()
        .to_vec();
    let repo_path = repo_path
        .to_str()
        .context("failed to get mount point as str")?
        .as_bytes()
        .to_vec();
    let target_path = target
        .to_str()
        .context("failed to get mount point as str")?
        .as_bytes()
        .to_vec();
    client
        .addBindMount(&co_path, &repo_path, &target_path)
        .await
        .with_context(|| "failed add bind mount thrift call")?;
    Ok(())
}

#[cfg(target_os = "linux")]
async fn _remove_bind_mount_thrift_call(mount_path: &Path, repo_path: &Path) -> Result<()> {
    let client = EdenFsInstance::global().connect(None).await?;
    let co_path = mount_path
        .to_str()
        .context("failed to get mount point as str")?
        .as_bytes()
        .to_vec();
    let repo_path = repo_path
        .to_str()
        .context("failed to get mount point as str")?
        .as_bytes()
        .to_vec();
    client
        .removeBindMount(&co_path, &repo_path)
        .await
        .with_context(|| "failed remove bind mount thrift call")?;
    Ok(())
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
                    state: Some(RedirectionState::MatchesConfiguration),
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
                    state: Some(RedirectionState::MatchesConfiguration),
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

#[cfg(test)]
pub(crate) mod tests {

    use std::path::PathBuf;

    use rand::distributions::Alphanumeric;
    use rand::distributions::DistString;
    use tempfile::tempdir;

    use crate::redirect::Redirection;
    use crate::redirect::RedirectionType;
    use crate::redirect::REPO_SOURCE;

    #[test]
    fn test_apply_symlink() {
        // The symlink creation will fail if we try to create a symlink where there's an existing
        // file. So let's try to prevent collisions by making the filename random.
        // TODO(@Cuev): Is there a better way to do this?
        let rand_file = format!(
            "test_path_{}",
            Alphanumeric.sample_string(&mut rand::thread_rng(), 16)
        );
        let redir1 = Redirection {
            repo_path: PathBuf::from(rand_file),
            redir_type: RedirectionType::Symlink,
            target: None,
            source: REPO_SOURCE.into(),
            state: None,
        };
        let fake_checkout = tempdir().expect("failed to create fake checkout");
        let fake_checkout_path = fake_checkout.path();

        let symlink_path = fake_checkout_path.join(&redir1.repo_path());
        redir1
            ._apply_symlink(fake_checkout_path, &symlink_path)
            .expect("Failed to create symlink");
        assert!(symlink_path.is_symlink())
    }
}
