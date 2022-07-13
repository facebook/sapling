/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Display;

use anyhow::Error;
use thiserror::Error;

use gotham_ext::error::HttpError;
use mononoke_api::ChangesetId;
use mononoke_api::MononokeError;
use types::HgId;
use types::Key;

/// Enum to add context to server errors.
///
/// Most of the functions in the EdenAPI server return `anyhow::Error`
/// as their error type. The intention of `ErrorKind` is to be used
/// in conjunction with `anyhow::Context` to annotate the error with
/// the appropriate context. In that sense, this type should be used
/// to "tag" other errors instead of being returned on its own.
///
/// Conversions to `gotham_ext::error::HttpError` are intentionally not
/// provided so that HTTP handlers are forced to specify an appropriate
/// status code for the specific situation in which the error occured.
///
/// In situations where a failure will always result in the same status
/// code (e.g., a permission check failure resulting in a 403), the code
/// should return an `HttpError` directly but should tag the underlying
/// error with an `ErrorKind` before wrapping it with `HttpError`.
#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Client cancelled the request")]
    ClientCancelled,
    #[error("Failed to parse the request's Content-Length header")]
    InvalidContentLength,
    #[error("Repository does not exist: {0}")]
    RepoDoesNotExist(String),
    #[error("Failed to load repository: {0}")]
    RepoLoadFailed(String),
    #[error("Key does not exist: {0:?}")]
    KeyDoesNotExist(Key),
    #[error("Invalid path: {}", String::from_utf8_lossy(.0))]
    InvalidPath(Vec<u8>),
    #[error("Unexpected empty path")]
    UnexpectedEmptyPath,
    #[error("Serialization failed")]
    SerializationFailed,
    #[error("Deserialization failed")]
    DeserializationFailed,
    #[error("Failed to fetch file for key: {0:?}")]
    FileFetchFailed(Key),
    #[error("Failed to fetch tree for key: {0:?}")]
    TreeFetchFailed(Key),
    #[error("Failed to fetch history for key: {0:?}")]
    HistoryFetchFailed(Key),
    #[error("Failed to fetch HgId for bookmark: {0:?}")]
    BookmarkResolutionFailed(String),
    #[error("Dag location to hash request failed")]
    CommitLocationToHashRequestFailed,
    #[error("Commit data request failed")]
    CommitRevlogDataRequestFailed,
    #[error("HgId not found: {0}")]
    HgIdNotFound(HgId),
    #[error("Failed to fetch HgId for Bonsai Changeset ID {0}")]
    BonsaiChangesetToHgIdError(ChangesetId),
    #[error(
        "Invalid file content upload token in 'upload/filenodes' request for filenode: {0}, reason: {1}"
    )]
    UploadHgFilenodeRequestInvalidToken(HgId, String),
}

/// Extension trait for converting `MononokeError`s into `HttpErrors`.
/// The variants of this error map straightforwardly onto HTTP status
/// codes in a broadly-applicable way, so this trait provides a way
/// to avoid having to pattern match on these errors. The caller must
/// still provide an appropriate context for this error which will be
/// attached to the underlying error prior to conversion.
pub trait MononokeErrorExt {
    fn into_http_error<C>(self, context: C) -> HttpError
    where
        C: Display + Send + Sync + 'static;
}

impl MononokeErrorExt for MononokeError {
    fn into_http_error<C>(self, context: C) -> HttpError
    where
        C: Display + Send + Sync + 'static,
    {
        use MononokeError::*;
        (match self {
            InvalidRequest(_) => HttpError::e400,
            ServicePermissionDenied { .. } => HttpError::e403,
            NotAvailable { .. } => HttpError::e503,
            HookFailure(_) => HttpError::e400,
            PushrebaseConflicts(_) => HttpError::e400,
            AuthorizationError(_) => HttpError::e403,
            InternalError(_) => HttpError::e500,
            MergeConflicts { .. } => HttpError::e400,
        })(Error::from(self).context(context))
    }
}
