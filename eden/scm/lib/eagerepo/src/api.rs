/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::EagerRepo;
use edenapi::types::BookmarkEntry;
use edenapi::types::CommitHashToLocationResponse;
use edenapi::types::CommitLocationToHashRequest;
use edenapi::types::CommitLocationToHashResponse;
use edenapi::types::CommitRevlogData;
use edenapi::types::FileEntry;
use edenapi::types::HgId;
use edenapi::types::HistoryEntry;
use edenapi::types::Key;
use edenapi::types::Parents;
use edenapi::types::RepoPathBuf;
use edenapi::types::TreeAttributes;
use edenapi::types::TreeEntry;
use edenapi::EdenApi;
use edenapi::EdenApiError;
use edenapi::Fetch;
use edenapi::ProgressCallback;
use edenapi::ResponseMeta;
use http::StatusCode;
use http::Version;

#[async_trait::async_trait]
impl EdenApi for EagerRepo {
    async fn health(&self) -> edenapi::Result<ResponseMeta> {
        Ok(default_response_meta())
    }

    async fn files(
        &self,
        _repo: String,
        keys: Vec<Key>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<FileEntry>> {
        todo!()
    }

    async fn history(
        &self,
        _repo: String,
        keys: Vec<Key>,
        _length: Option<u32>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<HistoryEntry>> {
        todo!()
    }

    async fn trees(
        &self,
        _repo: String,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<Result<TreeEntry, edenapi::types::EdenApiServerError>>> {
        todo!()
    }

    async fn complete_trees(
        &self,
        _repo: String,
        _rootdir: RepoPathBuf,
        _mfnodes: Vec<HgId>,
        _basemfnodes: Vec<HgId>,
        _depth: Option<usize>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<Result<TreeEntry, edenapi::types::EdenApiServerError>>> {
        todo!()
    }

    async fn commit_revlog_data(
        &self,
        _repo: String,
        hgids: Vec<HgId>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<CommitRevlogData>> {
        todo!()
    }

    async fn clone_data(
        &self,
        _repo: String,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<dag::CloneData<HgId>> {
        todo!()
    }

    async fn full_idmap_clone_data(
        &self,
        _repo: String,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<dag::CloneData<HgId>> {
        todo!()
    }

    async fn commit_location_to_hash(
        &self,
        _repo: String,
        requests: Vec<CommitLocationToHashRequest>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<CommitLocationToHashResponse>> {
        todo!()
    }

    async fn commit_hash_to_location(
        &self,
        _repo: String,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<CommitHashToLocationResponse>> {
        todo!()
    }

    async fn bookmarks(
        &self,
        _repo: String,
        bookmarks: Vec<String>,
        _progress: Option<ProgressCallback>,
    ) -> edenapi::Result<Fetch<BookmarkEntry>> {
        todo!()
    }
}

fn default_response_meta() -> ResponseMeta {
    ResponseMeta {
        version: Version::HTTP_11,
        status: StatusCode::OK,
        server: Some("EagerRepo".to_string()),
        ..Default::default()
    }
}
