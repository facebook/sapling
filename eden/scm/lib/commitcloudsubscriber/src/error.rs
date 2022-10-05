/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
    EventSourceNoContentType(),
}

lazy_static! {
    static ref RE: Regex = Regex::new(r"[\&\?]?access_token=\b\w+\b").unwrap();
}
