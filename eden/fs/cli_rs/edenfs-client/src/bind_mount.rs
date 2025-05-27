/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![cfg(target_os = "linux")]

use std::path::Path;

use anyhow::Context;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_utils::bytes_from_path;

use crate::client::Client;
use crate::client::EdenFsClient;
use crate::methods::EdenThriftMethod;

impl EdenFsClient {
    pub async fn add_bind_mount(
        &self,
        mount_path: &Path,
        repo_path: &Path,
        target_path: &Path,
    ) -> Result<()> {
        let mount_path = bytes_from_path(mount_path.to_path_buf()).with_context(|| {
            format!(
                "Failed to get mount point '{}' as str",
                mount_path.display()
            )
        })?;

        let repo_path = bytes_from_path(repo_path.to_path_buf()).with_context(|| {
            format!("Failed to get repo point '{}' as str", repo_path.display())
        })?;

        let target_path = bytes_from_path(target_path.to_path_buf())
            .with_context(|| format!("Failed to get target '{}' as str", target_path.display()))?;

        self.with_thrift(|thrift| {
            (
                thrift.addBindMount(&mount_path, &repo_path, &target_path),
                EdenThriftMethod::AddBindMount,
            )
        })
        .await
        .with_context(|| "failed add bind mount thrift call")
        .map_err(EdenFsError::from)
    }

    pub async fn remove_bind_mount(&self, mount_path: &Path, repo_path: &Path) -> Result<()> {
        let mount_path = bytes_from_path(mount_path.to_path_buf()).with_context(|| {
            format!(
                "Failed to get mount point '{}' as str",
                mount_path.display()
            )
        })?;

        let repo_path = bytes_from_path(repo_path.to_path_buf()).with_context(|| {
            format!("Failed to get repo point '{}' as str", repo_path.display())
        })?;

        self.with_thrift(|thrift| {
            (
                thrift.removeBindMount(&mount_path, &repo_path),
                EdenThriftMethod::RemoveBindMount,
            )
        })
        .await
        .with_context(|| "failed remove bind mount thrift call")
        .map_err(EdenFsError::from)
    }
}
