/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::backtrace::BacktraceStatus;
use std::error::Error as StdError;

use async_requests::AsyncRequestsError;
use derived_data_manager::DerivationError;
use git_types::GitError;
use megarepo_error::MegarepoError;
use mononoke_api::MononokeError;
use source_control as thrift;
use source_control_services::errors::source_control_service as service;

#[derive(Debug)]
pub enum ServiceError {
    Request(thrift::RequestError),
    Internal(thrift::InternalError),
    Overload(thrift::OverloadError),
    Poll(thrift::PollError),
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

impl From<thrift::OverloadError> for ServiceError {
    fn from(e: thrift::OverloadError) -> Self {
        Self::Overload(e)
    }
}

impl From<thrift::PollError> for ServiceError {
    fn from(e: thrift::PollError) -> Self {
        Self::Poll(e)
    }
}

#[derive(Clone, Copy)]
pub enum Status {
    RequestError,
    InternalError,
    OverloadError,
    PollError,
}

/// Error can be logged to SCS scuba table
pub trait LoggableError {
    fn status_and_description(&self) -> (Status, String);
}

impl LoggableError for ServiceError {
    fn status_and_description(&self) -> (Status, String) {
        match self {
            Self::Request(err) => (Status::RequestError, format!("{:?}", err)),
            Self::Internal(err) => (Status::InternalError, format!("{:?}", err)),
            Self::Overload(err) => (Status::OverloadError, format!("{:?}", err)),
            Self::Poll(err) => (Status::PollError, format!("{:?}", err)),
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
            Self::Overload(thrift::OverloadError { reason, .. }) => {
                let reason = format!("{}: {}", context, reason);
                Self::Overload(thrift::OverloadError {
                    reason,
                    ..Default::default()
                })
            }
            Self::Poll(thrift::PollError { reason, .. }) => {
                let reason = format!("{}: {}", context, reason);
                Self::Poll(thrift::PollError {
                    reason,
                    ..Default::default()
                })
            }
        }
    }

