/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use anyhow::Result;

use crate::get_edenfs_instance;

pub fn clean_mount_point(path: &Path) -> Result<()> {
    edenfs_remove::clean_mount_point(path)
}

pub async fn path_in_eden_config(path: &Path) -> Result<bool> {
    edenfs_remove::path_in_eden_config(get_edenfs_instance(), path).await
}
