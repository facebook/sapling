/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use http::header::HeaderMap;
use http::status::StatusCode;
use http_client::HttpClientError;
use http_client::Method;
use thiserror::Error;
use url::Url;

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

#[derive(Clone, Debug, Error)]
#[error("{error}")]
pub struct ClonableError {
    #[source]
    pub error: Arc<Error>,
}

impl ClonableError {
    pub fn new(error: Error) -> Self {
        ClonableError {
            error: Arc::new(error),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Advice(Option<String>);

impl std::fmt::Display for Advice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            None => Ok(()),
            Some(advice) => write!(f, "Advice: {}", advice),
        }
    }
}

impl From<Option<String>> for Advice {
    fn from(opt_s: Option<String>) -> Self {
        Advice(opt_s)
    }
}

#[derive(Error, Debug)]
pub enum TransferError {
    #[error("HTTP status {}. Returned headers: {:#?}", .0, .1)]
    HttpStatus(StatusCode, HeaderMap),

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

    #[error("Unexpected HTTP Status. Expected {}, received {}", .expected, .received)]
    UnexpectedHttpStatus {
        expected: StatusCode,
        received: StatusCode,
    },

    #[error(transparent)]
    InvalidResponse(Error),
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_clonable_source() {
        let clonable: anyhow::Error = ClonableError::new(EmptyMutablePack {}.into()).into();
        assert!(clonable.chain().any(|e| e.is::<EmptyMutablePack>()))
    }
}