    pub fn repo_not_found(&self) -> bool {
        match self {
            Self::Request(thrift::RequestError {
                kind: thrift::RequestErrorKind::REPO_NOT_FOUND,
                ..
            }) => true,
            Self::Request(thrift::RequestError {
                kind: thrift::RequestErrorKind::LARGE_REPO_NOT_FOUND,
                ..
            }) => true,
            _ => false,
        }
    }
}

pub trait ServiceErrorResultExt<T> {
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
                let backtrace = match error.backtrace().status() {
                    BacktraceStatus::Captured => Some(error.backtrace().to_string()),
                    _ => None,
                };
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

impl From<AsyncRequestsError> for ServiceError {
    fn from(e: AsyncRequestsError) -> Self {
        match e {
            AsyncRequestsError::RequestError(e) => Self::Request(thrift::RequestError {
                kind: thrift::RequestErrorKind::INVALID_REQUEST,
                reason: format!("{}", e),
                ..Default::default()
            }),
            AsyncRequestsError::InternalError(error) => {
                let reason = error.to_string();
                let backtrace = match error.backtrace().status() {
                    BacktraceStatus::Captured => Some(error.backtrace().to_string()),
                    _ => None,
                };
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

impl From<ServiceError> for AsyncRequestsError {
    fn from(e: ServiceError) -> Self {
        match e {
            ServiceError::Request(e) => Self::request(e),
            ServiceError::Internal(e) => Self::internal(e),
            ServiceError::Overload(e) => Self::internal(e),
            ServiceError::Poll(e) => Self::internal(e), // FIXME
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
            error @ MononokeError::LargeRepoNotFound(_) => Self::Request(thrift::RequestError {
                kind: thrift::RequestErrorKind::LARGE_REPO_NOT_FOUND,
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
            error @ MononokeError::NonFastForwardMove { .. } => {
                Self::Request(thrift::RequestError {
                    kind: thrift::RequestErrorKind::INVALID_REQUEST,
                    reason: error.to_string(),
                    ..Default::default()
                })
            }
            error @ MononokeError::PushrebaseConflicts(_) => Self::Request(thrift::RequestError {
                kind: thrift::RequestErrorKind::INVALID_REQUEST,
                reason: error.to_string(),
                ..Default::default()
            }),
            MononokeError::InternalError(error) => {
                let reason = format!("{:#}", error);
                let backtrace = match error.backtrace().status() {
                    BacktraceStatus::Captured => Some(error.backtrace().to_string()),
                    _ => None,
                };
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

impl From<DerivationError> for ServiceError {
    fn from(e: DerivationError) -> Self {
        let mononoke_error: MononokeError = e.into();
        mononoke_error.into()
    }
}

macro_rules! impl_into_thrift_error {
    // new-style poll methods can return a Poll error
    (poll $t:ty) => {
        impl From<ServiceError> for $t {
            fn from(e: ServiceError) -> Self {
                match e {
                    ServiceError::Request(e) => e.into(),
                    ServiceError::Internal(e) => e.into(),
                    ServiceError::Overload(e) => e.into(),
                    ServiceError::Poll(e) => e.into(),
                }
            }
        }
    };

    // Old-style poll methods can't distinguish between a Poll error and an Internal error, so let's do our best.
    // This also works just fine for non-poll methods that won't be returning `ServiceError::Poll` anyway.
    ($t:ty) => {
        impl From<ServiceError> for $t {
            fn from(e: ServiceError) -> Self {
                match e {
                    ServiceError::Request(e) => e.into(),
                    ServiceError::Internal(e) => e.into(),
                    ServiceError::Overload(e) => e.into(),
                    ServiceError::Poll(e) => internal_error(format!("poll error: {}", e)).into(), // this shouldn't happen
                }
            }
        }
    };
}

impl_into_thrift_error!(service::ListReposExn);
impl_into_thrift_error!(service::RepoInfoExn);
impl_into_thrift_error!(service::RepoResolveBookmarkExn);
impl_into_thrift_error!(service::RepoResolveCommitPrefixExn);
impl_into_thrift_error!(service::RepoListBookmarksExn);
impl_into_thrift_error!(service::RepoCreateCommitExn);
impl_into_thrift_error!(service::RepoCreateStackExn);
impl_into_thrift_error!(service::RepoCreateBookmarkExn);
impl_into_thrift_error!(service::RepoMoveBookmarkExn);
impl_into_thrift_error!(service::RepoMultipleCommitLookupExn);
impl_into_thrift_error!(service::RepoDeleteBookmarkExn);
impl_into_thrift_error!(service::RepoLandStackExn);
impl_into_thrift_error!(service::RepoBookmarkInfoExn);
impl_into_thrift_error!(service::RepoStackInfoExn);
impl_into_thrift_error!(service::RepoStackGitBundleStoreExn);
impl_into_thrift_error!(service::RepoPrepareCommitsExn);
impl_into_thrift_error!(service::RepoUploadFileContentExn);
impl_into_thrift_error!(service::CommitCommonBaseWithExn);
impl_into_thrift_error!(service::CommitFileDiffsExn);
impl_into_thrift_error!(service::CommitLookupExn);
impl_into_thrift_error!(service::CommitLookupPushrebaseHistoryExn);
impl_into_thrift_error!(service::CommitInfoExn);
impl_into_thrift_error!(service::CommitGenerationExn);
impl_into_thrift_error!(service::CommitCompareExn);
impl_into_thrift_error!(service::CommitIsAncestorOfExn);
impl_into_thrift_error!(service::CommitFindFilesExn);
impl_into_thrift_error!(service::CommitFindFilesStreamExn);
impl_into_thrift_error!(service::CommitFindFilesStreamStreamExn);
impl_into_thrift_error!(service::CommitHistoryExn);
impl_into_thrift_error!(service::CommitHgMutationHistoryExn);
impl_into_thrift_error!(service::CommitLinearHistoryExn);
impl_into_thrift_error!(service::CommitListDescendantBookmarksExn);
impl_into_thrift_error!(service::CommitRunHooksExn);
impl_into_thrift_error!(service::CommitSubtreeChangesExn);
impl_into_thrift_error!(service::CommitPathExistsExn);
impl_into_thrift_error!(service::CommitPathInfoExn);
impl_into_thrift_error!(service::CommitMultiplePathInfoExn);
impl_into_thrift_error!(service::CommitPathBlameExn);
impl_into_thrift_error!(service::CommitPathHistoryExn);
impl_into_thrift_error!(service::CommitPathLastChangedExn);
impl_into_thrift_error!(service::CommitMultiplePathLastChangedExn);
impl_into_thrift_error!(service::CommitSparseProfileDeltaAsyncExn);
impl_into_thrift_error!(poll service::CommitSparseProfileDeltaPollExn);
impl_into_thrift_error!(service::CommitSparseProfileSizeAsyncExn);
impl_into_thrift_error!(poll service::CommitSparseProfileSizePollExn);
impl_into_thrift_error!(service::TreeExistsExn);
impl_into_thrift_error!(service::TreeListExn);
impl_into_thrift_error!(service::FileExistsExn);
impl_into_thrift_error!(service::FileInfoExn);
impl_into_thrift_error!(service::FileContentChunkExn);
impl_into_thrift_error!(service::FileDiffExn);
impl_into_thrift_error!(service::CommitLookupXrepoExn);
impl_into_thrift_error!(service::CreateReposExn);
impl_into_thrift_error!(service::CreateReposPollExn);
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
impl_into_thrift_error!(service::RepoUpdateSubmoduleExpansionExn);
impl_into_thrift_error!(service::RepoUploadNonBlobGitObjectExn);
impl_into_thrift_error!(service::RepoUploadPackfileBaseItemExn);
impl_into_thrift_error!(service::CreateGitTreeExn);
impl_into_thrift_error!(service::CreateGitTagExn);
impl_into_thrift_error!(service::CloudWorkspaceInfoExn);
impl_into_thrift_error!(service::CloudUserWorkspacesExn);
impl_into_thrift_error!(service::CloudWorkspaceSmartlogExn);
impl_into_thrift_error!(service::AsyncPingExn);
impl_into_thrift_error!(poll service::AsyncPingPollExn);

pub fn invalid_request(reason: impl ToString) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::INVALID_REQUEST,
        reason: reason.to_string(),
        ..Default::default()
    }
}

pub fn internal_error(error: impl ToString) -> thrift::InternalError {
    thrift::InternalError {
        reason: error.to_string(),
        backtrace: None,
        source_chain: Vec::new(),
        ..Default::default()
    }
}

pub fn repo_not_found(repo: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::REPO_NOT_FOUND,
        reason: format!("repo not found ({})", repo),
        ..Default::default()
    }
}

pub fn commit_not_found(commit: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::COMMIT_NOT_FOUND,
        reason: format!("commit not found ({})", commit),
        ..Default::default()
    }
}

pub fn file_not_found(file: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::FILE_NOT_FOUND,
        reason: format!("file not found ({})", file),
        ..Default::default()
    }
}

pub fn tree_not_found(tree: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::TREE_NOT_FOUND,
        reason: format!("tree not found ({})", tree),
        ..Default::default()
    }
}

pub fn limit_too_low(limit: usize) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::INVALID_REQUEST,
        reason: format!(
            "the limit param value of {} is not enough for the method to make any progress",
            limit,
        ),
        ..Default::default()
    }
}

pub fn diff_input_too_big(total_size: u64) -> thrift::RequestError {
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

pub fn diff_input_too_many_paths(path_count: usize) -> thrift::RequestError {
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

pub fn not_available(reason: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::NOT_AVAILABLE,
        reason,
        ..Default::default()
    }
}

#[allow(unused)]
pub fn not_implemented(reason: String) -> thrift::RequestError {
    thrift::RequestError {
        kind: thrift::RequestErrorKind::NOT_IMPLEMENTED,
        reason,
        ..Default::default()
    }
}

pub fn overloaded(reason: String) -> thrift::OverloadError {
    thrift::OverloadError {
        reason,
        ..Default::default()
    }
}

pub fn poll_error(error: impl ToString) -> thrift::PollError {
    thrift::PollError {
        reason: error.to_string(),
        ..Default::default()
    }
}

impl From<GitError> for ServiceError {
    fn from(error: GitError) -> Self {
        match error {
            // Storage failure is a internal error with system generated error message.
            // Convert it into thrift::InternalError before sending across service boundary.
            GitError::StorageFailure(reason, error) => {
                let backtrace = match error.backtrace().status() {
                    BacktraceStatus::Captured => Some(error.backtrace().to_string()),
                    _ => None,
                };
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
            // All other kind of errors associated with git operations are a result of
            // invalid / bad user input. Categorize them under INVALID_REQUEST and include
            // the actual cause in the reason string.
            _ => Self::Request(thrift::RequestError {
                kind: thrift::RequestErrorKind::INVALID_REQUEST,
                reason: error.to_string(),
                ..Default::default()
            }),
        }
    }
}
