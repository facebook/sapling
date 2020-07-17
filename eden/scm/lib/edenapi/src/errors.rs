/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

use http_client::{CertOrKeyMissing, HttpClientError};

#[derive(Debug, Error)]
pub enum EdenApiError {
    #[error("Failed to serialize request: {0}")]
    RequestSerializationFailed(#[source] serde_cbor::Error),
    #[error("Empty request")]
    EmptyRequest,
    #[error(transparent)]
    BadConfig(#[from] ConfigError),
    #[error(transparent)]
    MissingCreds(#[from] CertOrKeyMissing),
    #[error(transparent)]
    Http(#[from] HttpClientError),
    #[error(transparent)]
    InvalidUrl(#[from] url::ParseError),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("No server URL specified")]
    MissingUrl,
    #[error("Invalid server URL: {0}")]
    InvalidUrl(#[source] url::ParseError),
    #[error("Config field '{0}' is malformed")]
    Malformed(String, #[source] anyhow::Error),
}
