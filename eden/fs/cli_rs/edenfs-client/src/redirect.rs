/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
#[cfg(unix)]
use std::ffi::OsStr;
use std::fmt;
#[cfg(unix)]
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Context;
use async_recursion::async_recursion;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_error::ResultExt;
#[cfg(fbcode_build)]
use edenfs_telemetry::redirect::RedirectionOverwriteSample;
#[cfg(fbcode_build)]
use edenfs_telemetry::send;
use edenfs_utils::is_buckd_running_for_path;
use edenfs_utils::metadata::MetadataExt;
use edenfs_utils::remove_symlink;
use edenfs_utils::stop_buckd_for_path;
use edenfs_utils::stop_buckd_for_repo;
#[cfg(fbcode_build)]
use fbinit::expect_init;
#[cfg(target_os = "windows")]
use mkscratch::zzencode;
#[cfg(target_os = "macos")]
use nix::sys::stat::stat;
use pathdiff::diff_paths;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use toml::value::Value;
use util::path::absolute;

use crate::checkout::CheckoutConfig;
use crate::checkout::EdenFsCheckout;
#[cfg(target_os = "linux")]
use crate::instance::EdenFsInstance;
use crate::mounttable::read_mount_table;

pub const REPO_SOURCE: &str = ".eden-redirections";
const USER_REDIRECTION_SOURCE: &str = ".eden/client/config.toml:redirections";
pub const APFS_HELPER: &str = "/usr/local/libexec/eden/eden_apfs_mount_helper";

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
#[derive(PartialEq, Debug)]
pub enum RepoPathDisposition {
    DoesNotExist,
    IsSymlink,
    IsBindMount,
    IsEmptyDir,
    IsNonEmptyDir,
    IsFile,
}

impl RepoPathDisposition {
    pub fn analyze(path: &Path) -> Result<RepoPathDisposition> {
        // We can't simply check path.exists() since that follows symlinks and checks whether the
        // symlink target exists (not the symlink itself). We want to know whether a symlink exists
        // regardless of whether the target exists or not.
        //
        // symlink_metadata() returns an error type if the path DNE and it returns the file
        // metadata otherwise. We can leverage this to tell whether or not the file exists, and
        // whether it's a symlink if it does exist.
        if let Ok(file_type) = std::fs::symlink_metadata(&path).map(|m| m.file_type()) {
            if file_type.is_symlink() {
                return Ok(RepoPathDisposition::IsSymlink);
            }
            if file_type.is_dir() {
                if is_bind_mount(path.into()).with_context(|| {
                    format!(
                        "failed to determine whether {} is a bind mount",
                        path.display()
                    )
                })? {
                    return Ok(RepoPathDisposition::IsBindMount);
                }
                if is_empty_dir(path).with_context(|| {
                    format!(
                        "failed to determine whether {} is an empty dir",
                        path.display()
                    )
                })? {
                    return Ok(RepoPathDisposition::IsEmptyDir);
                }
                return Ok(RepoPathDisposition::IsNonEmptyDir);
            }
            Ok(RepoPathDisposition::IsFile)
        } else {
            Ok(RepoPathDisposition::DoesNotExist)
        }
    }
}

impl fmt::Display for RepoPathDisposition {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{}",
            match *self {
                Self::DoesNotExist => "does-not-exist",
                Self::IsSymlink => "is-symlink",
                Self::IsBindMount => "is-bind-mount",
                Self::IsEmptyDir => "is-empty-dir",
                Self::IsNonEmptyDir => "is-non-empty-dir",
                Self::IsFile => "is-file",
            }
        )
    }
}

