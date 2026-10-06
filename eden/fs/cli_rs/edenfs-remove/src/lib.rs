/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt;
#[cfg(unix)]
use std::fs::Permissions;
use std::future::Future;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use edenfs_client::checkout::find_checkout;
use edenfs_client::checkout::get_mounts;
use edenfs_client::fsutil::forcefully_remove_dir_all;
use edenfs_client::instance::EdenFsInstance;
use edenfs_client::redirect::get_effective_redirections;
use edenfs_utils::is_active_eden_mount;
use fail::fail_point;
use tracing::debug;
use tracing::warn;

const DEFAULT_AUXILIARY_PROCESS_TIMEOUT: Duration = Duration::from_secs(60);

/// Options controlling removal of one registered EdenFS checkout.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RemoveCheckoutOptions {
    /// Leave the checkout mount-point directory in place after unregistering it.
    pub preserve_mount_point: bool,
    /// Disable forced EdenFS unmounting.
    pub no_force: bool,
    /// Maximum time to wait for redirection unmounting.
    pub auxiliary_process_timeout: Duration,
}

impl Default for RemoveCheckoutOptions {
    fn default() -> Self {
        Self {
            preserve_mount_point: false,
            no_force: false,
            auxiliary_process_timeout: DEFAULT_AUXILIARY_PROCESS_TIMEOUT,
        }
    }
}

/// A non-fatal condition encountered while removing a checkout.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RemoveCheckoutWarning {
    /// Redirection cleanup failed before checkout removal continued.
    RedirectionUnmountFailed { path: PathBuf, error: String },
    /// Redirection cleanup exceeded the configured timeout before checkout removal continued.
    RedirectionUnmountTimedOut { path: PathBuf, timeout: Duration },
}

impl fmt::Display for RemoveCheckoutWarning {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RedirectionUnmountFailed { path, error } => write!(
                formatter,
                "Error unmounting redirections for {}: {error}",
                path.display()
            ),
            Self::RedirectionUnmountTimedOut { path, timeout } => write!(
                formatter,
                "Unmounting redirections for {} timed out after {} seconds. Continuing with unmount...",
                path.display(),
                timeout.as_secs_f64()
            ),
        }
    }
}

/// Remove one registered EdenFS checkout.
pub async fn remove_checkout(
    instance: &EdenFsInstance,
    path: &Path,
    options: RemoveCheckoutOptions,
) -> Result<()> {
    remove_checkout_with_warning_handler(instance, path, options, |warning| warn!("{warning}"))
        .await
}

/// Remove one registered EdenFS checkout and report non-fatal warnings to the caller.
pub async fn remove_checkout_with_warning_handler(
    instance: &EdenFsInstance,
    path: &Path,
    options: RemoveCheckoutOptions,
    mut warning_handler: impl FnMut(RemoveCheckoutWarning),
) -> Result<()> {
    let path = tokio::fs::canonicalize(path)
        .await
        .with_context(|| format!("failed to canonicalize checkout path {}", path.display()))?;
    let checkout_is_active = is_active_eden_mount(&path);

    if checkout_is_active {
        remove_active_checkout(instance, &path, options, &mut warning_handler).await
    } else {
        remove_inactive_checkout(instance, &path, options).await
    }
}

fn get_test_delay() -> Option<Duration> {
    std::env::var("TEST_ONLY_AUX_PROCESSES_STOP_DELAY_SECS")
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .map(Duration::from_secs)
}

async fn unmount_redirections(instance: &EdenFsInstance, path: &Path) -> Result<()> {
    if let Some(delay) = get_test_delay() {
        debug!("Injecting test delay of {} seconds", delay.as_secs());
        tokio::time::sleep(delay).await;
    }

    let checkout = find_checkout(instance, path)
        .with_context(|| format!("failed to find checkout for {}", path.display()))?;
    let redirections = get_effective_redirections(instance, &checkout)
        .with_context(|| format!("failed to get redirections for {}", path.display()))?;

    for redirection in redirections.values() {
        redirection
            .remove_existing(instance, &checkout, false, false, "eden rm")
            .await
            .with_context(|| {
                format!(
                    "failed to unmount redirection {}",
                    redirection.repo_path().display()
                )
            })?;
    }
    Ok(())
}

