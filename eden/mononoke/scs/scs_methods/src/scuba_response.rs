/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use faster_hex::hex_string;
use scuba_ext::MononokeScubaSampleBuilder;
use source_control::AsyncPingResponse;
use source_control::{self as thrift};

/// A trait for logging a thrift `Response` struct to scuba.
pub(crate) trait AddScubaResponse {
    fn add_scuba_response(&self, _scuba: &mut MononokeScubaSampleBuilder) {}
}

impl AddScubaResponse for bool {}

impl AddScubaResponse for i64 {}

impl AddScubaResponse for Vec<thrift::Repo> {}

impl AddScubaResponse for thrift::RepoInfo {}

impl AddScubaResponse for thrift::RepoCreateCommitResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        if let Some(id) = self.ids.get(&thrift::CommitIdentityScheme::BONSAI) {
            scuba.add("commit", id.to_string());
        }
    }
}

impl AddScubaResponse for thrift::RepoCreateStackResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        if let Some(id) = self
            .commit_ids
            .last()
            .and_then(|id| id.get(&thrift::CommitIdentityScheme::BONSAI))
        {
            scuba.add("commit", id.to_string());
        }
        scuba.add("response_commit_count", self.commit_ids.len());
    }
}

impl AddScubaResponse for thrift::RepoCreateBookmarkResponse {}

impl AddScubaResponse for thrift::RepoMoveBookmarkResponse {}

impl AddScubaResponse for thrift::RepoMultipleCommitLookupResponse {}

impl AddScubaResponse for thrift::RepoDeleteBookmarkResponse {}

impl AddScubaResponse for thrift::RepoLandStackResponse {}

impl AddScubaResponse for thrift::RepoListBookmarksResponse {}

impl AddScubaResponse for thrift::RepoResolveBookmarkResponse {}

impl AddScubaResponse for thrift::RepoResolveCommitPrefixResponse {}

impl AddScubaResponse for thrift::RepoBookmarkInfoResponse {}

impl AddScubaResponse for thrift::RepoStackInfoResponse {}

impl AddScubaResponse for thrift::RepoPrepareCommitsResponse {}

impl AddScubaResponse for thrift::RepoUploadFileContentResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("response_id", hex_string(&self.id));
    }
}

impl AddScubaResponse for thrift::CommitCompareResponse {}

impl AddScubaResponse for thrift::CommitFileDiffsResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        let non_text_files = self
            .path_diffs
            .iter()
            .filter_map(|response| match &response.diff {
                thrift::Diff::metadata_diff(metadata) => match (
                    metadata.old_file_info.file_content_type,
                    metadata.new_file_info.file_content_type,
                ) {
                    (Some(old_file), Some(new_file)) => Some(vec![old_file, new_file]),
                    (Some(old_file), _) => Some(vec![old_file]),
                    (_, Some(new_file)) => Some(vec![new_file]),
                    _ => None,
                },
                _ => None,
            })
            .flatten()
            .filter(|content_type| content_type.0 > 1) // Non-text or binary
            .count();
        // Only log if there are any actual non-textual files
        if non_text_files > 0 {
            scuba.add("non_text_files", non_text_files);
        }
    }
}

impl AddScubaResponse for thrift::CommitFindFilesResponse {}

impl AddScubaResponse for thrift::CommitFindFilesStreamResponse {}

impl AddScubaResponse for thrift::CommitFindFilesStreamItem {}

impl AddScubaResponse for thrift::CommitInfo {}

impl AddScubaResponse for thrift::CommitLookupResponse {}

impl AddScubaResponse for thrift::CommitLookupPushrebaseHistoryResponse {}

impl AddScubaResponse for thrift::CommitHistoryResponse {}

impl AddScubaResponse for thrift::CommitHgMutationHistoryResponse {}

impl AddScubaResponse for thrift::CommitLinearHistoryResponse {}

impl AddScubaResponse for thrift::CommitListDescendantBookmarksResponse {}

impl AddScubaResponse for thrift::CommitRunHooksResponse {}

impl AddScubaResponse for thrift::CommitSubtreeChangesResponse {}

impl AddScubaResponse for thrift::CommitPathBlameResponse {}

impl AddScubaResponse for thrift::CommitPathHistoryResponse {}

impl AddScubaResponse for thrift::CommitPathExistsResponse {}

impl AddScubaResponse for thrift::CommitPathInfoResponse {}

impl AddScubaResponse for thrift::CommitMultiplePathInfoResponse {}

impl AddScubaResponse for thrift::CommitPathLastChangedResponse {}

impl AddScubaResponse for thrift::CommitMultiplePathLastChangedResponse {}

impl AddScubaResponse for thrift::CommitSparseProfileDeltaResponse {}

impl AddScubaResponse for thrift::CommitSparseProfileDeltaToken {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("token", self.id.to_string());
    }
}

impl AddScubaResponse for thrift::CommitSparseProfileSizeResponse {}

impl AddScubaResponse for thrift::CommitSparseProfileSizeToken {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("token", self.id.to_string());
    }
}

impl AddScubaResponse for thrift::CommitSparseProfileDeltaPollResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        match &self {
            thrift::CommitSparseProfileDeltaPollResponse::poll_pending(_) => {
                scuba.add("response_ready", false);
            }
            thrift::CommitSparseProfileDeltaPollResponse::response(response) => {
                scuba.add("response_ready", true);
                <thrift::CommitSparseProfileDeltaResponse as AddScubaResponse>::add_scuba_response(
                    response, scuba,
                );
            }
            thrift::CommitSparseProfileDeltaPollResponse::UnknownField(_) => {}
        }
    }
}

