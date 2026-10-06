/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Unmounting EdenFS checkouts through the [`EdenFsDaemon`](crate::daemon::EdenFsDaemon) handle.

use std::path::Path;

use anyhow::Context;
use edenfs_error::Result;
use tracing::warn;

use crate::instance::EdenFsInstance;

/// Unmount a checkout and mark the unmount as intentional so periodic
/// unmount recovery does not remount it.
pub async fn unmount(instance: &EdenFsInstance, path: &Path, no_force: bool) -> Result<()> {
    unmount_impl(instance, path, no_force, true).await
}

/// Unmount a checkout that is about to be removed. The intentional-unmount
/// marker is best-effort here: removal must also succeed for checkouts whose
/// client directory is missing or broken, where the marker cannot be created.
pub async fn unmount_for_removal(
    instance: &EdenFsInstance,
    path: &Path,
    no_force: bool,
) -> Result<()> {
    unmount_impl(instance, path, no_force, false).await?;
    // Without the marker, the daemon's periodic accidental-unmount recovery can
    // remount the checkout between this unmount and the deletion of the client
    // directory, leaving a mount entry with no configuration describing it.
    if let Err(e) = instance.create_intentional_unmount_flag(path) {
        warn!(
            "failed to mark the unmount of {} as intentional: {e}",
            path.display()
        );
    }
    Ok(())
}

async fn unmount_impl(
    instance: &EdenFsInstance,
    path: &Path,
    no_force: bool,
    mark_intentional_unmount: bool,
) -> Result<()> {
    instance
        .daemon()
        .unmount_with_fallback(path, !no_force)
        .await
        .with_context(|| format!("Failed to unmount {}", path.display()))?;
    if mark_intentional_unmount {
        instance.create_intentional_unmount_flag(path)?;
    }
    Ok(())
}
