/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use edenfs_error::Result;

use crate::client::EdenFsClient;
use crate::instance::EdenFsInstance;

impl EdenFsClient {
    pub async fn unmount(
        &self,
        instance: &EdenFsInstance,
        path: &Path,
        no_force: bool,
    ) -> Result<()> {
        edenfs_core::unmount::unmount(instance, path, no_force).await
    }

    pub async fn unmount_for_removal(
        &self,
        instance: &EdenFsInstance,
        path: &Path,
        no_force: bool,
    ) -> Result<()> {
        edenfs_core::unmount::unmount_for_removal(instance, path, no_force).await
    }
}
