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

use crate::api::EdenApi;
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
    ) -> Result<BlockingResponse<FileEntry>, EdenApiError> {
        BlockingResponse::from_async(self.files(repo, keys))
    }

    fn files_attrs_blocking(
        &self,
        repo: String,
        reqs: Vec<FileSpec>,
    ) -> Result<BlockingResponse<FileEntry>, EdenApiError> {
        BlockingResponse::from_async(self.files_attrs(repo, reqs))
    }

    fn history_blocking(
        &self,
        repo: String,
        keys: Vec<Key>,
        length: Option<u32>,
    ) -> Result<BlockingResponse<HistoryEntry>, EdenApiError> {
        BlockingResponse::from_async(self.history(repo, keys, length))
    }

    fn trees_blocking(
        &self,
        repo: String,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
    ) -> Result<BlockingResponse<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        BlockingResponse::from_async(self.trees(repo, keys, attributes))
    }

    fn complete_trees_blocking(
        &self,
        repo: String,
        rootdir: RepoPathBuf,
        mfnodes: Vec<HgId>,
        basemfnodes: Vec<HgId>,
        depth: Option<usize>,
    ) -> Result<BlockingResponse<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        BlockingResponse::from_async(self.complete_trees(
            repo,
            rootdir,
            mfnodes,
            basemfnodes,
            depth,
        ))
    }

    fn commit_revlog_data_blocking(
        &self,
        repo: String,
        hgids: Vec<HgId>,
    ) -> Result<BlockingResponse<CommitRevlogData>, EdenApiError> {
        BlockingResponse::from_async(self.commit_revlog_data(repo, hgids))
    }

    fn clone_data_blocking(&self, repo: String) -> Result<CloneData<HgId>, EdenApiError> {
        block_on(self.clone_data(repo))
    }

    fn full_idmap_clone_data_blocking(
        &self,
        repo: String,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        block_on(self.full_idmap_clone_data(repo))
    }

    fn commit_location_to_hash_blocking(
        &self,
        repo: String,
        requests: Vec<CommitLocationToHashRequest>,
    ) -> Result<BlockingResponse<CommitLocationToHashResponse>, EdenApiError> {
        BlockingResponse::from_async(self.commit_location_to_hash(repo, requests))
    }

    fn commit_hash_to_location_blocking(
        &self,
        repo: String,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
    ) -> Result<BlockingResponse<CommitHashToLocationResponse>, EdenApiError> {
        BlockingResponse::from_async(self.commit_hash_to_location(repo, master_heads, hgids))
    }

    fn bookmarks_blocking(
        &self,
        repo: String,
        bookmarks: Vec<String>,
    ) -> Result<BlockingResponse<BookmarkEntry>, EdenApiError> {
        BlockingResponse::from_async(self.bookmarks(repo, bookmarks))
    }
}

impl<T: EdenApi + ?Sized> EdenApiBlocking for T {}
