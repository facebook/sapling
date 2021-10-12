/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

use auth::{MissingCerts, X509Error};
use edenapi_types::{wire::WireToApiConversionError, EdenApiServerError};
use http::status::StatusCode;
use http_client::HttpClientError;

#[derive(Debug, Error)]
pub enum EdenApiError {
    #[error("Failed to serialize request: {0}")]
    RequestSerializationFailed(#[source] serde_cbor::Error),
    #[error("Failed to parse response: {0}")]
    ParseResponse(String),
    #[error(transparent)]
    BadConfig(#[from] ConfigError),
    #[error(transparent)]
    MissingCertificate(#[from] MissingCerts),
    #[error(transparent)]
    BadCertificate(#[from] X509Error),
    #[error(transparent)]
    Http(#[from] HttpClientError),
    #[error("Server reported an error ({status}): {message}")]
    HttpError { status: StatusCode, message: String },
    #[error(transparent)]
    InvalidUrl(#[from] url::ParseError),
    #[error(transparent)]
    WireToApiConversionFailed(#[from] WireToApiConversionError),
    #[error(transparent)]
    ServerError(#[from] EdenApiServerError),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
    #[error("Not supported by the server")]
    NotSupported,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("Missing required config item: {0}")]
    Missing(String),
    #[error("Invalid config item: {0}")]
    Invalid(String, #[source] anyhow::Error),
}