#[derive(Debug, Serialize, PartialEq)]
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
    pub fn have_apfs_helper() -> Result<bool> {
        match std::fs::symlink_metadata(APFS_HELPER) {
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
        PathBuf::from("edenfs").join("redirections")
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
        let output = Command::new(&mkscratch)
            .args(args)
            .output()
            .from_err()
            .with_context(|| {
                format!(
                    "Failed to execute mkscratch cmd: `{} {}`",
                    &mkscratch.display(),
                    args.join(" ")
                )
            })?;
        if output.status.success() {
            #[cfg(unix)]
            {
                let path = output.stdout.strip_suffix(b"\n").unwrap_or(&output.stdout);
                return Ok(PathBuf::from(OsStr::from_bytes(path)));
            }
            #[cfg(windows)]
            return Ok(PathBuf::from(
                std::str::from_utf8(&output.stdout).from_err()?.trim_end(),
            ));
        } else {
            Err(EdenFsError::Other(anyhow!(
                "Failed to execute `{} {}`, stderr: {}, exit status: {:?}",
                &mkscratch.display(),
                args.join(" "),
                String::from_utf8_lossy(&output.stderr),
                output.status,
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
        self.target = self.expand_target_abspath(checkout).with_context(|| {
            format!(
                "Failed to update target abspath for redirection: {}",
                self.repo_path.display()
            )
        })?;
        Ok(())
    }

    fn _dmg_file_name(&self, target: &Path) -> PathBuf {
        target.join("image.dmg.sparseimage")
    }

    #[cfg(target_os = "linux")]
    async fn _bind_mount_linux(&self, checkout_path: &Path, target: &Path) -> Result<()> {
        let abs_mount_path_in_repo = checkout_path.join(&self.repo_path);
        if abs_mount_path_in_repo.exists() {
            // To deal with the case where someone has manually unmounted
            // a bind mount and left the privhelper confused about the
            // list of bind mounts, we first speculatively try asking the
            // eden daemon to unmount it first, ignoring any error that
            // might raise.
            _remove_bind_mount_thrift_call(checkout_path, &self.repo_path)
                .await
                .ok();
        }
        // Ensure that the client directory exists before we try to mount over it
        std::fs::create_dir_all(target)
            .from_err()
            .with_context(|| format!("Failed to create directory {}", target.display()))?;
        std::fs::create_dir_all(&abs_mount_path_in_repo)
            .from_err()
            .with_context(|| {
                format!(
                    "Failed to create directory {}",
                    abs_mount_path_in_repo.display()
                )
            })?;
        _add_bind_mount_thrift_call(checkout_path, &self.repo_path, target)
            .await
            .with_context(|| {
                format!(
                    "add_bind_mount thrift call failed for target '{}' in checkout '{}'",
                    target.display(),
                    checkout_path.display()
                )
            })?;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    /// Attempt to use an APFS volume for a bind redirection.
    /// The heavy lifting is part of the APFS_HELPER utility found
    /// in `eden/scm/exec/eden_apfs_mount_helper/`
    fn _bind_mount_darwin_apfs(&self, checkout_path: &Path) -> Result<()> {
        let mount_path = checkout_path.join(&self.repo_path);
        std::fs::create_dir_all(&mount_path)
            .from_err()
            .with_context(|| format!("Failed to create directory {}", &mount_path.display()))?;
        let args = &["mount", &mount_path.to_string_lossy()];
        let output = Command::new(APFS_HELPER)
            .args(args)
            .output()
            .from_err()
            .with_context(|| {
                format!(
                    "Failed to execute command `{} {}`",
                    APFS_HELPER,
                    args.join(" ")
                )
            })?;
        if output.status.success() {
            Ok(())
        } else {
            Err(EdenFsError::Other(anyhow!(
                "failed to add bind mount for mount {}. stderr: {}\n stdout: {}",
                checkout_path.display(),
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            )))
        }
    }

    #[cfg(target_os = "macos")]
    fn _bind_mount_darwin_dmg(&self, checkout_path: &Path, target: &Path) -> Result<()> {
        // Since we don't have bind mounts, we set up a disk image file
        // and mount that instead.
        let image_file_path = self._dmg_file_name(target);
        let target_stat = stat(target)
            .from_err()
            .with_context(|| format!("Failed to stat target {}", target.display()))?;

        // Specify the size in kb because the disk utilities have weird
        // defaults if the units are unspecified, and `b` doesn't mean
        // bytes!
        let total_kb = target_stat.st_size / 1024;
        let mount_path = checkout_path.join(&self.repo_path());

        if !image_file_path.exists() {
            // We need to convert paths -> strings for the hdiutil commands
            let image_file_name = image_file_path.to_string_lossy();
            let mount_name = mount_path.to_string_lossy();

            let args = &[
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
            ];
            let create_output = Command::new("hdiutil")
                .args(args)
                .output()
                .from_err()
                .with_context(|| {
                    format!("Failed to execute command `hdiutil {}`", args.join(" "))
                })?;
            if !create_output.status.success() {
                return Err(EdenFsError::Other(anyhow!(
                    "failed to create dmg volume {} for mount {}. stderr: {}\n stdout: {}",
                    &image_file_name,
                    &mount_name,
                    String::from_utf8_lossy(&create_output.stderr),
                    String::from_utf8_lossy(&create_output.stdout)
                )));
            }

            let args = &[
                "attach",
                &image_file_name,
                "--nobrowse",
                "--mountpoint",
                &mount_name,
            ];
            let attach_output = Command::new("hdiutil")
                .args(args)
                .output()
                .from_err()
                .with_context(|| {
                    format!("Failed to execute command `hdiutil {}`", args.join(" "))
                })?;
            if !attach_output.status.success() {
                return Err(EdenFsError::Other(anyhow!(
                    "failed to attach dmg volume {} for mount {}. stderr: {}\n stdout: {}",
                    &image_file_name,
                    &mount_name,
                    String::from_utf8_lossy(&attach_output.stderr),
                    String::from_utf8_lossy(&attach_output.stdout)
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
            "Could not complete bind mount: unsupported platform"
        )))
    }

    #[cfg(target_os = "linux")]
    async fn _bind_unmount_linux(&self, checkout: &EdenFsCheckout) -> Result<()> {
        _remove_bind_mount_thrift_call(&checkout.path(), &self.repo_path)
            .await
            .with_context(|| {
                format!(
                    "remove_bind_mount thrift call failed for '{}' in checkout '{}'",
                    &self.repo_path.display(),
                    &checkout.path().display()
                )
            })?;
        Ok(())
    }

    pub fn expand_repo_path(&self, checkout: &EdenFsCheckout) -> PathBuf {
        checkout.path().join(&self.repo_path)
    }

    #[cfg(target_os = "macos")]
    fn _bind_unmount_darwin(&self, checkout: &EdenFsCheckout) -> Result<()> {
        let mount_path = checkout.path().join(&self.repo_path);
        let args = &["unmount", "force", &mount_path.to_string_lossy()];
        let output = Command::new("diskutil")
            .args(args)
            .output()
            .from_err()
            .with_context(|| format!("Failed to execute command `diskutil {}`", args.join(" ")))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(EdenFsError::Other(anyhow!(format!(
                "failed to remove bind mount. stderr: {}\n stdout: {}",
                String::from_utf8_lossy(&output.stderr),
                String::from_utf8_lossy(&output.stdout)
            ))))
        }
    }

    #[cfg(target_os = "windows")]
    fn _bind_unmount_windows(&self, checkout: &EdenFsCheckout) -> Result<()> {
        let repo_path = self.expand_repo_path(checkout);
        remove_symlink(&repo_path)
            .with_context(|| format!("Failed to remove symlink {}", repo_path.display()))?;
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
        std::os::unix::fs::symlink(target, &symlink_path)
            .from_err()
            .with_context(|| {
                format!(
                    "Failed to create symlink {} with target {}",
                    &symlink_path.display(),
                    target.display()
                )
            })?;

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
                    std::fs::remove_file(&temp_symlink_path)
                        .from_err()
                        .with_context(|| {
                            format!(
                                "Failed to remove existing file {}",
                                temp_symlink_path.display()
                            )
                        })?;
                }
                std::os::windows::fs::symlink_dir(target, &temp_symlink_path)
                    .from_err()
                    .with_context(|| {
                        format!(
                            "Failed to create symlink {} with target {}",
                            &temp_symlink_path.display(),
                            target.display()
                        )
                    })?;
                std::fs::rename(&temp_symlink_path, &symlink_path)
                    .from_err()
                    .with_context(|| {
                        format!(
                            "Failed to rename symlink {} to {}",
                            &temp_symlink_path.display(),
                            &symlink_path.display()
                        )
                    })?;
            } else {
                return Err(EdenFsError::Other(anyhow!(
                    "failed to create symlink for {}",
                    self.repo_path.display()
                )));
            }
        }
        Ok(())
    }

    #[async_recursion]
    pub async fn remove_existing(
        &self,
        checkout: &EdenFsCheckout,
        fail_if_bind_mount: bool,
    ) -> Result<RepoPathDisposition> {
        let repo_path = self.expand_repo_path(checkout);
        let disposition = RepoPathDisposition::analyze(&repo_path)
            .with_context(|| format!("Failed to analyze path {}", repo_path.display()))?;
        if disposition == RepoPathDisposition::DoesNotExist {
            return Ok(disposition);
        }

        // If this redirect was setup by buck, we should stop buck
        // prior to unmounting it, as it doesn't currently have a
        // great way to detect that the directories have gone away.
        if let Some(possible_buck_project) = repo_path.parent() {
            if is_buckd_running_for_path(possible_buck_project) {
                if let Err(e) = stop_buckd_for_path(possible_buck_project) {
                    eprintln!(
                        "Failed to kill buck. Please manually run `buck kill` in `{}`\n{}\n\n",
                        &possible_buck_project.display(),
                        e
                    );
                }
            }
        }

        // We have encountered issues with buck daemons holding references to files underneath the
        // redirection we're trying to remove. We should kill all buck instances for the repo to
        // guard against these cases and avoid `redirect fixup` failures.
        stop_buckd_for_repo(&checkout.path());

        if disposition == RepoPathDisposition::IsSymlink {
            remove_symlink(&repo_path)
                .with_context(|| format!("Failed to remove symlink {}", repo_path.display()))?;
            return Ok(RepoPathDisposition::DoesNotExist);
        }

        if disposition == RepoPathDisposition::IsBindMount {
            if fail_if_bind_mount {
                return Err(EdenFsError::Other(anyhow!(
                    "Failed to remove bind mount {}",
                    repo_path.display()
                )));
            }
            self._bind_unmount(checkout).await.with_context(|| {
                format!("Failed to unmount bind mount {}", self.repo_path.display())
            })?;

            // Now that it is unmounted, re-assess and ideally
            // remove the empty directory that was the mount point
            // To avoid infinite recursion, tell the next call to fail if
            // the disposition is still a bind mount
            return self.remove_existing(checkout, true).await;
        }

        if disposition == RepoPathDisposition::IsEmptyDir {
            match std::fs::remove_dir(repo_path) {
                Ok(_) => return Ok(RepoPathDisposition::DoesNotExist),
                Err(_) => return Ok(disposition),
            }
        }
        Ok(disposition)
    }

    pub async fn apply(&self, checkout: &EdenFsCheckout) -> Result<()> {
        // Check for non-empty directory. We only care about this if we are creating a symlink type redirection or bind type redirection on Windows.
        let disposition = self
            .remove_existing(checkout, false)
            .await
            .with_context(|| {
                format!(
                    "Failed to remove existing redirection {}",
                    self.repo_path.display()
                )
            })?;
        if disposition == RepoPathDisposition::IsNonEmptyDir
            && (self.redir_type == RedirectionType::Symlink
                || (self.redir_type == RedirectionType::Bind && cfg!(windows)))
        {
            // Part of me would like to show this error even if we're going
            // to mount something over the top, but on macOS the act of mounting
            // disk image can leave marker files like `.automounted` in the
            // directory that we mount over, so let's only treat this as a hard
            // error if we want to redirect using a symlink.
            return Err(EdenFsError::Other(anyhow!(
                "Cannot redirect {} because it is a non-empty directory.  Review its contents and \
                remove it if that is appropriate and then try again.",
                self.repo_path.display()
            )));
        }

        if disposition == RepoPathDisposition::IsFile {
            return Err(EdenFsError::Other(anyhow!(
                "Cannot redirect {} because it is a file",
                self.repo_path.display()
            )));
        }

        if self.redir_type == RedirectionType::Bind {
            let target = self.expand_target_abspath(checkout)?;
            match target {
                Some(t) => self._bind_mount(&checkout.path(), &t).await,
                None => Err(EdenFsError::Other(anyhow!(
                    "failed to expand target abspath for checkout {}",
                    &checkout.path().display()
                ))),
            }
        } else if self.redir_type == RedirectionType::Symlink {
            let target = self.expand_target_abspath(checkout).with_context(|| {
                format!(
                    "Failed to expand abspath for target {} in checkout {}",
                    self.target
                        .as_ref()
                        .unwrap_or(&PathBuf::from("DoesNotExist"))
                        .display(),
                    checkout.path().display()
                )
            })?;
            match target {
                Some(t) => self._apply_symlink(&checkout.path(), &t),
                None => Err(EdenFsError::Other(anyhow!(
                    "failed to expand target abspath for checkout {}",
                    &checkout.path().display()
                ))),
            }
        } else {
            Err(EdenFsError::Other(anyhow!(
                "Unsupported redirection type {}",
                self.redir_type
            )))
        }
    }
}

/// Detect the most common form of a bind mount in the repo;
/// its parent directory will have a different device number than
/// the mount point itself.  This won't detect something funky like
/// bind mounting part of the repo to a different part.
pub(crate) fn is_bind_mount(path: PathBuf) -> Result<bool> {
    let parent = path.parent();
    if let Some(parent_path) = parent {
        let path_metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
        .from_err()
        .with_context(|| format!("Failed to get symlink metadata for path {}", path.display()))?;
        let parent_metadata = match std::fs::symlink_metadata(parent_path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e),
        }
        .from_err()
        .with_context(|| {
            format!(
                "Failed to get symlink metadata for path {}",
                parent_path.display()
            )
        })?;

        match (path_metadata, parent_metadata) {
            (Some(m1), Some(m2)) => Ok(m1.eden_dev() != m2.eden_dev()),
            _ => Ok(false),
        }
    } else {
        Ok(false)
    }
}

pub fn is_empty_dir(path: &Path) -> Result<bool> {
    let mut dir_iter = path
        .read_dir()
        .with_context(|| anyhow!("failed to read directory {}", path.display()))?;
    // read_dir returns a directory iter that skips . and ..
    // Therefore, if .next() -> None, we know the dir is empty
    Ok(dir_iter.next().is_none())
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
    let client = EdenFsInstance::global()
        .connect(None)
        .await
        .with_context(|| "Unable to connect to EdenFS for add_bind_mount thrift call")?;
    let co_path = mount_path
        .to_str()
        .with_context(|| {
            format!(
                "Failed to get mount point '{}' as str",
                mount_path.display()
            )
        })?
        .as_bytes()
        .to_vec();
    let repo_path = repo_path
        .to_str()
        .with_context(|| format!("Failed to get repo path '{}' as str", repo_path.display()))?
        .as_bytes()
        .to_vec();
    let target_path = target
        .to_str()
        .with_context(|| format!("Failed to get target '{}' as str", target.display()))?
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
    let client = EdenFsInstance::global()
        .connect(None)
        .await
        .with_context(|| "Unable to connect to EdenFS for remove_bind_mount thrift call")?;
    let co_path = mount_path
        .to_str()
        .with_context(|| {
            format!(
                "Failed to get mount point '{}' as str",
                mount_path.display()
            )
        })?
        .as_bytes()
        .to_vec();
    let repo_path = repo_path
        .to_str()
        .with_context(|| format!("Failed to get repo path '{}' as str", repo_path.display()))?
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
pub fn get_configured_redirections(
    checkout: &EdenFsCheckout,
) -> Result<BTreeMap<PathBuf, Redirection>> {
    let mut redirs = BTreeMap::new();

    // Repo-specified settings have the lowest level of precedence
    let repo_redirection_config_file_name = checkout.path().join(".eden-redirections");
    if let Ok(contents) = std::fs::read(repo_redirection_config_file_name) {
        let s = String::from_utf8(contents).from_err()?;
        let config: RedirectionsConfig = toml::from_str(&s)
            .from_err()
            .with_context(|| format!("Failed to create RedirectionsConfig from str '{}'", &s))?;
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
    if let Some(expected_target) = redir.expand_target_abspath(checkout).with_context(|| {
        format!(
            "Failed to expand abspath for target {} in checkout {}",
            redir
                .target
                .as_ref()
                .unwrap_or(&PathBuf::from("DoesNotExist"))
                .display(),
            checkout.path().display()
        )
    })? {
        let expected_target = std::fs::canonicalize(&expected_target)
            .from_err()
            .with_context(|| {
                format!("Failed to canonicalize path {}", expected_target.display())
            })?;
        let symlink_path = checkout.path().join(&redir.repo_path);
        let target_path = std::fs::read_link(&symlink_path).with_context(|| {
            format!("Failed to read link for symlink {}", symlink_path.display())
        })?;
        let target = std::fs::canonicalize(&target_path)
            .from_err()
            .with_context(|| format!("Failed to canonicalize path {}", target_path.display()))?;
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
    let mount_table = read_mount_table().context("Failed to read mount table")?;
    for mount_info in mount_table {
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
            let is_mnt_path_a_bind_mount =
                is_bind_mount(mount_info.mount_point()).with_context(|| {
                    format!(
                        "Failed to check if mount point '{}' is a bind mount",
                        mount_info.mount_point().display()
                    )
                })?;
            if path_prefix != mount_point && is_mnt_path_a_bind_mount {
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

    let configured_redirections = get_configured_redirections(checkout).with_context(|| {
        format!(
            "Failed to get configured redirections for checkout {}",
            checkout.path().display()
        )
    })?;
    for (rel_path, mut redir) in configured_redirections {
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

/// We should return success early iff:
/// 1) we're adding a symlink redirection
/// 2) the symlink already exists
/// 3) the symlink is already a redirection that's managed by EdenFS
fn _should_return_success_early(
    redir_type: RedirectionType,
    configured_redirections: &BTreeMap<PathBuf, Redirection>,
    checkout_path: &Path,
    repo_path: &Path,
) -> Result<bool> {
    if redir_type == RedirectionType::Symlink {
        // We cannot use resolve_repo_relative_path() because it will essentially
        // attempt to resolve any existing symlinks twice. This causes us to never
        // return the correct path for existing symlinks. Instead, we skip resolving
        // and simply check if the absolute path is relative to the checkout path
        // and if any relative paths are pre-existing configured redirections.
        let mut relative_path = repo_path.to_owned();
        if repo_path.is_absolute() {
            let canonical_repo_path = repo_path;
            if !canonical_repo_path.starts_with(checkout_path) {
                return Err(EdenFsError::Other(anyhow!(
                    "The redirection path `{}` doesn't resolve \
                    to a path inside the repo `{}`",
                    repo_path.display(),
                    checkout_path.display()
                )));
            }
            relative_path = diff_paths(canonical_repo_path, checkout_path).unwrap_or_default();
        }
        if let Some(redir) = configured_redirections.get(&relative_path) {
            return Ok(
                redir.redir_type == RedirectionType::Symlink && redir.repo_path == relative_path
            );
        }
    }
    Ok(false)
}

/// Given a path, verify that it is an appropriate repo-root-relative path
/// and return the resolved form of that path.
///
/// The ideal is that they pass in `foo` and we return `foo`, but we also
/// allow for the path to be absolute path to `foo`, in which case we resolve
/// it and verify that it falls with the repo and then return the relative
/// path to `foo`.
fn resolve_repo_relative_path(checkout: &EdenFsCheckout, repo_rel_path: &Path) -> Result<PathBuf> {
    let checkout_path = checkout.path();
    if repo_rel_path.is_absolute() {
        // Well, the original intent was to only interpret paths as relative
        // to the repo root, but it's a bit burdensome to require the caller
        // to correctly relativize for that case, so we'll allow an absolute
        // path to be specified.
        if repo_rel_path.starts_with(&checkout_path) {
            let canonical_path = absolute(&repo_rel_path).from_err().with_context(|| {
                format!(
                    "Failed to find absolute and normalized path: {}",
                    repo_rel_path.display()
                )
            })?;
            let rel_path = diff_paths(&canonical_path, &checkout_path).with_context(|| {
                format!(
                    "{} starts with {}, but we failed to compute the relative repo path.",
                    &canonical_path.display(),
                    &checkout_path.display(),
                )
            })?;
            return Ok(rel_path);
        } else {
            return Err(EdenFsError::Other(anyhow!(
                "The path `{}` doesn't resolve to a path inside the repo `{}`",
                repo_rel_path.display(),
                checkout_path.display()
            )));
        };
    }

    // Otherwise, the path must be interpreted as being relative to the repo
    // root, so let's resolve that and verify that it lies within the repo
    let candidate_path = checkout_path.join(repo_rel_path);
    let candidate = absolute(&candidate_path).from_err().with_context(|| {
        format!(
            "Failed to get absolute path for {}",
            candidate_path.display()
        )
    })?;

    if !candidate.starts_with(&checkout_path) {
        return Err(EdenFsError::Other(anyhow!(
            "The redirection  path `{}` doesn't resolve \
            to a path inside the repo `{}`",
            repo_rel_path.display(),
            checkout_path.display()
        )));
    }
    let relative_path = diff_paths(candidate, &checkout_path).unwrap_or_default();

    // If the resolved and relativized path doesn't match the user-specified
    // path then it means that they either used `..` or a path that resolved
    // through a symlink.  The former is ambiguous, especially because it likely
    // implies that the user is assuming that the path is current working directory
    // relative instead of repo root relative, and the latter is problematic for
    // all of the usual symlink reasons.
    if relative_path != repo_rel_path {
        Err(EdenFsError::Other(anyhow!(
            "The redirection path `{}` resolves to `{}` but must be a canonical \
            repo-root-relative path. Specify either a canonical absolute path \
            to the redirection, or a canonical (without `..` components) path \
            relative to the repository root at `{}`.",
            repo_rel_path.display(),
            relative_path.display(),
            checkout_path.display(),
        )))
    } else {
        Ok(repo_rel_path.to_owned())
    }
}

pub async fn try_add_redirection(
    checkout: &EdenFsCheckout,
    config_dir: &Path,
    repo_path: &Path,
    redir_type: RedirectionType,
    force_remount_bind_mounts: bool,
    strict: bool,
) -> Result<i32> {
    // Get only the explicitly configured entries for the purposes of the
    // add command, so that we avoid writing out any of the effective list
    // of redirections to the local configuration.  That doesn't matter so
    // much at this stage, but when we add loading in profile(s) later we
    // don't want to scoop those up and write them out to this branch of
    // the configuration.
    let mut configured_redirs = get_configured_redirections(checkout).with_context(|| {
        format!(
            "Failed to get configured redirections for checkout {}",
            checkout.path().display()
        )
    })?;

    // We are only checking for pre-existing symlinks in this method, so we
    // can use the configured mounts instead of the effective mounts. This is
    // because the symlinks contained in these lists should be the same. I.e.
    // if a symlink is configured, it is also effective.
    if _should_return_success_early(redir_type, &configured_redirs, &checkout.path(), repo_path)? {
        println!("EdenFS managed symlink redirection already exists.");
        return Ok(0);
    }

    // We need to query the status of the mounts to catch things like
    // a redirect being configured but unmounted.  This improves the
    // UX in the case where eg: buck is adding a redirect.  Without this
    // we'd hit the skip case below because it is configured, but we wouldn't
    // bring the redirection back online.
    // However, we keep this separate from the `redirs` list below for
    // the reasons stated in the comment above.
    let effective_redirs = get_effective_redirections(checkout).with_context(|| {
        format!(
            "Failed to get effective redirections for checkout {}",
            checkout.path().display()
        )
    })?;

    let resolved_repo_path =
        resolve_repo_relative_path(checkout, repo_path).with_context(|| {
            format!(
                "Failed to resolve repo relative path for '{}' in checkout {}",
                repo_path.display(),
                checkout.path().display()
            )
        })?;

    let redir = Redirection {
        repo_path: resolved_repo_path.clone(),
        redir_type,
        target: None,
        source: USER_REDIRECTION_SOURCE.to_string(),
        state: Some(RedirectionState::MatchesConfiguration),
    };

    if let Some(existing_redir) = effective_redirs.get(&resolved_repo_path) {
        if let Some(existing_redir_state) = &existing_redir.state {
            if existing_redir.repo_path == redir.repo_path
                && !force_remount_bind_mounts
                && *existing_redir_state != RedirectionState::NotMounted
            {
                println!(
                    "Skipping {}; it is already configured. (use \
                    --force-remount-bind-mounts to force reconfiguring this \
                    redirection.",
                    resolved_repo_path.display(),
                );
                return Ok(0);
            }
        }
    }
    // We should prevent users from accidentally overwriting existing
    // directories. We only need to check this condition for bind mounts
    // because symlinks should already fail if the target dir exists.
    if redir_type == RedirectionType::Bind && redir.repo_path().is_dir() {
        if !strict {
            println!(
                "WARNING: {} already exists.\nMounting over \
                an existing directory will overwrite its contents.\nYou can \
                use --strict to prevent overwriting existing directories.\n",
                redir.repo_path.display()
            );
            #[cfg(fbcode_build)]
            {
                let sample = RedirectionOverwriteSample::build(
                    expect_init(),
                    &redir.repo_path.to_string_lossy(),
                    &checkout.path().to_string_lossy(),
                );
                send(sample.builder);
            }
        } else {
            println!(
                "Not adding redirection {} because \
                the --strict option was used.\nIf you would like \
                to add this redirection (not recommended), then \
                rerun this command without --strict.",
                redir.repo_path.display()
            );
            return Ok(1);
        }
    }

    redir.apply(checkout).await.with_context(|| {
        format!(
            "Failed to apply redirection '{}' for checkout {}",
            redir.repo_path.display(),
            checkout.path().display()
        )
    })?;

    // We expressly allow replacing an existing configuration in order to
    // support a user with a local ad-hoc override for global- or profile-
    // specified configuration.
    configured_redirs.insert(repo_path.to_owned(), redir);
    let mut checkout_config =
        CheckoutConfig::parse_config(config_dir.into()).with_context(|| {
            format!(
                "Failed to parse checkout config using config dir {}",
                config_dir.display()
            )
        })?;
    // and persist the configuration so that we can re-apply it in a subsequent
    // call to `edenfsctl redirect fixup`
    checkout_config
        .update_redirections(config_dir, &configured_redirs)
        .with_context(|| {
            format!(
                "Failed to update redirections for checkout {}",
                checkout.path().display()
            )
        })?;

    Ok(0)
}

pub mod scratch {
    use std::collections::BTreeSet;
    use std::collections::VecDeque;
    use std::fs;
    use std::fs::DirEntry;
    use std::path::Path;
    use std::path::PathBuf;

    use anyhow::Result;
    use edenfs_utils::metadata::MetadataExt;
    use subprocess::Exec;
    use subprocess::Redirection as SubprocessRedirection;

    use super::Redirection;

    pub fn usage_for_dir(
        path: &Path,
        device_id: Option<u64>,
    ) -> std::io::Result<(u64, Vec<PathBuf>)> {
        let device_id = match device_id {
            Some(device_id) => device_id,
            None => match fs::metadata(path) {
                Ok(metadata) => metadata.eden_dev(),
                Err(e) if ignored_io_error(&e) => return Ok((0, vec![path.to_path_buf()])),
                Err(e) => return Err(e),
            },
        };

        let mut total_size = 0;
        let mut failed_to_check_files = Vec::new();
        for dirent in fs::read_dir(path)? {
            match usage_for_dir_entry(dirent, device_id) {
                Ok((subtotal_size, mut failed_files)) => {
                    total_size += subtotal_size;
                    failed_to_check_files.append(&mut failed_files);
                    Ok(())
                }
                Err(e) if ignored_io_error(&e) => {
                    failed_to_check_files.push(path.to_path_buf());
                    Ok(())
                }
                Err(e) => Err(e),
            }?;
        }
        Ok((total_size, failed_to_check_files))
    }

    /// Intended to only be called by [usage_for_dir]
    fn usage_for_dir_entry(
        dirent: std::io::Result<DirEntry>,
        parent_device_id: u64,
    ) -> std::io::Result<(u64, Vec<PathBuf>)> {
        let entry = dirent?;
        let symlink_metadata = fs::symlink_metadata(entry.path())?;
        if symlink_metadata.is_dir() {
            // Don't recurse onto different filesystems
            if cfg!(windows) || symlink_metadata.eden_dev() == parent_device_id {
                usage_for_dir(&entry.path(), Some(parent_device_id))
            } else {
                Ok((0, vec![]))
            }
        } else {
            Ok((symlink_metadata.eden_file_size(), vec![]))
        }
    }

    fn ignored_io_error(error: &std::io::Error) -> bool {
        error.kind() == std::io::ErrorKind::NotFound
            || error.kind() == std::io::ErrorKind::PermissionDenied
    }

    /// Find all the directories under `redirection_path` that aren't present in
    /// `existing_redirections`.
    fn recursively_check_orphaned_mirrored_redirections(
        redirection_path: PathBuf,
        existing_redirections: &BTreeSet<PathBuf>,
    ) -> std::io::Result<Vec<PathBuf>> {
        let mut to_walk = VecDeque::new();
        to_walk.push_back(redirection_path);

        let mut orphaned = Vec::new();
        while let Some(current) = to_walk.pop_front() {
            // A range is required here to distinguish 3 cases:
            //  0) Is that path an existing redirection
            //  1) Is there an existing redirection in a subdirectory?
            //  2) Is this an orphaned redirection?
            let num_existing_redirections = existing_redirections
                // Logarithmically filter all the paths whose prefix is `current`
                .range(std::ops::RangeFrom {
                    start: current.clone(),
                })
                // And then filter the remaining paths whose prefix do not start with `current`.
                .take_while(|p| p.starts_with(&current))
                .count();
            match num_existing_redirections {
                0 => orphaned.push(current),
                1 if existing_redirections.contains(&current) => continue,
                _ => {
                    if current.is_dir() {
                        for current_subdir in fs::read_dir(current)? {
                            to_walk.push_back(current_subdir?.path());
                        }
                    }
                }
            }
        }

        Ok(orphaned)
    }

    fn get_orphaned_redirection_targets_impl(
        scratch_path: PathBuf,
        scratch_subdir: PathBuf,
        existing_redirections: &BTreeSet<PathBuf>,
    ) -> Result<Vec<PathBuf>> {
        // Scratch directories can either be flat, ie: a directory like foo/bar will be encoded as
        // fooZbar, or mirrored, where no encoding is performed. Let's test how mkscratch encoded the
        // directory and compare it against the EdenFS scratch namespace to test if mkscratch is
        // configured to be flat or mirrored.
        let is_scratch_mirrored = scratch_path.ends_with(&scratch_subdir);
        let (scratch_root, prefix) = if is_scratch_mirrored {
            (
                scratch_path
                    .ancestors()
                    .nth(scratch_subdir.components().count() + 1)
                    .unwrap(),
                scratch_subdir,
            )
        } else {
            (
                // We want to get the root of the scratch directory, which is 2 level up from the path
                // mkscratch gave us: first the path in the repository, and second the repository path.
                scratch_path.parent().unwrap().parent().unwrap(),
                PathBuf::from(scratch_path.file_name().unwrap().to_os_string()),
            )
        };

        let mut orphaned_redirections = Vec::new();
        if is_scratch_mirrored {
            for dirent in fs::read_dir(scratch_root)? {
                let dirent_path = dirent?.path();
                let redirection_path = dirent_path.join(&prefix);
                if redirection_path.exists() {
                    // The directory exist, now we need to check if there is an unknown redirection.
                    orphaned_redirections.extend(recursively_check_orphaned_mirrored_redirections(
                        redirection_path,
                        existing_redirections,
                    )?);
                }
            }
        } else {
            for dirent in fs::read_dir(scratch_root)? {
                let dirent_path = dirent?.path();
                if !dirent_path.is_dir() {
                    continue;
                }

                for subdir in fs::read_dir(dirent_path)? {
                    let path = subdir?.path();
                    if !existing_redirections.contains(&path)
                        && path
                            .file_name()
                            .unwrap()
                            .to_string_lossy()
                            .starts_with(&prefix.to_string_lossy().into_owned())
                    {
                        orphaned_redirections.push(path);
                    }
                }
            }
        }

        Ok(orphaned_redirections)
    }

    pub fn get_orphaned_redirection_targets(
        existing_redirections: &BTreeSet<PathBuf>,
    ) -> Result<Vec<PathBuf>> {
        let mkscratch = Redirection::mkscratch_bin();
        let scratch_subdir = Redirection::scratch_subdir();
        let scratch_subdir_str = scratch_subdir.to_string_lossy();
        let home_dir = match dirs::home_dir() {
            Some(dir) => dir,
            None => return Ok(vec![]),
        };
        let home_dir_str = home_dir.to_string_lossy();

        let mkscratch_args = vec![
            "--no-create",
            "path",
            &*home_dir_str,
            "--subdir",
            &*scratch_subdir_str,
        ];
        let mkscratch_res = Exec::cmd(mkscratch)
            .args(&mkscratch_args)
            .stdout(SubprocessRedirection::Pipe)
            .stderr(SubprocessRedirection::Pipe)
            .capture();

        let scratch_path = match mkscratch_res {
            Ok(output) if output.success() => PathBuf::from(output.stdout_str().trim()),
            _ => return Ok(vec![]),
        };

        get_orphaned_redirection_targets_impl(scratch_path, scratch_subdir, existing_redirections)
    }

    #[cfg(test)]
    mod tests {
        use std::fs::create_dir;
        use std::fs::create_dir_all;
        use std::path::Path;

        use tempfile::TempDir;

        use super::*;

        fn create_and_add(
            path: impl AsRef<Path>,
            existing_redirections: &mut BTreeSet<PathBuf>,
        ) -> Result<()> {
            create_dir_all(path.as_ref())?;
            existing_redirections.insert(path.as_ref().to_path_buf());
            Ok(())
        }

        #[test]
        fn test_recursive_check_orphaned_mirrored_redirections() -> Result<()> {
            let tempdir = TempDir::new()?;
            let path = tempdir.path();
            let mut existing_redirections = BTreeSet::new();

            // Single known directory
            create_and_add(path.join("A"), &mut existing_redirections)?;

            // Directory with an orphaned directory inside
            create_and_add(path.join("B/1"), &mut existing_redirections)?;
            create_and_add(path.join("B/2"), &mut existing_redirections)?;
            create_dir(path.join("B/3"))?;

            // Single orphaned directory
            create_dir(path.join("C"))?;

            // Single orphaned with several subdirectories
            create_dir_all(path.join("D/1"))?;
            create_dir_all(path.join("D/2"))?;
            create_dir_all(path.join("D/3"))?;

            // Orphaned redirection with an existing redirection as a sibling
            create_dir_all(path.join("E/1/2"))?;
            create_and_add(path.join("E/1/3"), &mut existing_redirections)?;

            let res = recursively_check_orphaned_mirrored_redirections(
                path.to_path_buf(),
                &existing_redirections,
            )?;
            assert!(!res.contains(&path.join("A")));

            assert!(!res.contains(&path.join("B")));
            assert!(!res.contains(&path.join("B/1")));
            assert!(!res.contains(&path.join("B/2")));
            assert!(res.contains(&path.join("B/3")));

            assert!(res.contains(&path.join("C")));

            assert!(res.contains(&path.join("D")));

            eprintln!("{:?}", res);
            assert!(res.contains(&path.join("E/1/2")));
            Ok(())
        }

        #[test]
        fn test_get_orphaned_redirection_targets_mirrored() -> Result<()> {
            let tempdir = TempDir::new()?;
            let path = tempdir.path();
            let scratch_subdir = Path::new("foo/bar");
            let mut existing_redirections = BTreeSet::new();

            let scratch_path = path.join("repository").join(scratch_subdir);

            // Single known directory
            create_and_add(
                path.join("repo1").join(scratch_subdir).join("A"),
                &mut existing_redirections,
            )?;

            // Directory with an orphaned directory inside
            create_and_add(
                path.join("repo2").join(scratch_subdir).join("B/1"),
                &mut existing_redirections,
            )?;
            create_and_add(
                path.join("repo2").join(scratch_subdir).join("B/2"),
                &mut existing_redirections,
            )?;
            create_dir_all(path.join("repo2").join(scratch_subdir).join("B/3"))?;

            // Single orphaned directory
            create_dir_all(path.join("repo3").join(scratch_subdir).join("C"))?;

            // Single orphaned with several subdirectories
            create_dir_all(path.join("repo4").join(scratch_subdir).join("D/1"))?;
            create_dir_all(path.join("repo4").join(scratch_subdir).join("D/2"))?;
            create_dir_all(path.join("repo4").join(scratch_subdir).join("D/3"))?;

            let res = get_orphaned_redirection_targets_impl(
                scratch_path,
                scratch_subdir.to_path_buf(),
                &existing_redirections,
            )?;
            assert!(!res.contains(&path.join("repo1").join(scratch_subdir).join("A")));

            assert!(!res.contains(&path.join("repo2").join(scratch_subdir).join("B")));
            assert!(!res.contains(&path.join("repo2").join(scratch_subdir).join("B/1")));
            assert!(!res.contains(&path.join("repo2").join(scratch_subdir).join("B/2")));
            assert!(res.contains(&path.join("repo2").join(scratch_subdir).join("B/3")));

            assert!(res.contains(&path.join("repo3").join(scratch_subdir)));

            assert!(res.contains(&path.join("repo4").join(scratch_subdir)));

            Ok(())
        }

        #[test]
        fn test_get_orphaned_redirection_targets_flat() -> Result<()> {
            let tempdir = TempDir::new()?;
            let path = tempdir.path();
            let scratch_subdir = Path::new("fooZbar");
            let mut existing_redirections = BTreeSet::new();

            let scratch_path = path.join("repository").join(scratch_subdir);

            // Single known directory
            let repo1_a_path =
                path.join("repo1")
                    .join(format!("{}Z{}", scratch_subdir.display(), "A"));
            create_and_add(&repo1_a_path, &mut existing_redirections)?;

            // Directory with an orphaned directory inside
            let repo2_b1_path =
                path.join("repo2")
                    .join(format!("{}Z{}Z{}", scratch_subdir.display(), "B", "1"));
            let repo2_b2_path =
                path.join("repo2")
                    .join(format!("{}Z{}Z{}", scratch_subdir.display(), "B", "2"));
            let repo2_b3_path =
                path.join("repo2")
                    .join(format!("{}Z{}Z{}", scratch_subdir.display(), "B", "3"));
            create_and_add(&repo2_b1_path, &mut existing_redirections)?;
            create_and_add(&repo2_b2_path, &mut existing_redirections)?;
            create_dir_all(&repo2_b3_path)?;

            // Single orphaned directory
            let repo3_c_path =
                path.join("repo3")
                    .join(format!("{}Z{}", scratch_subdir.display(), "C"));
            create_dir_all(&repo3_c_path)?;

            let res = get_orphaned_redirection_targets_impl(
                scratch_path,
                Path::new("foo/bar").to_path_buf(),
                &existing_redirections,
            )?;
            assert!(!res.contains(&repo1_a_path));

            assert!(!res.contains(&repo2_b1_path));
            assert!(!res.contains(&repo2_b2_path));
            assert!(res.contains(&repo2_b3_path));

            assert!(res.contains(&repo3_c_path));

            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::path::PathBuf;

    use rand::distributions::Alphanumeric;
    use rand::distributions::DistString;
    use serde_test::assert_ser_tokens;
    use serde_test::Token;
    use tempfile::tempdir;

    use crate::redirect::Redirection;
    use crate::redirect::RedirectionType;
    use crate::redirect::RepoPathDisposition;
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

    /// returns true if we succeeded in removing the existing file/dir
    fn try_remove(path: &Path) -> bool {
        if path.is_file() {
            std::fs::remove_file(path).ok();
        } else if path.is_dir() {
            std::fs::remove_dir_all(path).ok();
        }
        !path.exists()
    }

    fn check_empty_dir_and_non_empty_dir_and_file(dir_path: &Path) {
        std::fs::create_dir_all(dir_path).ok();
        assert_eq!(
            RepoPathDisposition::analyze(dir_path).expect("failed to analyze RepoPathDisposition"),
            RepoPathDisposition::IsEmptyDir
        );
        let test_file = dir_path.join("test_file");
        std::fs::File::create(&test_file).ok();
        assert_eq!(
            RepoPathDisposition::analyze(&test_file)
                .expect("failed to analyze RepoPathDisposition"),
            RepoPathDisposition::IsFile
        );
        assert_eq!(
            RepoPathDisposition::analyze(dir_path).expect("failed to analyze RepoPathDisposition"),
            RepoPathDisposition::IsNonEmptyDir
        );
    }

    /// We will test DNE, Empty dir, Non-empty dir, file, and symlink dispositions. We skip bind
    /// mounts since those are tough to test
    #[test]
    fn test_analyze() {
        // test non-existent path
        let dne_dir = tempdir().expect("couldn't create temporary directory for testing");
        let dne_path = dne_dir
            .path()
            .join(Alphanumeric.sample_string(&mut rand::thread_rng(), 16));
        #[allow(clippy::if_same_then_else)]
        if !dne_path.exists() {
            assert_eq!(
                RepoPathDisposition::analyze(&dne_path)
                    .expect("failed to analyze RepoPathDisposition"),
                RepoPathDisposition::DoesNotExist
            );
        // we were unlucky enough to somehow collide with an existing file. Let's try removing
        // it. If that fails, just skip this case
        } else if try_remove(&dne_path) {
            assert_eq!(
                RepoPathDisposition::analyze(&dne_path)
                    .expect("failed to analyze RepoPathDisposition"),
                RepoPathDisposition::DoesNotExist
            );
        }

        // empty dir, non-empty dir, and file
        let tmp_dir = tempdir().expect("couldn't create temp directory for testing");
        let dir_path = tmp_dir
            .path()
            .join(Alphanumeric.sample_string(&mut rand::thread_rng(), 16));
        #[allow(clippy::if_same_then_else)]
        if !dir_path.exists() {
            check_empty_dir_and_non_empty_dir_and_file(&dir_path);
        } else if try_remove(&dir_path) {
            check_empty_dir_and_non_empty_dir_and_file(&dir_path);
        }

        // symlink
        let symlink_dir = tempdir().expect("couldn't create temp directory for testing");
        let symlink_path = symlink_dir
            .path()
            .join(Alphanumeric.sample_string(&mut rand::thread_rng(), 16));
        #[cfg(windows)]
        std::os::windows::fs::symlink_file(&dir_path, &symlink_path).ok();
        #[cfg(not(windows))]
        std::os::unix::fs::symlink(&dir_path, &symlink_path).ok();

        if std::fs::symlink_metadata(&symlink_path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            // we actually created a symlink, so we can test if disposition detects the symlink
            assert_eq!(
                RepoPathDisposition::analyze(&symlink_path)
                    .expect("failed to analyze RepoPathDisposition"),
                RepoPathDisposition::IsSymlink
            );
        }
        // if we failed to make the symlink, skip this test case
    }

    /// The format of JSON-serialized redirections is relied upon by callers of
    /// `redirect --json`, so we should try not to break them.
    #[test]
    fn test_serialize_redirection() {
        assert_ser_tokens(
            &Redirection {
                repo_path: "/mnt/foo".into(),
                redir_type: RedirectionType::Bind,
                source: "test".to_string(),
                state: None,
                target: None,
            },
            &[
                Token::Struct {
                    name: "Redirection",
                    len: 5,
                },
                Token::Str("repo_path"),
                Token::Str("/mnt/foo"),
                Token::Str("type"),
                Token::UnitVariant {
                    name: "RedirectionType",
                    variant: "bind",
                },
                Token::Str("source"),
                Token::Str("test"),
                Token::Str("state"),
                Token::None,
                Token::Str("target"),
                Token::None,
                Token::StructEnd,
            ],
        );

        assert_ser_tokens(
            &Redirection {
                repo_path: "/mnt/foo".into(),
                redir_type: RedirectionType::Bind,
                source: "test".to_string(),
                state: None,
                target: Some("/mnt/target".into()),
            },
            &[
                Token::Struct {
                    name: "Redirection",
                    len: 5,
                },
                Token::Str("repo_path"),
                Token::Str("/mnt/foo"),
                Token::Str("type"),
                Token::UnitVariant {
                    name: "RedirectionType",
                    variant: "bind",
                },
                Token::Str("source"),
                Token::Str("test"),
                Token::Str("state"),
                Token::None,
                Token::Str("target"),
                Token::Some,
                Token::Str("/mnt/target"),
                Token::StructEnd,
            ],
        );
    }
}
