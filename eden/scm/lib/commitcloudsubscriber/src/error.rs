/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use lazy_static::lazy_static;
use mime::Mime;
use regex::Regex;
use reqwest::StatusCode;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Commit Cloud `hg cloud sync` error: {0}")]
    CommitCloudHgCloudSyncError(String),
    #[error("Commit Cloud config error: {0}")]
    CommitCloudConfigError(&'static str),
    #[error("Commit Cloud EventSource HTTP error: {}", RE.replace_all(.0, ""))] // remove any token
    CommitCloudHttpError(String),
    #[error("Unexpected error: {0}")]
    CommitCloudUnexpectedError(String),
    #[error("EventSource: HTTP status code: {0}")]
    EventSourceHttp(StatusCode),
    #[error("EventSource: unexpected Content-Type: {0}")]
    EventSourceInvalidContentType(Mime),
    #[error("EventSource: Content-Type missing")]
    EventSourceNoContentType,
    #[error("Commit Cloud Updates Polling Failure: unauthorized")]
    PollingUpdatesUnauthorizedError,
    #[error("Commit Cloud Updates Polling Failure: HTTP status code: {0}")]
    PollingUpdatesHttpError(StatusCode),
    #[error("Commit Cloud Updates Polling Failure: received an error response: {0}")]
    PollingUpdatesServerError(String),
    #[error("Commit Cloud Updates Polling Failure: failed to parse payload field")]
    PollingUpdatesPayloadError,
}

lazy_static! {
    static ref RE: Regex = Regex::new(r"[\&\?]?access_token=\b\w+\b").unwrap();
}
