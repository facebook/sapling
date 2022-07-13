/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::backtrace::BacktraceStatus;
use std::error::Error as StdError;

use megarepo_error::MegarepoError;
use mononoke_api::MononokeError;
use source_control as thrift;
use source_control::services::source_control_service as service;

pub(crate) enum ServiceError {
    Request(thrift::RequestError),
    Internal(thrift::InternalError),
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

#[derive(Clone, Copy)]
pub(crate) enum Status {
    RequestError,
    InternalError,
}

/// Error can be logged to SCS scuba table
pub(crate) trait LoggableError {
    fn status_and_description(&self) -> (Status, String);
}

impl LoggableError for ServiceError {
    fn status_and_description(&self) -> (Status, String) {
        match self {
            Self::Request(err) => (Status::RequestError, format!("{:?}", err)),
            Self::Internal(err) => (Status::InternalError, format!("{:?}", err)),
        }
    }
}

impl ServiceError {
    pub fn context(self, context: &str) -> Self {
        match self {
            Self::Request(thrift::RequestError { kind, reason, .. }) => {
                let reason = format!("{}: {}", context, reason);
                Self::Request(thrift::RequestError {
                    kind,
                    reason,
                    ..Default::default()
                })
            }
            Self::Internal(thrift::InternalError {
                reason,
                backtrace,
                source_chain,
                ..
            }) => {
                let reason = format!("{}: {}", context, reason);
                Self::Internal(thrift::InternalError {
                    reason,
                    backtrace,
                    source_chain,
                    ..Default::default()
                })
            }
        }
    }
}

pub(crate) trait ServiceErrorResultExt<T> {
    fn context(self, context: &str) -> Result<T, ServiceError>;
    fn with_context(self, context_fn: impl FnOnce() -> String) -> Result<T, ServiceError>;
}

impl<T, E> ServiceErrorResultExt<T> for Result<T, E>
where
    E: Into<ServiceError>,
{
    fn context(self, context: &str) -> Result<T, ServiceError> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => Err(e.into().context(context)),
        }
    }

    fn with_context(self, context_fn: impl FnOnce() -> String) -> Result<T, ServiceError> {
        match self {
            Ok(v) => Ok(v),
            Err(e) => Err(e.into().context(context_fn().as_str())),
        }
    }
}

impl From<MegarepoError> for ServiceError {
    fn from(e: MegarepoError) -> Self {
        match e {
            MegarepoError::RequestError(e) => Self::Request(thrift::RequestError {
                kind: thrift::RequestErrorKind::INVALID_REQUEST,
                reason: format!("{}", e),
                ..Default::default()
            }),
            MegarepoError::InternalError(error) => {
                let reason = error.to_string();
                let backtrace = error
                    .backtrace()
                    .and_then(|backtrace| match backtrace.status() {
                        BacktraceStatus::Captured => Some(backtrace.to_string()),
                        _ => None,
                    });
                let mut source_chain = Vec::new();
                let mut error: &dyn StdError = &error;
                while let Some(source) = error.source() {
                    source_chain.push(source.to_string());
                    error = source;
                }

                Self::Internal(thrift::InternalError {
                    reason,
                    backtrace,
                    source_chain,
                    ..Default::default()
                })
            }
        }
    }
}

impl From<MononokeError> for ServiceError {
    fn from(e: MononokeError) -> Self {
        match e {
            MononokeError::InvalidRequest(reason) => Self::Request(thrift::RequestError {
                kind: thrift::RequestErrorKind::INVALID_REQUEST,
                reason,
                ..Default::default()
            }),
            error @ MononokeError::MergeConflicts { .. } => Self::Request(thrift::RequestError {
                kind: thrift::RequestErrorKind::MERGE_CONFLICTS,
                reason: error.to_string(),
                ..Default::default()
            }),
            error @ MononokeError::ServicePermissionDenied { .. } => {
                Self::Request(thrift::RequestError {
                    kind: thrift::RequestErrorKind::PERMISSION_DENIED,
                    reason: error.to_string(),
                    ..Default::default()
                })
            }
            error @ MononokeError::AuthorizationError(_) => Self::Request(thrift::RequestError {
                kind: thrift::RequestErrorKind::PERMISSION_DENIED,
                reason: error.to_string(),
                ..Default::default()
            }),
            error @ MononokeError::NotAvailable(_) => Self::Request(thrift::RequestError {
                kind: thrift::RequestErrorKind::NOT_AVAILABLE,
                reason: error.to_string(),
                ..Default::default()
            }),
            error @ MononokeError::HookFailure(_) => Self::Request(thrift::RequestError {
                kind: thrift::RequestErrorKind::INVALID_REQUEST,
                reason: error.to_string(),
                ..Default::default()
            }),
            error @ MononokeError::PushrebaseConflicts(_) => Self::Request(thrift::RequestError {
                kind: thrift::RequestErrorKind::INVALID_REQUEST,
                reason: error.to_string(),
                ..Default::default()
            }),
            MononokeError::InternalError(error) => {
                let reason = format!("{:#}", error);
                let backtrace = error
                    .backtrace()
                    .and_then(|backtrace| match backtrace.status() {
                        BacktraceStatus::Captured => Some(backtrace.to_string()),
                        _ => None,
                    });
                let mut source_chain = Vec::new();
                let mut error: &dyn StdError = &error;
                while let Some(source) = error.source() {
                    source_chain.push(source.to_string());
                    error = source;
                }
                Self::Internal(thrift::InternalError {
                    reason,
                    backtrace,
                    source_chain,
                    ..Default::default()
                })
            }
        }
    }
}