impl AddScubaResponse for thrift::CommitSparseProfileSizePollResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        match &self {
            thrift::CommitSparseProfileSizePollResponse::poll_pending(_) => {
                scuba.add("response_ready", false);
            }
            thrift::CommitSparseProfileSizePollResponse::response(response) => {
                scuba.add("response_ready", true);
                <thrift::CommitSparseProfileSizeResponse as AddScubaResponse>::add_scuba_response(
                    response, scuba,
                );
            }
            thrift::CommitSparseProfileSizePollResponse::UnknownField(_) => {}
        }
    }
}

impl AddScubaResponse for thrift::FileChunk {}

impl AddScubaResponse for thrift::FileInfo {}

impl AddScubaResponse for thrift::FileDiffResponse {}

impl AddScubaResponse for thrift::TreeListResponse {}

impl AddScubaResponse for thrift::CreateReposResponse {}

impl AddScubaResponse for thrift::CreateReposToken {}

impl AddScubaResponse for thrift::CreateReposPollResponse {}

// TODO: report cs_ids and actual error where possible
impl AddScubaResponse for thrift::MegarepoRemergeSourceResult {}

impl AddScubaResponse for thrift::MegarepoSyncChangesetResult {}

impl AddScubaResponse for thrift::MegarepoChangeTargetConfigResult {}

impl AddScubaResponse for thrift::MegarepoAddTargetResult {}

impl AddScubaResponse for thrift::MegarepoAddBranchingTargetResult {}

impl AddScubaResponse for thrift::MegarepoAddConfigResponse {}

impl AddScubaResponse for thrift::MegarepoReadConfigResponse {}

impl AddScubaResponse for thrift::CloudWorkspaceInfoResponse {}

impl AddScubaResponse for thrift::CloudUserWorkspacesResponse {}

impl AddScubaResponse for thrift::CloudWorkspaceSmartlogResponse {}

// Helper fn to report PollResponse types
fn report_maybe_result<R: AddScubaResponse>(
    maybe_result: &Option<R>,
    scuba: &mut MononokeScubaSampleBuilder,
) {
    match maybe_result {
        None => {
            scuba.add("megarepo_ready", false);
        }
        Some(resp) => {
            scuba.add("megarepo_ready", true);
            <R as AddScubaResponse>::add_scuba_response(resp, scuba);
        }
    }
}

impl AddScubaResponse for thrift::MegarepoRemergeSourcePollResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        report_maybe_result(&self.result, scuba);
    }
}

impl AddScubaResponse for thrift::MegarepoSyncChangesetPollResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        report_maybe_result(&self.result, scuba);
    }
}

impl AddScubaResponse for thrift::MegarepoChangeTargetConfigPollResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        report_maybe_result(&self.result, scuba);
    }
}

impl AddScubaResponse for thrift::MegarepoAddTargetPollResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        report_maybe_result(&self.result, scuba);
    }
}

impl AddScubaResponse for thrift::MegarepoAddTargetToken {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("megarepo_token", self.id);
    }
}

impl AddScubaResponse for thrift::MegarepoAddBranchingTargetPollResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        report_maybe_result(&self.result, scuba);
    }
}

impl AddScubaResponse for thrift::MegarepoAddBranchingTargetToken {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("megarepo_token", self.id);
    }
}

impl AddScubaResponse for thrift::MegarepoChangeConfigToken {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("megarepo_token", self.id);
    }
}

impl AddScubaResponse for thrift::MegarepoRemergeSourceToken {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("megarepo_token", self.id);
    }
}

impl AddScubaResponse for thrift::MegarepoSyncChangesetToken {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("megarepo_token", self.id);
    }
}

// TODO(T179531912): Log responses to scuba
impl AddScubaResponse for thrift::RepoUpdateSubmoduleExpansionResponse {}

impl AddScubaResponse for thrift::RepoUploadNonBlobGitObjectResponse {}
impl AddScubaResponse for thrift::CreateGitTreeResponse {}
impl AddScubaResponse for thrift::CreateGitTagResponse {}
impl AddScubaResponse for thrift::RepoUploadPackfileBaseItemResponse {}

impl AddScubaResponse for thrift::RepoStackGitBundleStoreResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("response_bundle_handle", self.everstore_handle.as_ref());
    }
}

impl AddScubaResponse for thrift::AsyncPingToken {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("token", self.id.to_string());
    }
}

impl AddScubaResponse for thrift::AsyncPingPollResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        match &self {
            thrift::AsyncPingPollResponse::poll_pending(_) => {
                scuba.add("response_ready", false);
            }
            thrift::AsyncPingPollResponse::response(response) => {
                scuba.add("response_ready", true);
                <AsyncPingResponse as AddScubaResponse>::add_scuba_response(response, scuba);
            }
            thrift::AsyncPingPollResponse::UnknownField(_) => {}
        }
    }
}

impl AddScubaResponse for thrift::AsyncPingResponse {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("response_payload", self.payload.clone());
    }
}

// For the streaming response we log the initial response first, stream is wrapped with log_result later
impl<R, S, I> AddScubaResponse for (R, S)
where
    R: Send + Sync + AddScubaResponse,
    S: futures::Stream<Item = I>,
{
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        let (response, _stream) = self;
        response.add_scuba_response(scuba);
    }
}
