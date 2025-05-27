/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
#[cfg(unix)]
use std::fs::Permissions;
use std::io::ErrorKind;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use edenfs_client::checkout::get_mounts;
use edenfs_client::fsutil::forcefully_remove_dir_all;
use edenfs_utils::is_active_eden_mount;

use super::types::RemoveContext;
use crate::get_edenfs_instance;

pub fn remove_client_config_dir(context: &RemoveContext) -> Result<()> {
    let instance = get_edenfs_instance();

    match fs::remove_dir_all(instance.client_dir_for_mount_point(&context.canonical_path)?) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(()),
        Err(e) => Err(anyhow!(
            "Failed to remove client config directory for {}: {}",
            context,
            e
        )),
    }
}

pub fn remove_client_config_entry(context: &RemoveContext) -> Result<()> {
    let instance = get_edenfs_instance();

    instance
        .remove_path_from_directory_map(&context.canonical_path)
        .with_context(|| format!("Failed to remove {} from config json file", context))
}

#[cfg(unix)]
pub fn clean_mount_point(path: &Path) -> Result<()> {
    let perms = Permissions::from_mode(0o755);
    fs::set_permissions(path, perms)
        .with_context(|| format!("Failed to set permission 755 for path {}", path.display()))?;
    forcefully_remove_dir_all(path)
        .with_context(|| format!("Failed to remove mount point {}", path.display()))
}

#[cfg(windows)]
pub fn clean_mount_point(path: &Path) -> Result<()> {
    // forcefully_remove_dir_all() is simply a wrapper of remove_dir_all() which handles the retry logic.
    //
    // There is a chance that remove_dir_all() can hit the error:
    // """
    // Failed to remove mount point \\?\C:\open\repo-for-safe-remove: The provider that supports,
    // file system virtualization is temporarily unavailable. (os error 369)
    // """
    //
    // Hopefully, retrying the command will fix the issue since it's temporary.
    // But if we keep seeing this error even after retrying, we should consider implementing
    // something similar to Remove-Item(rm) cmdlet from PowerShell.
    //
    // Note: It's known that "rm -f -r" should be able to remove the repo but we should not rely
    // on it from the code.
    forcefully_remove_dir_all(path)
        .with_context(|| anyhow!("Failed to remove repo directory {}", path.display()))
}

pub async fn is_inactive_eden_mount(original_path: &Path) -> Result<bool> {
    // Check if it's a directory managed under eden
    let mut path_copy = original_path.to_path_buf();
    loop {
        if path_copy.pop() {
            if is_active_eden_mount(&path_copy) {
                let err_msg = format!(
                    "{} is not the root of checkout {}, not removing",
                    original_path.display(),
                    path_copy.display()
                );
                return Err(anyhow!(err_msg));
            } else {
                continue;
            }
        }
        break;
    }

    // Maybe it's a directory that is left after unmount
    // If so, unregister it and clean from there
    path_in_eden_config(original_path).await
}

pub async fn path_in_eden_config(path: &Path) -> Result<bool> {
    let instance = get_edenfs_instance();
    let mut mounts = get_mounts(instance)
        .await
        .with_context(|| anyhow!("Failed to call eden list"))?;
    let entry_key = dunce::simplified(path);
    mounts.retain(|mount_path_key, _| dunce::simplified(mount_path_key) == entry_key);

    Ok(!mounts.is_empty())
}
