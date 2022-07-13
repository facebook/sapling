/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use scuba_ext::MononokeScubaSampleBuilder;
use source_control as thrift;

use crate::commit_id::CommitIdExt;
use crate::scuba_common::report_megarepo_target;
use crate::scuba_common::Reported;

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

impl AddScubaResponse for thrift::RepoBookmarkInfoResponse {}

impl AddScubaResponse for thrift::RepoStackInfoResponse {}

impl AddScubaResponse for thrift::CommitCompareResponse {}

impl AddScubaResponse for thrift::CommitFileDiffsResponse {}

impl AddScubaResponse for thrift::CommitFindFilesResponse {}

impl AddScubaResponse for thrift::CommitInfo {}

impl AddScubaResponse for thrift::CommitLookupResponse {}

impl AddScubaResponse for thrift::CommitLookupPushrebaseHistoryResponse {}

impl AddScubaResponse for thrift::CommitHistoryResponse {}

impl AddScubaResponse for thrift::CommitListDescendantBookmarksResponse {}

impl AddScubaResponse for thrift::CommitRunHooksResponse {}

impl AddScubaResponse for thrift::CommitPathBlameResponse {}

impl AddScubaResponse for thrift::CommitPathHistoryResponse {}

impl AddScubaResponse for thrift::CommitPathExistsResponse {}

impl AddScubaResponse for thrift::CommitPathInfoResponse {}

impl AddScubaResponse for thrift::CommitMultiplePathInfoResponse {}

impl AddScubaResponse for thrift::CommitPathLastChangedResponse {}

impl AddScubaResponse for thrift::CommitMultiplePathLastChangedResponse {}

impl AddScubaResponse for thrift::CommitSparseProfileDeltaResponse {}

impl AddScubaResponse for thrift::CommitSparseProfileSizeResponse {}

impl AddScubaResponse for thrift::FileChunk {}

impl AddScubaResponse for thrift::FileInfo {}

impl AddScubaResponse for thrift::FileDiffResponse {}

impl AddScubaResponse for thrift::TreeListResponse {}

impl AddScubaResponse for thrift::RepoListHgManifestResponse {}

// TODO: report cs_ids and actual error where possible
impl AddScubaResponse for thrift::MegarepoRemergeSourceResult {}

impl AddScubaResponse for thrift::MegarepoSyncChangesetResult {}

impl AddScubaResponse for thrift::MegarepoChangeTargetConfigResult {}

impl AddScubaResponse for thrift::MegarepoAddTargetResult {}

impl AddScubaResponse for thrift::MegarepoAddBranchingTargetResult {}

impl AddScubaResponse for thrift::MegarepoAddConfigResponse {}

impl AddScubaResponse for thrift::MegarepoReadConfigResponse {}

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
        report_megarepo_target(&self.target, scuba, Reported::Response);
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
        report_megarepo_target(&self.target, scuba, Reported::Response);
    }
}

impl AddScubaResponse for thrift::MegarepoChangeConfigToken {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("megarepo_token", self.id);
        report_megarepo_target(&self.target, scuba, Reported::Response);
    }
}

impl AddScubaResponse for thrift::MegarepoRemergeSourceToken {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("megarepo_token", self.id);
        report_megarepo_target(&self.target, scuba, Reported::Response);
    }
}

impl AddScubaResponse for thrift::MegarepoSyncChangesetToken {
    fn add_scuba_response(&self, scuba: &mut MononokeScubaSampleBuilder) {
        scuba.add("megarepo_token", self.id);
        report_megarepo_target(&self.target, scuba, Reported::Response);
    }
}
