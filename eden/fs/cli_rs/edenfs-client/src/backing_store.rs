/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use anyhow::Context;
use anyhow::anyhow;
use edenfs_error::EdenFsError;
use edenfs_error::Result;

use crate::client::Client;
use crate::client::EdenFsClient;
use crate::methods::EdenThriftMethod;

#[derive(Debug, Clone)]
pub struct FetchedFiles {
    pub fetched_file_paths: BTreeMap<String, BTreeSet<Vec<u8>>>,
}

impl From<thrift_types::edenfs::GetFetchedFilesResult> for FetchedFiles {
    fn from(from: thrift_types::edenfs::GetFetchedFilesResult) -> Self {
        FetchedFiles {
            fetched_file_paths: from.fetchedFilePaths,
        }
    }
}

impl EdenFsClient {
    pub async fn stop_recording_backing_store_fetch(&self) -> Result<FetchedFiles> {
        self.with_thrift(|thrift| {
            (
                thrift.stopRecordingBackingStoreFetch(),
                EdenThriftMethod::StopRecordingBackingStoreFetch,
            )
        })
        .await
        .with_context(|| anyhow!("stopRecordingBackingStoreFetch thrift call failed"))
        .map(|fetched_files| fetched_files.into())
        .map_err(EdenFsError::from)
    }
    pub async fn start_recording_backing_store_fetch(&self) -> Result<()> {
        self.with_thrift(|thrift| {
            (
                thrift.startRecordingBackingStoreFetch(),
                EdenThriftMethod::StartRecordingBackingStoreFetch,
            )
        })
        .await
        .with_context(|| anyhow!("startRecordingBackingStoreFetch thrift call failed"))
        .map_err(EdenFsError::from)
    }
}
