/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use hyper::StatusCode;

use thiserror::Error;

use lfs_protocol::{RequestObject, ResponseObject};
use mononoke_types::ContentId;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Client cancelled the request")]
    ClientCancelled,
    #[error("An error occurred forwarding the request to upstream")]
    UpstreamDidNotRespond,
    #[error("An error ocurred receiving a response from upstream ({0}): {1}")]
    UpstreamError(StatusCode, String),
    #[error("Could not serialize")]
    SerializationFailed,
    #[error("Could not initialize HTTP client")]
    HttpClientInitializationFailed,
    #[error("Could not build {0}")]
    UriBuilderFailed(&'static str),
    #[error("Invalid Uri {0}: {1}")]
    InvalidUri(String, &'static str),
    #[error("Object does not exist: {0}")]
    ObjectDoesNotExist(ContentId),
    #[error("Could not dispatch batch request to upstream")]
    UpstreamBatchNoResponse,
    #[error("Upstream batch response is invalid")]
    UpstreamBatchInvalid,
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
    #[error("Could not generate download URIs")]
    GenerateDownloadUrisError,
    #[error("Could not generate upload URIs")]
    GenerateUploadUrisError,
    #[error("Could not parse Request Batch")]
    InvalidBatch,
    #[error("Could not parse Content ID")]
    InvalidContentId,
    #[error("Could not access Filestore for reads")]
    FilestoreReadFailure,
    #[error("Could not access Filestore for writes")]
    FilestoreWriteFailure,
    #[error("Failed to create response")]
    ResponseCreationFailure,
    #[error("Throttled by counter: {0} (value: {1}, limit: {2})")]
    Throttled(String, i64, i64),
}

#[derive(Debug, Error)]
pub enum LfsServerContextErrorKind {
    #[error("Operated not permitted")]
    Forbidden,
    #[error("Repository does not exist: {0}")]
    RepositoryDoesNotExist(String),
}
