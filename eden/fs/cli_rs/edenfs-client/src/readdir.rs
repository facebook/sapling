/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use edenfs_error::impl_eden_data_into_result;
use edenfs_error::EdenDataIntoResult;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_utils::bytes_from_path;
use edenfs_utils::path_from_bytes_lossy;

use crate::attributes::FileAttributeDataOrErrorV2;
use crate::client::EdenFsClient;
use crate::types::attributes_as_bitmask;
use crate::types::FileAttributes;
use crate::types::SyncBehavior;

type DirListAttributeEntry = HashMap<PathBuf, FileAttributeDataOrErrorV2>;

#[derive(Debug)]
enum DirListAttributeDataOrError {
    DirListAttributeData(DirListAttributeEntry),
    Error(EdenFsError),
    UnknownField(i32),
}

impl From<thrift_types::edenfs::DirListAttributeDataOrError> for DirListAttributeDataOrError {
    fn from(from: thrift_types::edenfs::DirListAttributeDataOrError) -> Self {
        match from {
            thrift_types::edenfs::DirListAttributeDataOrError::dirListAttributeData(data) => {
                DirListAttributeDataOrError::DirListAttributeData(
                    data.into_iter()
                        .map(|e| (path_from_bytes_lossy(&e.0), e.1.into()))
                        .collect(),
                )
            }
            thrift_types::edenfs::DirListAttributeDataOrError::error(error) => {
                Self::Error(EdenFsError::ThriftRequestError(error.into()))
            }
            thrift_types::edenfs::DirListAttributeDataOrError::UnknownField(unknown) => {
                Self::UnknownField(unknown)
            }
        }
    }
}

impl_eden_data_into_result!(
    DirListAttributeDataOrError,
    DirListAttributeEntry,
    DirListAttributeData
);

#[derive(Debug)]
struct ReaddirResult {
    #[allow(dead_code)]
    pub dir_lists: Vec<DirListAttributeDataOrError>,
}

impl From<thrift_types::edenfs::ReaddirResult> for ReaddirResult {
    fn from(from: thrift_types::edenfs::ReaddirResult) -> Self {
        Self {
            dir_lists: from.dirLists.into_iter().map(Into::into).collect(),
        }
    }
}

impl EdenFsClient {
    async fn readdir<P, R>(
        &self,
        mount_path: &P,
        directory_paths: &[R],
        attributes: i64,
        sync: SyncBehavior,
    ) -> Result<ReaddirResult>
    where
        P: AsRef<Path>,
        R: AsRef<Path>,
    {
        let directory_paths: Result<Vec<Vec<u8>>> = directory_paths
            .iter()
            .map(|p| bytes_from_path(p.as_ref().to_path_buf()))
            .collect();
        let params = thrift_types::edenfs::ReaddirParams {
            mountPoint: bytes_from_path(mount_path.as_ref().to_path_buf())?,
            directoryPaths: directory_paths?,
            requestedAttributes: attributes,
            sync: sync.into(),
            ..Default::default()
        };
        tracing::trace!(
            "Issuing readdir request with the following params: {:?}",
            &params
        );
        self.with_thrift(|t| t.readdir(&params))
            .await
            .map_err(|e| EdenFsError::Other(anyhow!("failed to get readdir result: {:?}", e)))
            .map(Into::into)
    }

    #[allow(dead_code)]
    async fn readdir_with_attributes_vec<P, R>(
        &self,
        mount_path: &P,
        directory_paths: &[R],
        attributes: &[FileAttributes],
        sync: SyncBehavior,
    ) -> Result<ReaddirResult>
    where
        P: AsRef<Path>,
        R: AsRef<Path>,
    {
        let attributes = attributes_as_bitmask(attributes);
        self.readdir(mount_path, directory_paths, attributes, sync)
            .await
    }
}
