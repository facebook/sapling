/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use scuba_ext::MononokeScubaSampleBuilder;
use source_control as thrift;

use crate::commit_id::CommitIdExt;

/// A trait for logging a thrift `Response` struct to scuba.
pub(crate) trait AddScubaResponse: Send + Sync {
    fn add_scuba_response(&self, _scuba: &mut MononokeScubaSampleBuilder) {}
}

impl AddScubaResponse for bool {}

impl AddScubaResponse for Vec<thrift::Repo> {}

impl AddScubaResponse for thrift::RepoCreateCommitResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        if let Some(id) = self.ids.get(&thrift::CommitIdentityScheme::BONSAI) {
            scuba.add("commit", id.to_string());
        }
    }
}

impl AddScubaResponse for thrift::RepoCreateBookmarkResponse {}

impl AddScubaResponse for thrift::RepoMoveBookmarkResponse {}

impl AddScubaResponse for thrift::RepoDeleteBookmarkResponse {}

impl AddScubaResponse for thrift::RepoLandStackResponse {}

impl AddScubaResponse for thrift::RepoListBookmarksResponse {}

impl AddScubaResponse for thrift::RepoResolveBookmarkResponse {}

impl AddScubaResponse for thrift::RepoResolveCommitPrefixResponse {}

impl AddScubaResponse for thrift::RepoStackInfoResponse {}

impl AddScubaResponse for thrift::CommitCompareResponse {}

impl AddScubaResponse for thrift::CommitFileDiffsResponse {}

impl AddScubaResponse for thrift::CommitFindFilesResponse {}

impl AddScubaResponse for thrift::CommitInfo {}

impl AddScubaResponse for thrift::CommitLookupResponse {}

impl AddScubaResponse for thrift::CommitHistoryResponse {}

impl AddScubaResponse for thrift::CommitListDescendantBookmarksResponse {}

impl AddScubaResponse for thrift::CommitPathBlameResponse {}

impl AddScubaResponse for thrift::CommitPathHistoryResponse {}

impl AddScubaResponse for thrift::CommitPathInfoResponse {}

impl AddScubaResponse for thrift::CommitMultiplePathInfoResponse {}

impl AddScubaResponse for thrift::FileChunk {}

impl AddScubaResponse for thrift::FileInfo {}

impl AddScubaResponse for thrift::FileDiffResponse {}

impl AddScubaResponse for thrift::TreeListResponse {}

impl AddScubaResponse for thrift::RepoListHgManifestResponse {}

// MegarepoRemergeSourceToken, MegarepoChangeConfigToken
// MegarepoSyncChangesetToken are all just aliases for String
impl AddScubaResponse for String {}

// TODO: report cs_ids where possible
impl AddScubaResponse for thrift::MegarepoRemergeSourceResponse {}

impl AddScubaResponse for thrift::MegarepoSyncChangesetResponse {}

impl AddScubaResponse for thrift::MegarepoChangeTargetConfigResponse {}

impl AddScubaResponse for thrift::MegarepoAddTargetResponse {}

impl AddScubaResponse for thrift::MegarepoAddConfigResponse {}

impl AddScubaResponse for thrift::MegarepoRemergeSourcePollResponse {}

impl AddScubaResponse for thrift::MegarepoSyncChangesetPollResponse {}

impl AddScubaResponse for thrift::MegarepoChangeTargetConfigPollResponse {}

impl AddScubaResponse for thrift::MegarepoAddTargetPollResponse {}
