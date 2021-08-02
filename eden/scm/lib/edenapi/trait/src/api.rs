/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Borrow;
use std::sync::Arc;

use async_trait::async_trait;

use edenapi_types::CommitGraphEntry;
use edenapi_types::CommitKnownResponse;
use edenapi_types::{
    AnyFileContentId, AnyId, BookmarkEntry, CloneData, CommitHashToLocationResponse,
    CommitLocationToHashRequest, CommitLocationToHashResponse, CommitRevlogData,
    EdenApiServerError, EphemeralPrepareResponse, FileEntry, FileSpec, HgFilenodeData,
    HgMutationEntryContent, HistoryEntry, LookupResponse, TreeAttributes, TreeEntry,
    UploadBonsaiChangeset, UploadHgChangeset, UploadToken, UploadTokensResponse, UploadTreeEntry,
    UploadTreeResponse,
};
use http_client::Progress;
use minibytes::Bytes;
use types::{HgId, Key, RepoPathBuf};

use crate::errors::EdenApiError;
use crate::response::{Fetch, ResponseMeta};

pub type ProgressCallback = Box<dyn FnMut(Progress) + Send + 'static>;

#[async_trait]
pub trait EdenApi: Send + Sync + 'static {
    async fn health(&self) -> Result<ResponseMeta, EdenApiError>;

    async fn files(
        &self,
        repo: String,
        keys: Vec<Key>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<FileEntry>, EdenApiError>;

    async fn files_attrs(
        &self,
        repo: String,
        reqs: Vec<FileSpec>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<FileEntry>, EdenApiError>;

    async fn history(
        &self,
        repo: String,
        keys: Vec<Key>,
        length: Option<u32>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<HistoryEntry>, EdenApiError>;

    async fn trees(
        &self,
        repo: String,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<Result<TreeEntry, EdenApiServerError>>, EdenApiError>;

    async fn complete_trees(
        &self,
        repo: String,
        rootdir: RepoPathBuf,
        mfnodes: Vec<HgId>,
        basemfnodes: Vec<HgId>,
        depth: Option<usize>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<Result<TreeEntry, EdenApiServerError>>, EdenApiError>;

    async fn commit_revlog_data(
        &self,
        repo: String,
        hgids: Vec<HgId>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<CommitRevlogData>, EdenApiError>;

    async fn clone_data(
        &self,
        repo: String,
        progress: Option<ProgressCallback>,
    ) -> Result<CloneData<HgId>, EdenApiError>;

    async fn pull_fast_forward_master(
        &self,
        repo: String,
        old_master: HgId,
        new_master: HgId,
    ) -> Result<CloneData<HgId>, EdenApiError>;

    async fn full_idmap_clone_data(
        &self,
        repo: String,
        progress: Option<ProgressCallback>,
    ) -> Result<CloneData<HgId>, EdenApiError>;

    async fn commit_location_to_hash(
        &self,
        repo: String,
        requests: Vec<CommitLocationToHashRequest>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<CommitLocationToHashResponse>, EdenApiError>;

    async fn commit_hash_to_location(
        &self,
        repo: String,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<CommitHashToLocationResponse>, EdenApiError>;

    /// Return a subset of commits that are known by the server.
    /// This is similar to the "known" command in HG wireproto.
    async fn commit_known(
        &self,
        repo: String,
        hgids: Vec<HgId>,
    ) -> Result<Fetch<CommitKnownResponse>, EdenApiError>;

    /// Return part of the commit graph that are ancestors of `heads`,
    /// excluding ancestors of `common`.
    ///
    /// This is `heads % common` (or `only(heads, common)`) expressed using HG
    /// revset, or `changelog.findmissing` in HG Python code base.
    ///
    /// If any of the nodes in `heads` and `common` are unknown by the server,
    /// it should be an error.
    async fn commit_graph(
        &self,
        repo: String,
        heads: Vec<HgId>,
        common: Vec<HgId>,
    ) -> Result<Fetch<CommitGraphEntry>, EdenApiError>;

    async fn bookmarks(
        &self,
        repo: String,
        bookmarks: Vec<String>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<BookmarkEntry>, EdenApiError>;

    /// Lookup items and return signed upload tokens if an item has been uploaded
    /// Supports: file content, hg filenode, hg tree, hg changeset
    async fn lookup_batch(
        &self,
        repo: String,
        items: Vec<AnyId>,
    ) -> Result<Fetch<LookupResponse>, EdenApiError>;

    /// Upload files content
    async fn process_files_upload(
        &self,
        repo: String,
        data: Vec<(AnyFileContentId, Bytes)>,
    ) -> Result<Fetch<UploadToken>, EdenApiError>;

    /// Upload list of hg filenodes
    async fn upload_filenodes_batch(
        &self,
        repo: String,
        items: Vec<HgFilenodeData>,
    ) -> Result<Fetch<UploadTokensResponse>, EdenApiError>;

    /// Upload list of trees
    async fn upload_trees_batch(
        &self,
        repo: String,
        items: Vec<UploadTreeEntry>,
    ) -> Result<Fetch<UploadTreeResponse>, EdenApiError>;

    /// Upload list of changesets
    async fn upload_changesets(
        &self,
        repo: String,
        changesets: Vec<UploadHgChangeset>,
        mutations: Vec<HgMutationEntryContent>,
    ) -> Result<Fetch<UploadTokensResponse>, EdenApiError>;

    async fn upload_bonsai_changesets(
        &self,
        repo: String,
        changesets: Vec<UploadBonsaiChangeset>,
        mutations: Vec<HgMutationEntryContent>,
    ) -> Result<Fetch<UploadTokensResponse>, EdenApiError>;

    async fn ephemeral_prepare(
        &self,
        repo: String,
    ) -> Result<Fetch<EphemeralPrepareResponse>, EdenApiError>;
}

#[async_trait]
impl EdenApi for Arc<dyn EdenApi> {
    async fn health(&self) -> Result<ResponseMeta, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .health()
            .await
    }

    async fn files(
        &self,
        repo: String,
        keys: Vec<Key>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<FileEntry>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .files(repo, keys, progress)
            .await
    }

    async fn files_attrs(
        &self,
        repo: String,
        reqs: Vec<FileSpec>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<FileEntry>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .files_attrs(repo, reqs, progress)
            .await
    }

    async fn history(
        &self,
        repo: String,
        keys: Vec<Key>,
        length: Option<u32>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<HistoryEntry>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .history(repo, keys, length, progress)
            .await
    }

    async fn trees(
        &self,
        repo: String,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .trees(repo, keys, attributes, progress)
            .await
    }

    async fn complete_trees(
        &self,
        repo: String,
        rootdir: RepoPathBuf,
        mfnodes: Vec<HgId>,
        basemfnodes: Vec<HgId>,
        depth: Option<usize>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .complete_trees(repo, rootdir, mfnodes, basemfnodes, depth, progress)
            .await
    }

    async fn commit_revlog_data(
        &self,
        repo: String,
        hgids: Vec<HgId>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<CommitRevlogData>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .commit_revlog_data(repo, hgids, progress)
            .await
    }

    async fn clone_data(
        &self,
        repo: String,
        progress: Option<ProgressCallback>,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .clone_data(repo, progress)
            .await
    }

    async fn pull_fast_forward_master(
        &self,
        repo: String,
        old_master: HgId,
        new_master: HgId,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .pull_fast_forward_master(repo, old_master, new_master)
            .await
    }

    async fn full_idmap_clone_data(
        &self,
        repo: String,
        progress: Option<ProgressCallback>,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .full_idmap_clone_data(repo, progress)
            .await
    }

    async fn commit_location_to_hash(
        &self,
        repo: String,
        requests: Vec<CommitLocationToHashRequest>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<CommitLocationToHashResponse>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .commit_location_to_hash(repo, requests, progress)
            .await
    }

    async fn commit_hash_to_location(
        &self,
        repo: String,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<CommitHashToLocationResponse>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .commit_hash_to_location(repo, master_heads, hgids, progress)
            .await
    }

    async fn commit_known(
        &self,
        repo: String,
        hgids: Vec<HgId>,
    ) -> Result<Fetch<CommitKnownResponse>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .commit_known(repo, hgids)
            .await
    }

    async fn commit_graph(
        &self,
        repo: String,
        heads: Vec<HgId>,
        common: Vec<HgId>,
    ) -> Result<Fetch<CommitGraphEntry>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .commit_graph(repo, heads, common)
            .await
    }

    async fn bookmarks(
        &self,
        repo: String,
        bookmarks: Vec<String>,
        progress: Option<ProgressCallback>,
    ) -> Result<Fetch<BookmarkEntry>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .bookmarks(repo, bookmarks, progress)
            .await
    }

    async fn lookup_batch(
        &self,
        repo: String,
        items: Vec<AnyId>,
    ) -> Result<Fetch<LookupResponse>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .lookup_batch(repo, items)
            .await
    }

    async fn process_files_upload(
        &self,
        repo: String,
        data: Vec<(AnyFileContentId, Bytes)>,
    ) -> Result<Fetch<UploadToken>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .process_files_upload(repo, data)
            .await
    }


    async fn upload_filenodes_batch(
        &self,
        repo: String,
        items: Vec<HgFilenodeData>,
    ) -> Result<Fetch<UploadTokensResponse>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .upload_filenodes_batch(repo, items)
            .await
    }

    async fn upload_trees_batch(
        &self,
        repo: String,
        items: Vec<UploadTreeEntry>,
    ) -> Result<Fetch<UploadTreeResponse>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .upload_trees_batch(repo, items)
            .await
    }

    async fn upload_changesets(
        &self,
        repo: String,
        changesets: Vec<UploadHgChangeset>,
        mutations: Vec<HgMutationEntryContent>,
    ) -> Result<Fetch<UploadTokensResponse>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .upload_changesets(repo, changesets, mutations)
            .await
    }

    async fn upload_bonsai_changesets(
        &self,
        repo: String,
        changesets: Vec<UploadBonsaiChangeset>,
        mutations: Vec<HgMutationEntryContent>,
    ) -> Result<Fetch<UploadTokensResponse>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .upload_bonsai_changesets(repo, changesets, mutations)
            .await
    }

    async fn ephemeral_prepare(
        &self,
        repo: String,
    ) -> Result<Fetch<EphemeralPrepareResponse>, EdenApiError> {
        <Arc<dyn EdenApi> as Borrow<dyn EdenApi>>::borrow(self)
            .ephemeral_prepare(repo)
            .await
    }
}
