/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use anyhow::Context;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_utils::bytes_from_path;

use crate::client::EdenFsClient;

#[derive(Clone, Debug)]
pub struct CurrentSnapshotInfo {
    pub filter_id: Option<String>,
}

impl From<thrift_types::edenfs::GetCurrentSnapshotInfoResponse> for CurrentSnapshotInfo {
    fn from(from: thrift_types::edenfs::GetCurrentSnapshotInfoResponse) -> Self {
        Self {
            filter_id: from.filterId,
        }
    }
}

impl<'a> EdenFsClient<'a> {
    pub async fn get_current_snapshot_info(
        &self,
        mount_point: PathBuf,
    ) -> Result<CurrentSnapshotInfo> {
        let mount_point = bytes_from_path(mount_point)?;
        let snapshot_info_params = thrift_types::edenfs::GetCurrentSnapshotInfoRequest {
            mountId: thrift_types::edenfs::MountId {
                mountPoint: mount_point,
                ..Default::default()
            },
            cri: None,
            ..Default::default()
        };

        self.with_thrift(|thrift| thrift.getCurrentSnapshotInfo(&snapshot_info_params))
            .await
            .with_context(|| "failed to get snapshot info ")
            .map(|snapshot_info| snapshot_info.into())
            .map_err(EdenFsError::from)
    }
}
