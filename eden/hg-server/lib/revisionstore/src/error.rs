/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::time::Duration;

use anyhow::Error;
use thiserror::Error;
use url::Url;

use http::status::StatusCode;
use http_client::{HttpClientError, Method};

#[derive(Debug, Error)]
#[error("Empty Mutable Pack")]
pub struct EmptyMutablePack;

#[derive(Error, Debug)]
#[error("Fetch failed: {} {}", .url, .method)]
pub struct FetchError {
    pub url: Url,
    pub method: Method,
    #[source]
    pub error: TransferError,
}

#[derive(Error, Debug)]
pub enum TransferError {
    #[error("HTTP status {}", .0)]
    HttpStatus(StatusCode),

    #[error("HTTP transfer failed")]
    HttpClientError(#[from] HttpClientError),

    #[error("Unexpected end of stream for http fetch")]
    EndOfStream,

    #[error("Timed out after waiting {:?}", .0)]
    Timeout(Duration),

    #[error(
        "Timed out after waiting {:?} for a chunk, after having received {} bytes over {}ms. Request ID: {}",
        .timeout, .bytes, .elapsed, .request_id
    )]
    ChunkTimeout {
        timeout: Duration,
        bytes: usize,
        elapsed: u128,
        request_id: String,
    },

    #[error(transparent)]
    InvalidResponse(Error),
}
