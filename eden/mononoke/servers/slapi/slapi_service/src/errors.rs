/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fmt::Display;

use anyhow::Error;
use edenapi_types::CODE_PERMISSION_DENIED;
use edenapi_types::CommitId;
use edenapi_types::ServerError;
use gotham_ext::error::HttpError;
use mononoke_api::ChangesetId;
use mononoke_api::MononokeError;
use thiserror::Error;
use types::HgId;
use types::Key;

/// Enum to add context to server errors.
///
/// Most of the functions in the SaplingRemoteAPI server return `anyhow::Error`
/// as their error type. The intention of `SaplingRemoteApiServiceError` is to be used
/// in conjunction with `anyhow::Context` to annotate the error with
/// the appropriate context. In that sense, this type should be used
/// to "tag" other errors instead of being returned on its own.
///
/// Conversions to `gotham_ext::error::HttpError` are intentionally not
/// provided so that HTTP handlers are forced to specify an appropriate
/// status code for the specific situation in which the error occurred.
///
/// In situations where a failure will always result in the same status
/// code (e.g., a permission check failure resulting in a 403), the code
/// should return an `HttpError` directly but should tag the underlying
/// error with an `SaplingRemoteApiServiceError` before wrapping it with `HttpError`.
#[derive(Debug, Error)]
pub enum SaplingRemoteApiServiceError {
    #[error("Client cancelled the request")]
    ClientCancelled,
    #[error("Failed to parse the request's Content-Length header")]
    InvalidContentLength,
    #[error("Repository does not exist: {0}")]
    RepoDoesNotExist(String),
    #[error("Repository not available on this server: {0}")]
    RepoNotLoaded(String),
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
    #[error("Failed to fetch file's aux data for key: {0:?}")]
    FileAuxDataFetchFailed(Key),
    #[error("Failed to fetch tree for key: {0:?}")]
    TreeFetchFailed(Key),
    #[error("Failed to fetch history for key: {0:?}")]
    HistoryFetchFailed(Key),
    #[error("Failed to fetch HgId for bookmark: {0:?}. Err: {1:#}")]
    BookmarkResolutionFailed(String, Error),
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
    #[error("CommitId not found: {0}")]
    CommitIdNotFound(CommitId),
    #[allow(dead_code)]
    #[error("Invalid Request, reason: {0}")]
    InvalidRequest(String),
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
            ManifestNotDerived(_) => HttpError::e400,
            HookFailure(_) => HttpError::e400,
            NonFastForwardMove { .. } => HttpError::e400,
            PushrebaseConflicts(_) => HttpError::e400,
            AuthorizationError(_) => HttpError::e403,
            RestrictedPathsAuthorizationError(_) => HttpError::e403,
            InternalError(_) => HttpError::e500,
            MergeConflicts { .. } => HttpError::e400,
            LargeRepoNotFound(_) => HttpError::e400,
            RedactionError { .. } => HttpError::e403,
            // The set_bookmark handler normally reports this in-band. If it
            // reaches here, the move was still already applied, so the server
            // did its job. That is not a server fault, so do not map it to a
            // 5xx.
            BookmarkMoveAlreadyProcessed => HttpError::e400,
        })(Error::from(self).context(context))
    }
}

/// The in-band error reported for a failed bookmark write or land.
///
/// Access control denials carry `CODE_PERMISSION_DENIED`, so that the client
/// can report them as such, wherever the denial sits in the error's chain of
/// causes. The message is the same either way.
pub fn write_server_error(e: &Error) -> ServerError {
    let message = format!("{e:?}");
    let denied = e.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<MononokeError>(),
            Some(
                MononokeError::AuthorizationError(_)
                    | MononokeError::ServicePermissionDenied { .. }
            )
        )
    });
    if denied {
        ServerError::new(message, CODE_PERMISSION_DENIED)
    } else {
        ServerError::generic(message)
    }
}

#[cfg(test)]
mod tests {
    use mononoke_macros::mononoke;

    use super::*;

    #[derive(Debug, Error)]
    #[error("land failed")]
    struct LandFailed(#[source] MononokeError);

    fn denial() -> Error {
        Error::from(MononokeError::AuthorizationError(
            "repo:test denied".to_string(),
        ))
    }

    #[mononoke::test]
    fn denial_has_permission_denied_code() {
        assert_eq!(write_server_error(&denial()).code, CODE_PERMISSION_DENIED);
    }

    #[mononoke::test]
    fn denial_under_context_has_permission_denied_code() {
        let e = denial()
            .context("Failed to move bookmark")
            .context("set_bookmark failed");
        let server_error = write_server_error(&e);
        assert_eq!(server_error.code, CODE_PERMISSION_DENIED);
        assert!(
            server_error.message.contains("repo:test denied"),
            "message should keep the denial: {}",
            server_error.message
        );
    }

    #[mononoke::test]
    fn denial_as_source_has_permission_denied_code() {
        let e = Error::from(LandFailed(MononokeError::AuthorizationError(
            "repo:test denied".to_string(),
        )));
        assert_eq!(write_server_error(&e).code, CODE_PERMISSION_DENIED);
    }

    #[mononoke::test]
    fn service_denial_under_context_has_permission_denied_code() {
        let e = Error::from(MononokeError::ServicePermissionDenied {
            identities: "USER:test".to_string(),
            reponame: "repo".to_string(),
            service_identity: "svc".to_string(),
        })
        .context("land_stack failed");
        assert_eq!(write_server_error(&e).code, CODE_PERMISSION_DENIED);
    }

    #[mononoke::test]
    fn other_errors_are_generic() {
        let e = Error::from(MononokeError::InvalidRequest("bad".to_string()))
            .context("set_bookmark failed");
        assert_eq!(write_server_error(&e).code, 0);
        assert_eq!(write_server_error(&anyhow::anyhow!("boom")).code, 0);
    }
}
