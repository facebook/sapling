/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
        use http_client::TlsErrorKind::*;
        use EdenApiError::*;
        match self {
            Http(client_error) => match client_error {
                Tls(tls_error) => match tls_error.kind {
                    ConnectError | RecvError => true,
                    // Don't retry if there are general auth issues.
                    _ => false,
                },
                _ => true,
            },
            HttpError { .. } => true,
            _ => false,
        }
    }
}
