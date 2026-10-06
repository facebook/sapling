/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use anyhow::Result;
use anyhow::anyhow;
use edenfs_utils::is_active_eden_mount;

use crate::get_edenfs_instance;

pub fn clean_mount_point(path: &Path) -> Result<()> {
    edenfs_remove::clean_mount_point(path)
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
    edenfs_remove::path_in_eden_config(get_edenfs_instance(), path).await
}
