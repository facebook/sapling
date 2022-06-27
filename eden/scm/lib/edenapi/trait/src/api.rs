/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::time::Duration;

use async_trait::async_trait;
use edenapi_types::AnyFileContentId;
use edenapi_types::AnyId;
use edenapi_types::BonsaiChangesetContent;
use edenapi_types::BookmarkEntry;
use edenapi_types::CloneData;
use edenapi_types::CommitGraphEntry;
use edenapi_types::CommitHashLookupResponse;
use edenapi_types::CommitHashToLocationResponse;
use edenapi_types::CommitId;
use edenapi_types::CommitIdScheme;
use edenapi_types::CommitKnownResponse;
use edenapi_types::CommitLocationToHashRequest;
use edenapi_types::CommitLocationToHashResponse;
use edenapi_types::CommitMutationsResponse;
use edenapi_types::CommitRevlogData;
use edenapi_types::CommitTranslateIdResponse;
use edenapi_types::EdenApiServerError;
use edenapi_types::EphemeralPrepareResponse;
use edenapi_types::FetchSnapshotRequest;
use edenapi_types::FetchSnapshotResponse;
use edenapi_types::FileResponse;
use edenapi_types::FileSpec;
use edenapi_types::HgFilenodeData;
use edenapi_types::HgMutationEntryContent;
use edenapi_types::HistoryEntry;
use edenapi_types::LandStackResponse;
use edenapi_types::LookupResponse;
use edenapi_types::TreeAttributes;
use edenapi_types::TreeEntry;
use edenapi_types::UploadHgChangeset;
use edenapi_types::UploadToken;
use edenapi_types::UploadTokensResponse;
use edenapi_types::UploadTreeEntry;
use edenapi_types::UploadTreeResponse;
use minibytes::Bytes;
use types::HgId;
use types::Key;

use crate::errors::EdenApiError;
use crate::response::Response;
use crate::response::ResponseMeta;

