/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::path::PathBuf;

use edenfs_error::impl_eden_data_into_result;
use edenfs_error::EdenDataIntoResult;
use edenfs_error::EdenFsError;
use edenfs_utils::path_from_bytes_lossy;

use crate::attributes::FileAttributeDataOrErrorV2;

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
