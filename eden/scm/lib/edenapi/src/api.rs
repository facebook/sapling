/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_trait::async_trait;

use edenapi_types::{DataEntry, HistoryEntry};
use http_client::Progress;
use types::{HgId, Key, RepoPathBuf};

use crate::errors::EdenApiError;
use crate::name::RepoName;
use crate::response::{Fetch, ResponseMeta};

pub type ProgressCallback = Box<dyn FnMut(Progress) + Send + 'static>;

#[async_trait]
pub trait EdenApi: Send + Sync + 'static {
    async fn health(&self) -> Result<ResponseMeta, EdenApiError>;

    async fn files(
        &self,
        repo: RepoName,
        keys: Vec<Key>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<DataEntry>, EdenApiError>;

    async fn history(
        &self,
        repo: RepoName,
        keys: Vec<Key>,
        length: Option<u32>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<HistoryEntry>, EdenApiError>;

    async fn trees(
        &self,
        repo: RepoName,
        keys: Vec<Key>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<DataEntry>, EdenApiError>;

    async fn complete_trees(
        &self,
        repo: RepoName,
        rootdir: RepoPathBuf,
        mfnodes: Vec<HgId>,
        basemfnodes: Vec<HgId>,
        depth: Option<usize>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<DataEntry>, EdenApiError>;
}