#[async_trait]
pub trait EdenApi: Send + Sync + 'static {
    async fn health(&self) -> Result<ResponseMeta, EdenApiError> {
        Err(EdenApiError::NotSupported)
    }

    async fn capabilities(&self) -> Result<Vec<String>, EdenApiError> {
        Err(EdenApiError::NotSupported)
    }

    async fn files(&self, keys: Vec<Key>) -> Result<Response<FileResponse>, EdenApiError> {
        let _ = keys;
        Err(EdenApiError::NotSupported)
    }

    async fn files_attrs(
        &self,
        reqs: Vec<FileSpec>,
    ) -> Result<Response<FileResponse>, EdenApiError> {
        let _ = reqs;
        Err(EdenApiError::NotSupported)
    }

    async fn history(
        &self,
        keys: Vec<Key>,
        length: Option<u32>,
    ) -> Result<Response<HistoryEntry>, EdenApiError> {
        let _ = (keys, length);
        Err(EdenApiError::NotSupported)
    }

    async fn trees(
        &self,
        keys: Vec<Key>,
        attributes: Option<TreeAttributes>,
    ) -> Result<Response<Result<TreeEntry, EdenApiServerError>>, EdenApiError> {
        let _ = (keys, attributes);
        Err(EdenApiError::NotSupported)
    }

    async fn commit_revlog_data(
        &self,
        hgids: Vec<HgId>,
    ) -> Result<Response<CommitRevlogData>, EdenApiError> {
        let _ = hgids;
        Err(EdenApiError::NotSupported)
    }

    async fn clone_data(&self) -> Result<CloneData<HgId>, EdenApiError> {
        Err(EdenApiError::NotSupported)
    }

    /// Provide data to complete a lazy pull. `common` defines known heads,
    /// `missing` defines unknown heads.
    async fn pull_lazy(
        &self,
        common: Vec<HgId>,
        missing: Vec<HgId>,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        let _ = (common, missing);
        Err(EdenApiError::NotSupported)
    }

    async fn pull_fast_forward_master(
        &self,
        old_master: HgId,
        new_master: HgId,
    ) -> Result<CloneData<HgId>, EdenApiError> {
        self.pull_lazy(vec![old_master], vec![new_master]).await
    }

    async fn commit_location_to_hash(
        &self,
        requests: Vec<CommitLocationToHashRequest>,
    ) -> Result<Vec<CommitLocationToHashResponse>, EdenApiError> {
        let _ = requests;
        Err(EdenApiError::NotSupported)
    }

    async fn commit_hash_to_location(
        &self,
        master_heads: Vec<HgId>,
        hgids: Vec<HgId>,
    ) -> Result<Vec<CommitHashToLocationResponse>, EdenApiError> {
        let _ = (master_heads, hgids);
        Err(EdenApiError::NotSupported)
    }

    /// Return a subset of commits that are known by the server.
    /// This is similar to the "known" command in HG wireproto.
    async fn commit_known(
        &self,
        hgids: Vec<HgId>,
    ) -> Result<Vec<CommitKnownResponse>, EdenApiError> {
        let _ = hgids;
        Err(EdenApiError::NotSupported)
    }

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
        heads: Vec<HgId>,
        common: Vec<HgId>,
    ) -> Result<Vec<CommitGraphEntry>, EdenApiError> {
        let _ = (heads, common);
        Err(EdenApiError::NotSupported)
    }

    /// Return matching full hashes of hex hash prefix
    async fn hash_prefixes_lookup(
        &self,
        prefixes: Vec<String>,
    ) -> Result<Vec<CommitHashLookupResponse>, EdenApiError> {
        let _ = prefixes;
        Err(EdenApiError::NotSupported)
    }

    async fn bookmarks(&self, bookmarks: Vec<String>) -> Result<Vec<BookmarkEntry>, EdenApiError> {
        let _ = bookmarks;
        Err(EdenApiError::NotSupported)
    }

    /// Create, delete, or move a bookmark
    async fn set_bookmark(
        &self,
        bookmark: String,
        to: Option<HgId>,
        from: Option<HgId>,
        pushvars: HashMap<String, String>,
    ) -> Result<(), EdenApiError> {
        let _ = (bookmark, to, from, pushvars);
        Err(EdenApiError::NotSupported)
    }

    /// Land a stack of commits, rebasing them onto the specified bookmark
    /// and updating the bookmark to the top of the rebased stack.
    async fn land_stack(
        &self,
        bookmark: String,
        head: HgId,
        base: HgId,
        pushvars: HashMap<String, String>,
    ) -> Result<LandStackResponse, EdenApiError> {
        let _ = (bookmark, head, base, pushvars);
        Err(EdenApiError::NotSupported)
    }

    /// Lookup items and return signed upload tokens if an item has been uploaded
    /// Supports: file content, hg filenode, hg tree, hg changeset
    async fn lookup_batch(
        &self,
        items: Vec<AnyId>,
        bubble_id: Option<NonZeroU64>,
        copy_from_bubble_id: Option<NonZeroU64>,
    ) -> Result<Vec<LookupResponse>, EdenApiError> {
        let _ = (items, bubble_id, copy_from_bubble_id);
        Err(EdenApiError::NotSupported)
    }

    /// Upload files content
    async fn process_files_upload(
        &self,
        data: Vec<(AnyFileContentId, Bytes)>,
        bubble_id: Option<NonZeroU64>,
        copy_from_bubble_id: Option<NonZeroU64>,
    ) -> Result<Response<UploadToken>, EdenApiError> {
        let _ = (data, bubble_id, copy_from_bubble_id);
        Err(EdenApiError::NotSupported)
    }

    /// Upload list of hg filenodes
    async fn upload_filenodes_batch(
        &self,
        items: Vec<HgFilenodeData>,
    ) -> Result<Response<UploadTokensResponse>, EdenApiError> {
        let _ = items;
        Err(EdenApiError::NotSupported)
    }

    /// Upload list of trees
    async fn upload_trees_batch(
        &self,
        items: Vec<UploadTreeEntry>,
    ) -> Result<Response<UploadTreeResponse>, EdenApiError> {
        let _ = items;
        Err(EdenApiError::NotSupported)
    }

    /// Upload list of changesets
    async fn upload_changesets(
        &self,
        changesets: Vec<UploadHgChangeset>,
        mutations: Vec<HgMutationEntryContent>,
    ) -> Result<Response<UploadTokensResponse>, EdenApiError> {
        let _ = (changesets, mutations);
        Err(EdenApiError::NotSupported)
    }

    async fn upload_bonsai_changeset(
        &self,
        changeset: BonsaiChangesetContent,
        bubble_id: Option<NonZeroU64>,
    ) -> Result<Response<UploadTokensResponse>, EdenApiError> {
        let _ = (changeset, bubble_id);
        Err(EdenApiError::NotSupported)
    }

    async fn ephemeral_prepare(
        &self,
        custom_duration: Option<Duration>,
    ) -> Result<Response<EphemeralPrepareResponse>, EdenApiError> {
        let _ = custom_duration;
        Err(EdenApiError::NotSupported)
    }

    /// Fetch information about the requested snapshot
    async fn fetch_snapshot(
        &self,
        request: FetchSnapshotRequest,
    ) -> Result<Response<FetchSnapshotResponse>, EdenApiError> {
        let _ = request;
        Err(EdenApiError::NotSupported)
    }

    /// Download single file from upload token
    async fn download_file(&self, token: UploadToken) -> Result<Bytes, EdenApiError> {
        let _ = token;
        Err(EdenApiError::NotSupported)
    }

    /// Download mutation info related to given commits
    async fn commit_mutations(
        &self,
        commits: Vec<HgId>,
    ) -> Result<Vec<CommitMutationsResponse>, EdenApiError> {
        let _ = commits;
        Err(EdenApiError::NotSupported)
    }

    /// Translate commit IDs to a different commit ID scheme
    async fn commit_translate_id(
        &self,
        commits: Vec<CommitId>,
        scheme: CommitIdScheme,
    ) -> Result<Response<CommitTranslateIdResponse>, EdenApiError> {
        let _ = (commits, scheme);
        Err(EdenApiError::NotSupported)
    }
}
