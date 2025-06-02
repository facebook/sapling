/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(dead_code)]

use std::path::Path;

use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;
use edenfs_utils::bytes_from_path;
use thrift_types::edenfs::PrefetchParams;

use crate::client::Client;
use crate::client::EdenFsClient;
use crate::glob_files::Glob;
use crate::glob_files::PredictiveFetchParams;
use crate::methods::EdenThriftMethod;

#[derive(Clone, Debug)]
pub struct PrefetchResult {
    pub prefetched_files: Option<Glob>,
}

impl From<thrift_types::edenfs::PrefetchResult> for PrefetchResult {
    fn from(from: thrift_types::edenfs::PrefetchResult) -> Self {
        Self {
            prefetched_files: from.prefetchedFiles.map(Glob::from),
        }
    }
}

impl EdenFsClient {
    pub async fn prefetch_files<P: AsRef<Path>, S: AsRef<Path>>(
        &self,
        mount_point: P,
        glob_patterns: Vec<String>,
        directories_only: bool,
        revisions: Option<&[&str]>,
        search_root: Option<S>,
        background: Option<bool>,
        predictive_glob: Option<PredictiveFetchParams>,
        return_prefetched_files: bool,
    ) -> Result<PrefetchResult> {
        let prefetch_params = PrefetchParams {
            mountPoint: bytes_from_path(mount_point.as_ref().to_path_buf())?,
            globs: glob_patterns,
            directoriesOnly: directories_only,
            revisions: revisions
                .unwrap_or_default()
                .iter()
                .map(|s| s.as_bytes().to_vec())
                .collect(),
            searchRoot: search_root
                .and_then(|sr| bytes_from_path(sr.as_ref().to_path_buf()).ok())
                .unwrap_or_default(),
            background: background.unwrap_or_default(),
            predictiveGlob: predictive_glob.map(Into::into),
            returnPrefetchedFiles: return_prefetched_files,
            ..Default::default()
        };
        self.with_thrift(|thrift| {
            (
                thrift.prefetchFilesV2(&prefetch_params),
                EdenThriftMethod::PrefetchFilesV2,
            )
        })
        .await
        .map_err(|err| {
            EdenFsError::Other(anyhow!(
                "Failed invoking prefetchFilesV2 using params='{:?}' with error={:?}'",
                prefetch_params,
                err
            ))
        })
        .map(Into::into)
    }
}
