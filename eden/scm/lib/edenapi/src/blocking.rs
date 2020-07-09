/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Context;
use tokio::runtime::Runtime;

use edenapi_types::{DataEntry, HistoryEntry};
use types::{HgId, Key, RepoPathBuf};

use crate::api::{EdenApi, ProgressCallback};
use crate::errors::EdenApiError;
use crate::name::RepoName;
use crate::response::{BlockingFetch, ResponseMeta};

pub trait EdenApiBlocking: EdenApi {
    fn health_blocking(&self) -> Result<ResponseMeta, EdenApiError> {
        let mut rt = Runtime::new().context("Failed to initialize Tokio runtime")?;
        rt.block_on(self.health())
    }

    fn files_blocking(
        &self,
        repo: RepoName,
        keys: Vec<Key>,
        progress: Option<ProgressCallback>,
    ) -> Result<BlockingFetch<DataEntry>, EdenApiError> {
        BlockingFetch::from_async(self.files(repo, keys, progress))
    }

    fn history_blocking(
        &self,
        repo: RepoName,
        keys: Vec<Key>,
        length: Option<u32>,
        progress: Option<ProgressCallback>,
    ) -> Result<BlockingFetch<HistoryEntry>, EdenApiError> {
        BlockingFetch::from_async(self.history(repo, keys, length, progress))
    }

    fn trees_blocking(
        &self,
        repo: RepoName,
        keys: Vec<Key>,
        progress: Option<ProgressCallback>,
    ) -> Result<BlockingFetch<DataEntry>, EdenApiError> {
        BlockingFetch::from_async(self.trees(repo, keys, progress))
    }

    fn complete_trees_blocking(
        &self,
        repo: RepoName,
        rootdir: RepoPathBuf,
        mfnodes: Vec<HgId>,
        basemfnodes: Vec<HgId>,
        depth: Option<usize>,
        progress: Option<ProgressCallback>,
    ) -> Result<BlockingFetch<DataEntry>, EdenApiError> {
        BlockingFetch::from_async(self.complete_trees(
            repo,
            rootdir,
            mfnodes,
            basemfnodes,
            depth,
            progress,
        ))
    }
}

impl<T: EdenApi + ?Sized> EdenApiBlocking for T {}
