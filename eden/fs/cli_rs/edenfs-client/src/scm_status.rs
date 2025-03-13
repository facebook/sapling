/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_utils::bytes_from_path;

use crate::client::EdenFsClient;

#[derive(Clone, Debug, Default)]
pub struct RootIdOptions {
    pub filter_id: Option<String>,
}

impl From<thrift_types::edenfs::RootIdOptions> for RootIdOptions {
    fn from(from: thrift_types::edenfs::RootIdOptions) -> Self {
        Self {
            filter_id: from.filterId,
        }
    }
}

impl From<RootIdOptions> for thrift_types::edenfs::RootIdOptions {
    fn from(from: RootIdOptions) -> thrift_types::edenfs::RootIdOptions {
        thrift_types::edenfs::RootIdOptions {
            filterId: from.filter_id,
            ..Default::default()
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScmStatusDetails {
    // If needed, we can also wrap ScmFileStatus to remove all Thrift types
    // from edenfs_client APIs.
    pub entries: BTreeMap<Vec<u8>, thrift_types::edenfs::ScmFileStatus>,
    pub errors: BTreeMap<Vec<u8>, String>,
}

impl From<thrift_types::edenfs::ScmStatus> for ScmStatusDetails {
    fn from(from: thrift_types::edenfs::ScmStatus) -> Self {
        Self {
            entries: from.entries,
            errors: from.errors,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScmStatus {
    pub status: ScmStatusDetails,
    pub version: String,
}

impl From<thrift_types::edenfs::GetScmStatusResult> for ScmStatus {
    fn from(from: thrift_types::edenfs::GetScmStatusResult) -> Self {
        Self {
            status: from.status.into(),
            version: from.version,
        }
    }
}

impl<'a> EdenFsClient<'a> {
    pub async fn get_scm_status_v2(
        &self,
        mount_point: PathBuf,
        commit_str: String,
        list_ignored: bool,
        root_id_options: Option<RootIdOptions>,
    ) -> Result<ScmStatus> {
        let get_scm_status_params = thrift_types::edenfs::GetScmStatusParams {
            mountPoint: bytes_from_path(mount_point)?,
            commit: commit_str.as_bytes().to_vec(),
            listIgnored: list_ignored,
            rootIdOptions: root_id_options.map(|r| r.into()),
            ..Default::default()
        };
        self.with_thrift(|thrift| thrift.getScmStatusV2(&get_scm_status_params))
            .await
            .map(|scm_status| scm_status.into())
            .map_err(|_| EdenFsError::Other(anyhow!("failed to get scm status v2 result")))
    }
}
