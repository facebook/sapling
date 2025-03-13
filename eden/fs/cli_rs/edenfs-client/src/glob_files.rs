/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_utils::bytes_from_path;
use thrift_thriftclients::thrift::OsDtype;
use thrift_types::edenfs::GlobParams;

use crate::client::EdenFsClient;

#[derive(Clone, Debug)]
pub struct Glob {
    pub matching_files: Vec<Vec<u8>>,
    pub dtypes: Vec<OsDtype>,
    pub origin_hashes: Vec<Vec<u8>>,
}

impl From<thrift_types::edenfs::Glob> for Glob {
    fn from(from: thrift_types::edenfs::Glob) -> Self {
        Self {
            matching_files: from.matchingFiles,
            dtypes: from.dtypes,
            origin_hashes: from.originHashes,
        }
    }
}

impl<'a> EdenFsClient<'a> {
    pub async fn glob_files<P: AsRef<Path>, S: AsRef<Path>>(
        &self,
        mount_point: P,
        globs: Vec<String>,
        include_dotfiles: bool,
        prefetch_files: bool,
        suppress_file_list: bool,
        want_dtype: bool,
        search_root: S,
        background: bool,
        list_only_files: bool,
    ) -> Result<Glob> {
        let glob_params = GlobParams {
            mountPoint: bytes_from_path(mount_point.as_ref().to_path_buf())?,
            globs,
            includeDotfiles: include_dotfiles,
            prefetchFiles: prefetch_files,
            suppressFileList: suppress_file_list,
            wantDtype: want_dtype,
            searchRoot: bytes_from_path(search_root.as_ref().to_path_buf())?,
            background,
            listOnlyFiles: list_only_files,
            ..Default::default()
        };
        self.with_client(|client| client.globFiles(&glob_params))
            .await
            .map(|glob| glob.into())
            .map_err(|_| EdenFsError::Other(anyhow!("failed to get glob files result")))
    }
}
