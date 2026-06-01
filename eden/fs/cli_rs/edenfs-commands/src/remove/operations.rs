/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use edenfs_client::checkout::find_checkout;
use edenfs_client::redirect::get_effective_redirections;
use edenfs_utils::is_active_eden_mount;
use fail::fail_point;
use tracing::debug;
use tracing::warn;

use super::types::PathType;
use super::types::RemoveContext;
use super::utils;
use crate::get_edenfs_instance;

const AUX_PROCESSES_STOP_TIMEOUT: u64 = 60;

/// Get the timeout for stopping aux processes.
///
/// Priority: CLI arg > EDENFS_AUX_PROCESSES_TIMEOUT_SECS env var > default (60s).
pub fn get_aux_processes_stop_timeout(cli_timeout: Option<u64>) -> u64 {
    if let Some(t) = cli_timeout {
        return t;
    }
    std::env::var("EDENFS_AUX_PROCESSES_TIMEOUT_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(AUX_PROCESSES_STOP_TIMEOUT)
}

/// Get delay to inject for testing.
fn get_test_delay() -> Option<Duration> {
    std::env::var("TEST_ONLY_AUX_PROCESSES_STOP_DELAY_SECS")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// Unmount redirections for a given path before removing the checkout.
pub async fn unmount_redirections_for_path(path: &Path) -> Result<()> {
    if let Some(delay) = get_test_delay() {
        debug!("Injecting test delay of {} seconds", delay.as_secs());
        tokio::time::sleep(delay).await;
    }
    let instance = get_edenfs_instance();

    let checkout = find_checkout(instance, path)
        .with_context(|| format!("Failed to find checkout for {}", path.display()))?;

    let redirs = get_effective_redirections(instance, &checkout)
        .with_context(|| format!("Failed to get redirections for {}", path.display()))?;

    for redir in redirs.values() {
        redir
            .remove_existing(instance, &checkout, false, false, "eden rm")
            .await
            .with_context(|| {
                format!(
                    "Failed to unmount redirection {}",
                    redir.repo_path().display()
                )
            })?;
    }

    Ok(())
}

// Validate and canonicalize the given path into absolute path with the type of PathBuf.
// Then determine a type for this path.
//
// Returns a tuple of:
//   1. Canonicalized path
//   2. Type of path
pub async fn classify_path(path: &str) -> Result<(PathBuf, PathType)> {
    let path_buf = PathBuf::from(path);

    match path_buf.canonicalize() {
        Err(e) => Err(e.into()),
        Ok(canonicalized_path) => {
            let path = canonicalized_path.as_path();
            if path.is_file() {
                return Ok((canonicalized_path, PathType::RegularFile));
            }

            if !path.is_dir() {
                // This is rare, but when it happens we should warn it.
                warn!(
                    "path {} is not a file or directory, please make sure it exists and you have permission to it.",
                    path.display()
                );
                return Err(anyhow!("Not a file or directory"));
            }

            debug!("{} is determined as a directory", path.display());

            if is_active_eden_mount(path) {
                debug!(
                    "path {} is determined to be an active EdenFS mount",
                    path.display()
                );

                return Ok((canonicalized_path, PathType::ActiveEdenMount));
            }

            debug!("{} is not an active EdenFS mount", path.display());

            if utils::is_inactive_eden_mount(&canonicalized_path).await? {
                debug!(
                    "path {} is determined to be an inactive EdenFS mount",
                    path.display()
                );

                return Ok((canonicalized_path, PathType::InactiveEdenMount));
            }

            // It's a directory that is not listed inside config.json
            // We don't know how to handle it properly, so move to "Unknown" state
            // and try to handle from there with "the best efforts".
            Ok((canonicalized_path, PathType::Unknown))
        }
    }
}

pub async fn remove_active_eden_mount(context: &RemoveContext) -> Result<()> {
    let timeout = context.timeout;
    // First, unmount redirections before unmounting the checkout
    context.io.info(format!(
        "Unmounting redirections for {}...",
        context.canonical_path.display()
    ));

    match tokio::time::timeout(
        timeout,
        unmount_redirections_for_path(&context.canonical_path),
    )
    .await
    {
        Ok(Ok(())) => {
            context.io.done();
        }
        Ok(Err(e)) => {
            warn!(
                "Error unmounting redirections for {}: {}",
                context.canonical_path.display(),
                e
            );
        }
        Err(_) => {
            context.io.warn(format!(
                "Unmounting redirections for {} timed out after {} seconds. Continuing with unmount...",
                context.canonical_path.display(),
                timeout.as_secs_f64(),
            ));
        }
    }

    context
        .io
        .info(format!("Unmounting repo at {} ...", context.original_path));

    let instance = get_edenfs_instance();
    let client = instance.get_client();

    match client
        .unmount_for_removal(instance, &context.canonical_path, context.no_force)
        .await
    {
        Ok(_) => {
            context.io.done();
            remove_inactive_eden_mount(context).await
        }
        Err(e) => Err(anyhow!("Failed to unmount mount point at {context}: {e}")),
    }
}

pub async fn remove_inactive_eden_mount(context: &RemoveContext) -> Result<()> {
    context.io.info(format!(
        "Unregistering repo {} from EdenFS configs...",
        context.original_path
    ));
    utils::remove_client_config_dir(context)?;
    utils::remove_client_config_entry(context)?;

    context.io.done();

    clean_up(context).await
}

pub async fn clean_up(context: &RemoveContext) -> Result<()> {
    if context.preserve_mount_point {
        context.io.warn(format!(
            "preserve_mount_point flag is set, not removing the mount point {}!",
            context.original_path
        ));
        Ok(())
    } else {
        context.io.info(format!(
            "Cleaning up the directory {} ...",
            context.original_path
        ));
        utils::clean_mount_point(&context.canonical_path)
            .with_context(|| anyhow!("Failed to clean mount point {context}"))?;
        context.io.done();

        validate_removal_completion(context).await
    }
}

async fn validate_removal_completion(context: &RemoveContext) -> Result<()> {
    context
        .io
        .info("Checking eden mount list and file system to verify the removal...".to_string());
    // check eden list
    if utils::path_in_eden_config(context.canonical_path.as_path()).await? {
        return Err(anyhow!("Repo {context} is still mounted"));
    }

    fail_point!("remove:validate", |_| {
        Err(anyhow!("failpoint: expected failure"))
    });

    // check directory clean up
    if !context.preserve_mount_point {
        match context.canonical_path.try_exists() {
            Ok(false) => {
                context.io.done();
                Ok(())
            }
            Ok(true) => Err(anyhow!("Directory left by repo {context} is not removed")),
            Err(e) => Err(anyhow!("Failed to check the status of path {context}: {e}")),
        }
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use anyhow::Context;
    use tempfile::TempDir;
    use tempfile::tempdir;

    use super::*;

    /// This helper function creates a directory structure that looks like this:
    /// "some_tmp_dir/test/nested/inner"
    /// then it returns the path to the "some_tmp_dir" directory
    fn prepare_directory() -> TempDir {
        let temp_dir = tempdir().context("couldn't create temp dir").unwrap();
        let path = temp_dir.path().join("test").join("nested").join("inner");
        let prefix = path.parent().unwrap();
        println!("creating dirs: {:?}", prefix.to_str().unwrap());
        std::fs::create_dir_all(prefix).unwrap();
        temp_dir
    }

    #[tokio::test]
    async fn test_classify_path_regular_file() {
        let temp_dir = prepare_directory();
        let file_path_buf = temp_dir.path().join("temporary-file.txt");
        fs::write(file_path_buf.as_path(), "anything").unwrap_or_else(|err| {
            panic!(
                "cannot write to a file at {}: {}",
                file_path_buf.display(),
                err
            )
        });

        let result = classify_path(file_path_buf.to_str().unwrap()).await;
        assert!(
            result.is_ok(),
            "path of a regular file should be classified"
        );
        let (p, t) = result.unwrap();
        assert!(
            p == file_path_buf.canonicalize().unwrap(),
            "path of a regular file should be canonicalized"
        );
        assert!(
            matches!(t, PathType::RegularFile),
            "path of a regular file should be classified as RegFile"
        );
    }

    #[tokio::test]
    async fn test_classify_nonexistent_path() {
        let tmp_dir = prepare_directory();
        let path = format!("{}/test/no_file", tmp_dir.path().to_str().unwrap());
        let path_buf = PathBuf::from(path);
        let result = classify_path(path_buf.to_str().unwrap()).await;
        assert!(
            result.is_err(),
            "nonexistent path should not be canonicalized"
        );
    }
}