async fn unmount_redirections_with_timeout<UnmountFuture, WarningHandler>(
    path: &Path,
    timeout: Duration,
    unmount: UnmountFuture,
    warning_handler: &mut WarningHandler,
) where
    UnmountFuture: Future<Output = Result<()>>,
    WarningHandler: FnMut(RemoveCheckoutWarning),
{
    match tokio::time::timeout(timeout, unmount).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => warning_handler(RemoveCheckoutWarning::RedirectionUnmountFailed {
            path: path.to_path_buf(),
            error: format!("{error:#}"),
        }),
        Err(_) => warning_handler(RemoveCheckoutWarning::RedirectionUnmountTimedOut {
            path: path.to_path_buf(),
            timeout,
        }),
    }
}

async fn remove_active_checkout<WarningHandler>(
    instance: &EdenFsInstance,
    path: &Path,
    options: RemoveCheckoutOptions,
    warning_handler: &mut WarningHandler,
) -> Result<()>
where
    WarningHandler: FnMut(RemoveCheckoutWarning),
{
    unmount_redirections_with_timeout(
        path,
        options.auxiliary_process_timeout,
        unmount_redirections(instance, path),
        warning_handler,
    )
    .await;

    instance
        .get_client()
        .unmount_for_removal(instance, path, options.no_force)
        .await
        .with_context(|| format!("failed to unmount mount point at {}", path.display()))?;
    remove_inactive_checkout(instance, path, options).await
}

async fn remove_inactive_checkout(
    instance: &EdenFsInstance,
    path: &Path,
    options: RemoveCheckoutOptions,
) -> Result<()> {
    remove_client_config_dir(instance, path)?;
    instance
        .remove_path_from_directory_map(path)
        .with_context(|| format!("failed to remove {} from config json file", path.display()))?;

    if !options.preserve_mount_point {
        clean_mount_point(path)?;
    }
    validate_removal_completion(instance, path, options.preserve_mount_point).await
}

fn remove_client_config_dir(instance: &EdenFsInstance, path: &Path) -> Result<()> {
    let client_dir = instance.client_dir_for_mount_point(path)?;
    match forcefully_remove_dir_all(&client_dir) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(anyhow!(
            "failed to remove client config directory for {}: {error}",
            path.display()
        )),
    }
}

/// Remove a directory using the same permission and retry handling as checkout removal.
pub fn clean_mount_point(path: &Path) -> Result<()> {
    #[cfg(unix)]
    std::fs::set_permissions(path, Permissions::from_mode(0o755))
        .with_context(|| format!("failed to set permission 755 for path {}", path.display()))?;

    forcefully_remove_dir_all(path)
        .with_context(|| format!("failed to remove mount point {}", path.display()))
}

async fn validate_removal_completion(
    instance: &EdenFsInstance,
    path: &Path,
    preserve_mount_point: bool,
) -> Result<()> {
    if preserve_mount_point {
        return Ok(());
    }

    if path_in_eden_config(instance, path).await? {
        return Err(anyhow!("repo {} is still mounted", path.display()));
    }

    fail_point!("remove:validate", |_| {
        Err(anyhow!("failpoint: expected failure"))
    });

    match path.try_exists() {
        Ok(false) => Ok(()),
        Ok(true) => Err(anyhow!(
            "directory left by repo {} is not removed",
            path.display()
        )),
        Err(error) => Err(anyhow!(
            "failed to check the status of path {}: {error}",
            path.display()
        )),
    }
}

/// Return whether a path is still registered in the EdenFS directory map.
pub async fn path_in_eden_config(instance: &EdenFsInstance, path: &Path) -> Result<bool> {
    let mut mounts = get_mounts(instance)
        .await
        .context("failed to call eden list")?;
    let entry_key = dunce::simplified(path);
    mounts.retain(|mount_path, _| dunce::simplified(mount_path) == entry_key);
    Ok(!mounts.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_redirection_unmount_failure() {
        let mut warnings = Vec::new();

        unmount_redirections_with_timeout(
            Path::new("/checkout"),
            Duration::from_secs(10),
            async { Err(anyhow!("device busy").context("failed to unmount buck-out")) },
            &mut |warning| warnings.push(warning),
        )
        .await;

        assert_eq!(warnings.len(), 1, "the caller must receive the failure");
        assert!(
            warnings[0]
                .to_string()
                .contains("failed to unmount buck-out: device busy"),
            "the warning must retain the underlying cause"
        );
    }
}
