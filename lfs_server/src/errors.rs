// Copyright (c) 2018-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use hyper::StatusCode;

use failure_ext::Fail;

use mononoke_types::ContentId;
use std::collections::HashMap;

use crate::protocol::{ObjectAction, Operation, RequestObject, ResponseObject};

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
    #[fail(
        display = "Upstream batch response did not include expected actions for {:?}: {:?}",
        _0, _1
    )]
    UpstreamBatchNoActions(RequestObject, HashMap<Operation, ObjectAction>),
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
}
