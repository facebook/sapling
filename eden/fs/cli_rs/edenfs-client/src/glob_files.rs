/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_utils::bytes_from_path;
use edenfs_utils::path_from_bytes;
use serde::ser::Serialize;
use serde::ser::SerializeStruct;
use serde::ser::Serializer;
use thrift_types::edenfs::GlobParams;
use thrift_types::edenfs::OsDtype;

use crate::client::Client;
use crate::client::EdenFsClient;
use crate::methods::EdenThriftMethod;
use crate::types::OSName;
use crate::types::SyncBehavior;

pub fn dtype_to_str(dtype: &OsDtype) -> &str {
    match dtype {
        0 => "Unknown",
        1 => "Fifo",
        2 => "Char",
        4 => "Dir",
        6 => "Block",
        8 => "Regular",
        10 => "Symlink",
        12 => "Socket",
        14 => "Whiteout",
        _ => "Unknown",
    }
}

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

impl Serialize for Glob {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("Glob", 3)?;
        let mut matching_files = Vec::new();
        for matching_file in &self.matching_files {
            matching_files.push(path_from_bytes(matching_file).map_err(serde::ser::Error::custom)?);
        }
        let dtypes = self.dtypes.iter().map(dtype_to_str).collect::<Vec<&str>>();
        let origin_hashes = self
            .origin_hashes
            .iter()
            .map(hex::encode)
            .collect::<Vec<String>>();
        state.serialize_field("matching_files", &matching_files)?;
        // NOTE: this should be called dtypes, but to maintain compat with the previous python code we are using dtype instead
        state.serialize_field("dtype", &dtypes)?;
        state.serialize_field("origin_hashes", &origin_hashes)?;
        state.end()
    }
}

pub struct PredictiveFetchParams {
    pub num_top_directories: Option<i32>,
    pub user: Option<String>,
    pub repo: Option<String>,
    pub os: Option<OSName>,
    pub start_time: Option<i64>,
    pub end_time: Option<i64>,
}

impl From<PredictiveFetchParams> for thrift_types::edenfs::PredictiveFetch {
    fn from(from: PredictiveFetchParams) -> Self {
        Self {
            numTopDirectories: from.num_top_directories,
            user: from.user,
            repo: from.repo,
            os: from.os.map(|os| os.to_string()),
            startTime: from.start_time,
            endTime: from.end_time,
            ..Default::default()
        }
    }
}

impl EdenFsClient {
    async fn glob_files_optional<P: AsRef<Path>, S: AsRef<Path>>(
        &self,
        mount_point: P,
        glob_patterns: Vec<String>,
        include_dotfiles: Option<bool>,
        prefetch_files: Option<bool>,
        suppress_file_list: Option<bool>,
        want_dtype: Option<bool>,
        revisions: Option<&[&str]>,
        prefetch_metadata: Option<bool>,
        search_root: Option<S>,
        background: Option<bool>,
        predictive_glob: Option<PredictiveFetchParams>,
        list_only_files: Option<bool>,
        sync: Option<SyncBehavior>,
    ) -> Result<Glob> {
        let glob_params = GlobParams {
            mountPoint: bytes_from_path(mount_point.as_ref().to_path_buf())?,
            globs: glob_patterns,
            includeDotfiles: include_dotfiles.unwrap_or_default(),
            prefetchFiles: prefetch_files.unwrap_or_default(),
            suppressFileList: suppress_file_list.unwrap_or_default(),
            wantDtype: want_dtype.unwrap_or_default(),
            revisions: revisions
                .unwrap_or_default()
                .iter()
                .map(|s| s.as_bytes().to_vec())
                .collect(),
            prefetchMetadata: prefetch_metadata.unwrap_or_default(),
            searchRoot: search_root
                .and_then(|sr| bytes_from_path(sr.as_ref().to_path_buf()).ok())
                .unwrap_or_default(),
            background: background.unwrap_or_default(),
            predictiveGlob: predictive_glob.map(Into::into),
            listOnlyFiles: list_only_files.unwrap_or_default(),
            sync: sync.map(Into::into).unwrap_or_default(),
            ..Default::default()
        };
        self.with_thrift(|thrift| (thrift.globFiles(&glob_params), EdenThriftMethod::GlobFiles))
            .await
            .map_err(|err| {
                EdenFsError::Other(anyhow!(
                    "Failed invoking globFiles using params='{:?}' with error={:?}'",
                    glob_params,
                    err
                ))
            })
            .map(Into::into)
    }

    pub async fn glob_files<P: AsRef<Path>, S: AsRef<Path>>(
        &self,
        mount_point: P,
        glob_patterns: Vec<String>,
        include_dotfiles: bool,
        prefetch_files: bool,
        suppress_file_list: bool,
        want_dtype: bool,
        search_root: S,
        background: bool,
        list_only_files: bool,
    ) -> Result<Glob> {
        self.glob_files_optional(
            mount_point,
            glob_patterns,
            Some(include_dotfiles),
            Some(prefetch_files),
            Some(suppress_file_list),
            Some(want_dtype),
            None,
            None,
            Some(search_root),
            Some(background),
            None,
            Some(list_only_files),
            None,
        )
        .await
    }

    pub async fn glob_files_foreground(
        &self,
        mount_point: &Path,
        glob_patterns: Vec<String>,
    ) -> Result<Glob> {
        let glob_list = glob_patterns.join(", ");
        tracing::trace!("resolving globs ({}) in foreground", &glob_list);
        self.glob_files_optional(
            mount_point,
            glob_patterns,
            None,
            None,
            None,
            None,
            None,
            None,
            None::<PathBuf>,
            Some(false),
            None,
            None,
            None,
        )
        .await
    }
}
