/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use async_runtime::block_on;
use edenapi_types::{
    BookmarkEntry, CloneData, CommitHashToLocationResponse, CommitLocationToHashRequest,
    CommitLocationToHashResponse, CommitRevlogData, EdenApiServerError, FileEntry, FileSpec,
    HistoryEntry, TreeAttributes, TreeEntry,
};
use types::{HgId, Key, RepoPathBuf};

use crate::api::{EdenApi, ProgressCallback};
use crate::errors::EdenApiError;
use crate::response::{BlockingResponse, ResponseMeta};

pub trait EdenApiBlocking: EdenApi {
    fn health_blocking(&self) -> Result<ResponseMeta, EdenApiError> {
        block_on(self.health())
    }

    fn files_blocking(
        &self,
        repo: String,
        keys: Vec<Key>,
        progress: Option<ProgressCallback>,
    ) -> Result<BlockingResponse<FileEntry>, EdenApiError> {
        BlockingResponse::from_async(self.files(repo, keys, progress))
    }

    fn files_attrs_blocking(
        &self,
        repo: String,
        reqs: Vec<FileSpec>,
        progress: Option<ProgressCallback>,
    ) -> Result<BlockingResponse<FileEntry>, EdenApiError> {
        BlockingResponse::from_async(self.files_attrs(repo, reqs, progress))
    }

    fn history_blocking(
        &self,
        repo: String,
        keys: Vec<Key>,
        length: Option<u32>,
        progress: Option<ProgressCallback>,
    ) -> Result<BlockingResponse<HistoryEntry>, EdenApiError> {
        BlockingResponse::from_async(self.history(repo, keys, length, progress))
    }

    fn trees_blocking(
        &self,
        repo: String,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
        progress: Option<ProgressCallback>,
    ) -> Result<BlockingResponse<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        BlockingResponse::from_async(self.trees(repo, keys, attributes, progress))
    }

    fn complete_trees_blocking(
        &self,
        repo: String,
        rootdir: RepoPathBuf,
        mfnodes: Vec<HgId>,
        basemfnodes: Vec<HgId>,
        depth: Option<usize>,
        progress: Option<ProgressCallback>,
    ) -> Result<BlockingResponse<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        BlockingResponse::from_async(self.complete_trees(
            repo,
            rootdir,
            mfnodes,
            basemfnodes,
            depth,
            progress,
        ))
    }

    fn commit_revlog_data_blocking(
        &self,
        repo: String,
        hgids: Vec<HgId>,
        progress: Option<ProgressCallback>,
    ) -> Result<BlockingResponse<CommitRevlogData>, EdenApiError> {
        BlockingResponse::from_async(self.commit_revlog_data(repo, hgids, progress))
    }

    fn clone_data_blocking(
        &self,
        repo: String,
        progress: Option<ProgressCallback>,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        block_on(self.clone_data(repo, progress))
    }

    fn full_idmap_clone_data_blocking(
        &self,
        repo: String,
        progress: Option<ProgressCallback>,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        block_on(self.full_idmap_clone_data(repo, progress))
    }

    fn commit_location_to_hash_blocking(
        &self,
        repo: String,
        requests: Vec<CommitLocationToHashRequest>,
        progress: Option<ProgressCallback>,
    ) -> Result<BlockingResponse<CommitLocationToHashResponse>, EdenApiError> {
        BlockingResponse::from_async(self.commit_location_to_hash(repo, requests, progress))
    }

    fn commit_hash_to_location_blocking(
        &self,
        repo: String,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
        progress: Option<ProgressCallback>,
    ) -> Result<BlockingResponse<CommitHashToLocationResponse>, EdenApiError> {
        BlockingResponse::from_async(self.commit_hash_to_location(
            repo,
            master_heads,
            hgids,
            progress,
        ))
    }

    fn bookmarks_blocking(
        &self,
        repo: String,
        bookmarks: Vec<String>,
        progress: Option<ProgressCallback>,
    ) -> Result<BlockingResponse<BookmarkEntry>, EdenApiError> {
        BlockingResponse::from_async(self.bookmarks(repo, bookmarks, progress))
    }
}

impl<T: EdenApi + ?Sized> EdenApiBlocking for T {}
