/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use mononoke_api::MononokeError;
use source_control as thrift;
use source_control::services::source_control_service as service;

pub(crate) enum ServiceError {
    Request(thrift::RequestError),
    Internal(thrift::InternalError),
    Mononoke(MononokeError),
}

impl From<thrift::RequestError> for ServiceError {
    fn from(e: thrift::RequestError) -> Self {
        Self::Request(e)
    }
}

impl From<thrift::InternalError> for ServiceError {
    fn from(e: thrift::InternalError) -> Self {
        Self::Internal(e)
    }
}

impl From<MononokeError> for ServiceError {
    fn from(e: MononokeError) -> Self {
        Self::Mononoke(e)
    }
}

macro_rules! impl_into_thrift_error {
    ($t:ty) => {
        impl From<ServiceError> for $t {
            fn from(e: ServiceError) -> Self {
                match e {
                    ServiceError::Request(e) => e.into(),
                    ServiceError::Internal(e) => e.into(),
                    ServiceError::Mononoke(e) => e.into(),
                }
            }
        }
    };
}

impl_into_thrift_error!(service::RepoResolveBookmarkExn);
impl_into_thrift_error!(service::RepoListBookmarksExn);
impl_into_thrift_error!(service::CommitFileDiffsExn);
impl_into_thrift_error!(service::CommitLookupExn);
impl_into_thrift_error!(service::CommitInfoExn);
impl_into_thrift_error!(service::CommitCompareExn);
impl_into_thrift_error!(service::CommitIsAncestorOfExn);
impl_into_thrift_error!(service::CommitFindFilesExn);
impl_into_thrift_error!(service::CommitPathInfoExn);
impl_into_thrift_error!(service::CommitPathBlameExn);
impl_into_thrift_error!(service::TreeListExn);
impl_into_thrift_error!(service::FileExistsExn);
impl_into_thrift_error!(service::FileInfoExn);
impl_into_thrift_error!(service::FileContentChunkExn);
impl_into_thrift_error!(service::CommitLookupXrepoExn);

pub(crate) fn invalid_request(reason: impl ToString) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::INVALID_REQUEST,
        reason: reason.to_string(),
    }
}

pub(crate) fn internal_error(error: impl ToString) -> thrift::InternalError {
    thrift::InternalError {
        reason: error.to_string(),
        backtrace: None,
    }
}

pub(crate) fn repo_not_found(repo: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::REPO_NOT_FOUND,
        reason: format!("repo not found ({})", repo),
    }
}

pub(crate) fn commit_not_found(commit: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::COMMIT_NOT_FOUND,
        reason: format!("commit not found ({})", commit),
    }
}

pub(crate) fn file_not_found(file: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::FILE_NOT_FOUND,
        reason: format!("file not found ({})", file),
    }
}

pub(crate) fn tree_not_found(tree: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::TREE_NOT_FOUND,
        reason: format!("tree not found ({})", tree),
    }
}

pub(crate) fn diff_input_too_big(total_size: u64) -> thrift::RequestError {
    thrift::RequestError {
            kind: thrift::RequestErrorKind::INVALID_REQUEST_INPUT_TOO_BIG,
            reason: format!(
                "only {} bytes of files (in total) can be diffed in one request, you asked for {} bytes",
                thrift::consts::COMMIT_FILE_DIFFS_SIZE_LIMIT, total_size,
            ),
        }
}

pub(crate) fn diff_input_too_many_paths(path_count: usize) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::INVALID_REQUEST_TOO_MANY_PATHS,
        reason: format!(
            "only at most {} paths can be diffed in one request, you asked for {}",
            thrift::consts::COMMIT_FILE_DIFFS_PATH_COUNT_LIMIT,
            path_count,
        ),
    }
}