macro_rules! impl_into_thrift_error {
    ($t:ty) => {
        impl From<ServiceError> for $t {
            fn from(e: ServiceError) -> Self {
                match e {
                    ServiceError::Request(e) => e.into(),
                    ServiceError::Internal(e) => e.into(),
                }
            }
        }
    };
}

impl_into_thrift_error!(service::ListReposExn);
impl_into_thrift_error!(service::RepoResolveBookmarkExn);
impl_into_thrift_error!(service::RepoResolveCommitPrefixExn);
impl_into_thrift_error!(service::RepoListBookmarksExn);
impl_into_thrift_error!(service::RepoCreateCommitExn);
impl_into_thrift_error!(service::RepoCreateBookmarkExn);
impl_into_thrift_error!(service::RepoMoveBookmarkExn);
impl_into_thrift_error!(service::RepoDeleteBookmarkExn);
impl_into_thrift_error!(service::RepoLandStackExn);
impl_into_thrift_error!(service::RepoBookmarkInfoExn);
impl_into_thrift_error!(service::RepoStackInfoExn);
impl_into_thrift_error!(service::CommitCommonBaseWithExn);
impl_into_thrift_error!(service::CommitFileDiffsExn);
impl_into_thrift_error!(service::CommitLookupExn);
impl_into_thrift_error!(service::CommitLookupPushrebaseHistoryExn);
impl_into_thrift_error!(service::CommitInfoExn);
impl_into_thrift_error!(service::CommitCompareExn);
impl_into_thrift_error!(service::CommitIsAncestorOfExn);
impl_into_thrift_error!(service::CommitFindFilesExn);
impl_into_thrift_error!(service::CommitHistoryExn);
impl_into_thrift_error!(service::CommitListDescendantBookmarksExn);
impl_into_thrift_error!(service::CommitRunHooksExn);
impl_into_thrift_error!(service::CommitPathExistsExn);
impl_into_thrift_error!(service::CommitPathInfoExn);
impl_into_thrift_error!(service::CommitMultiplePathInfoExn);
impl_into_thrift_error!(service::CommitPathBlameExn);
impl_into_thrift_error!(service::CommitPathHistoryExn);
impl_into_thrift_error!(service::CommitPathLastChangedExn);
impl_into_thrift_error!(service::CommitMultiplePathLastChangedExn);
impl_into_thrift_error!(service::CommitSparseProfileDeltaExn);
impl_into_thrift_error!(service::CommitSparseProfileSizeExn);
impl_into_thrift_error!(service::TreeExistsExn);
impl_into_thrift_error!(service::TreeListExn);
impl_into_thrift_error!(service::FileExistsExn);
impl_into_thrift_error!(service::FileInfoExn);
impl_into_thrift_error!(service::FileContentChunkExn);
impl_into_thrift_error!(service::FileDiffExn);
impl_into_thrift_error!(service::CommitLookupXrepoExn);
impl_into_thrift_error!(service::RepoListHgManifestExn);
impl_into_thrift_error!(service::MegarepoAddSyncTargetConfigExn);
impl_into_thrift_error!(service::MegarepoReadTargetConfigExn);
impl_into_thrift_error!(service::MegarepoAddSyncTargetExn);
impl_into_thrift_error!(service::MegarepoAddSyncTargetPollExn);
impl_into_thrift_error!(service::MegarepoAddBranchingSyncTargetExn);
impl_into_thrift_error!(service::MegarepoAddBranchingSyncTargetPollExn);
impl_into_thrift_error!(service::MegarepoChangeTargetConfigExn);
impl_into_thrift_error!(service::MegarepoChangeTargetConfigPollExn);
impl_into_thrift_error!(service::MegarepoSyncChangesetExn);
impl_into_thrift_error!(service::MegarepoSyncChangesetPollExn);
impl_into_thrift_error!(service::MegarepoRemergeSourceExn);
impl_into_thrift_error!(service::MegarepoRemergeSourcePollExn);

pub(crate) fn invalid_request(reason: impl ToString) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::INVALID_REQUEST,
        reason: reason.to_string(),
        ..Default::default()
    }
}

pub(crate) fn internal_error(error: impl ToString) -> thrift::InternalError {
    thrift::InternalError {
        reason: error.to_string(),
        backtrace: None,
        source_chain: Vec::new(),
        ..Default::default()
    }
}

pub(crate) fn repo_not_found(repo: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::REPO_NOT_FOUND,
        reason: format!("repo not found ({})", repo),
        ..Default::default()
    }
}

pub(crate) fn commit_not_found(commit: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::COMMIT_NOT_FOUND,
        reason: format!("commit not found ({})", commit),
        ..Default::default()
    }
}

pub(crate) fn file_not_found(file: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::FILE_NOT_FOUND,
        reason: format!("file not found ({})", file),
        ..Default::default()
    }
}

pub(crate) fn tree_not_found(tree: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::TREE_NOT_FOUND,
        reason: format!("tree not found ({})", tree),
        ..Default::default()
    }
}

pub(crate) fn limit_too_low(limit: usize) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::INVALID_REQUEST,
        reason: format!(
            "the limit param value of {} is not enough for the method to make any progress",
            limit,
        ),
        ..Default::default()
    }
}

pub(crate) fn diff_input_too_big(total_size: u64) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::INVALID_REQUEST_INPUT_TOO_BIG,
        reason: format!(
            "only {} bytes of files (in total) can be diffed in one request, you asked for {} bytes",
            thrift::consts::COMMIT_FILE_DIFFS_SIZE_LIMIT,
            total_size,
        ),
        ..Default::default()
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
        ..Default::default()
    }
}

pub(crate) fn not_available(reason: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::NOT_AVAILABLE,
        reason,
        ..Default::default()
    }
}
