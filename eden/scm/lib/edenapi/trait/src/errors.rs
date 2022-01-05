/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use auth::MissingCerts;
use auth::X509Error;
use edenapi_types::wire::WireToApiConversionError;
use edenapi_types::EdenApiServerError;
use http::status::StatusCode;
use http_client::HttpClientError;
use thiserror::Error;

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
    #[error("expected response, but none returned by the server")]
    NoResponse,
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

impl EdenApiError {
    pub fn is_retryable(&self) -> bool {
        use http_client::HttpClientError::*;
        use EdenApiError::*;
        match self {
            Http(client_error) => match client_error {
                Tls(_) => false,
                _ => true,
            },
            HttpError { status, message: _ } => {
                // 300-399
                if status.is_redirection() {
                    false
                // 400-499
                } else if status.is_client_error() {
                    match *status {
                        StatusCode::REQUEST_TIMEOUT => true,
                        StatusCode::TOO_MANY_REQUESTS => true,
                        _ => false,
                    }
                // 500-599
                } else if status.is_server_error() {
                    true
                } else {
                    false
                }
            }
            _ => false,
        }
    }
}
