/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use hyper::StatusCode;

use thiserror::Error;

use gotham_ext::error::HttpError;
use lfs_protocol::RequestObject;
use lfs_protocol::ResponseObject;

use filestore::FetchKey;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Client cancelled the request")]
    ClientCancelled,
    #[error("An error occurred forwarding the request to upstream")]
    UpstreamDidNotRespond,
    #[error("An error ocurred receiving a response from upstream ({0}): {1}")]
    UpstreamError(StatusCode, String),
    #[error("Could not serialize")]
    SerializationFailed(#[source] anyhow::Error),
    #[error("Could not initialize HTTP client")]
    HttpClientInitializationFailed,
    #[error("Could not build {0}: {1}")]
    UriBuilderFailed(&'static str, anyhow::Error),
    #[error("Invalid Uri {0}: {1}")]
    InvalidUri(String, &'static str),
    #[error("Host {0} is not allowlisted")]
    HostNotAllowlisted(String),
    #[error("Object does not exist: {0:?}")]
    ObjectDoesNotExist(FetchKey),
    #[error("Could not dispatch batch request to upstream")]
    UpstreamBatchNoResponse(#[source] anyhow::Error),
    #[error("Upstream batch response is invalid")]
    UpstreamBatchInvalid(#[source] anyhow::Error),
    #[error("Could not fetch upstream batch")]
    UpstreamBatchError,
    #[error("Could not perform upstream upload")]
    UpstreamUploadError,
    #[error("Upstream batch response included an invalid transfer")]
    UpstreamInvalidTransfer,
    #[error("Upstream batch response did not include requested object: {0:?}")]
    UpstreamMissingObject(RequestObject),
    #[error("Upstream batch response included an invalid object: {0:?}")]
    UpstreamInvalidObject(ResponseObject),
    #[error("Could not load local alias")]
    LocalAliasLoadError,
    #[error("Could not parse Request Batch")]
    InvalidBatch,
    #[error("Could not parse Content ID")]
    InvalidContentId,
    #[error("Could not parse SHA256")]
    InvalidOid,
    #[error("Could not access Filestore for reads")]
    FilestoreReadFailure,
    #[error("Could not access Filestore for writes")]
    FilestoreWriteFailure,
    #[error("Object size ({0}) exceeds max allowed size ({1})")]
    UploadTooLarge(u64, u64),
    #[error("Object is not internally available, and upstream is not available: {0}")]
    ObjectNotInternallyAvailableAndUpstreamUnavailable(lfs_protocol::Sha256),
    #[error("Object could not be synced from upstream: {0:?}")]
    ObjectCannotBeSynced(RequestObject),

    /// A generic error occurred, and we'd like to propagate it.
    #[error(transparent)]
    Error(anyhow::Error),
}

#[derive(Debug, Error)]
pub enum LfsServerContextErrorKind {
    #[error("Operated not permitted")]
    Forbidden,
    #[error("Client not authenticated")]
    NotAuthenticated,
    #[error("Permission check failed: {0}")]
    PermissionCheckFailed(anyhow::Error),
    #[error("Repository does not exist: {0}")]
    RepositoryDoesNotExist(String),
    #[error("Missing host header")]
    MissingHostHeader,
}

impl From<LfsServerContextErrorKind> for HttpError {
    fn from(e: LfsServerContextErrorKind) -> HttpError {
        use LfsServerContextErrorKind::*;
        match e {
            Forbidden => HttpError::e403(e),
            RepositoryDoesNotExist(_) => HttpError::e400(e),
            PermissionCheckFailed(_) => HttpError::e500(e),
            MissingHostHeader => HttpError::e400(e),
            NotAuthenticated => HttpError::e403(e),
        }
    }
}
