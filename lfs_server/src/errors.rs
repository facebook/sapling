/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use hyper::StatusCode;

use failure_ext::Fail;

use lfs_protocol::{RequestObject, ResponseObject};
use mononoke_types::ContentId;

#[derive(Debug, Fail)]
pub enum ErrorKind {
    #[fail(display = "Client cancelled the request")]
    ClientCancelled,
    #[fail(display = "An error occurred forwarding the request to upstream")]
    UpstreamDidNotRespond,
    #[fail(
        display = "An error ocurred receiving a response from upstream ({}): {}",
        _0, _1
    )]
    UpstreamError(StatusCode, String),
    #[fail(display = "Could not serialize")]
    SerializationFailed,
    #[fail(display = "Could not initialize HTTP client")]
    HttpClientInitializationFailed,
    #[fail(display = "Repository does not exist: {}", _0)]
    RepositoryDoesNotExist(String),
    #[fail(display = "Could not build {}", _0)]
    UriBuilderFailed(&'static str),
    #[fail(display = "Invalid Uri {}: {}", _0, _1)]
    InvalidUri(String, &'static str),
    #[fail(display = "Object does not exist: {}", _0)]
    ObjectDoesNotExist(ContentId),
    #[fail(display = "Could not dispatch batch request to upstream")]
    UpstreamBatchNoResponse,
    #[fail(display = "Upstream batch response is invalid")]
    UpstreamBatchInvalid,
    #[fail(display = "Could not fetch upstream batch")]
    UpstreamBatchError,
    #[fail(display = "Could not perform upstream upload")]
    UpstreamUploadError,
    #[fail(display = "Upstream batch response included an invalid transfer")]
    UpstreamInvalidTransfer,
    #[fail(
        display = "Upstream batch response did not include requested object: {:?}",
        _0
    )]
    UpstreamMissingObject(RequestObject),
    #[fail(
        display = "Upstream batch response included an invalid object: {:?}",
        _0
    )]
    UpstreamInvalidObject(ResponseObject),
    #[fail(display = "Could not load local alias")]
    LocalAliasLoadError,
    #[fail(display = "Could not generate download URIs")]
    GenerateDownloadUrisError,
    #[fail(display = "Could not generate upload URIs")]
    GenerateUploadUrisError,
    #[fail(display = "Could not parse Request Batch")]
    InvalidBatch,
    #[fail(display = "Could not parse Content ID")]
    InvalidContentId,
    #[fail(display = "Could not access Filestore for reads")]
    FilestoreReadFailure,
    #[fail(display = "Could not access Filestore for writes")]
    FilestoreWriteFailure,
    #[fail(display = "Failed to create response")]
    ResponseCreationFailure,
}
