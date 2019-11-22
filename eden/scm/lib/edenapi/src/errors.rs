/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    convert::TryInto,
    error::Error as StdError,
    fmt::{self, Display},
    path::PathBuf,
};

use anyhow::{Error, Result};
use http::StatusCode;
use thiserror::Error;

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
pub struct ApiError {
    cause: Option<Error>,
    kind: ApiErrorKind,
}

impl ApiError {
    pub fn new(kind: ApiErrorKind, cause: impl Into<Error>) -> Self {
        ApiError {
            cause: Some(cause.into()),
            kind,
        }
    }

    pub fn kind(&self) -> &ApiErrorKind {
        &self.kind
    }

    pub(crate) fn from_http(code: u32, msg: impl ToString) -> Self {
        let code = match code.try_into() {
            Ok(code) => match StatusCode::from_u16(code) {
                Ok(code) => code,
                Err(e) => return ApiError::new(ApiErrorKind::BadResponse, e),
            },
            Err(e) => return ApiError::new(ApiErrorKind::BadResponse, e),
        };
        let msg = msg.to_string();

        // XXX: Really crude heuristic. Typically, Mercurial will be configured to
        // talk to the API server through an HTTP proxy. When the proxy encounters
        // an error, it will generally return an HTML response since it assumes the
        // client is a browser. In contrast, the API server itself will never return
        // an HTML payload. As such, if we observe something that looks like an HTML
        // response, we can assume that it's an error from the proxy server.
        if msg.starts_with("<!DOCTYPE html") {
            return ApiErrorKind::Proxy(code).into();
        }

        ApiErrorKind::Http { code, msg }.into()
    }
}

impl StdError for ApiError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match &self.cause {
            Some(cause) => Some(&**cause),
            None => None,
        }
    }
}

impl Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(cause) = &self.cause {
            write!(f, "{}: {}", self.kind, cause)
        } else {
            write!(f, "{}", self.kind)
        }
    }
}

/// Enum representing the possible kinds of errors that can be returned by the
/// Eden API.
///
/// In general, the variants of this enum fall into two categories:
/// those that are used to tag an underlying error returned by another
/// crate, and those used to create a brand new error from scratch.
///
/// Variants in the former category typically do not have any associated values,
/// as it is expected that the underlying error will contain more specific
/// information about the underlying failure. The underlying error is available
/// as part of the `Context` in the returned [`ApiError`] struct and is printed
/// as part of the `Debug` implementation thereof.
///
/// Example:
/// ```rust,ignore
/// use curl::easy::Easy;
///
/// let mut handle = Easy::new();
///
/// // Do some setup...
///
/// // Tag this error as coming from libcurl before propagating it.
/// handle.perform().context(ApiErrorKind::Curl)?;
/// ```
///
/// Variants in the latter category do not have an associated underlying error,
/// and as such must contain sufficient information to understand the failure.
/// As such, these variants typically have associated values which are used
/// to provide a useful error message. These variants are typically constructed
/// at the site of the corresponding failure and inserted into an empty Context.
///
/// Example:
/// ```rust,ignore
/// if !certificate.is_file() {
///     // Create and return a new ApiError from scratch.
///     Err(ApiErrorKind::BadCertificate(certificate))?;
/// }
/// ```
///
/// This `Other` variant is a catch-all that can be used to return a custom
/// error message. This can be attached to an existing error via the `context`
/// method, as the appropriate conversions are defined for `String` and `&str`.
///
/// Example:
/// ```rust,ignore
/// // The string will be converted into an ApiError wrapping the underlying
/// // error. The Display implementation will just print the message while
/// // the Debug implementation will print both the message and the error.
/// my_function().context("An error occurred in my_function")?;
/// ```
#[derive(Clone, Debug, Error)]
pub enum ApiErrorKind {
    #[error("Client TLS certificate is missing or invalid: {0:?}")]
    BadCertificate(PathBuf),
    #[error("Invalid client configuration: {0}")]
    BadConfig(String),
    #[error("The server returned an unexpected or invalid response")]
    BadResponse,
    #[error("libcurl returned an error")]
    Curl,
    #[error("Received HTTP status '{code}' with response: {msg:?}")]
    Http { code: StatusCode, msg: String },
    #[error("Proxy server returned an error (HTTP {0})")]
    Proxy(StatusCode),
    #[error("Error during serialization/deserialization")]
    Serialization,
    #[error("Failed to write data to the store")]
    Store,
    #[error("A TLS error occurred")]
    Tls,
    #[error("Malformed URL")]
    Url,
    #[error("{0}")]
    Other(String),
}

pub trait ApiErrorContext<T> {
    fn context(self, kind: ApiErrorKind) -> ApiResult<T>;
}

impl<T, E: Into<Error>> ApiErrorContext<T> for Result<T, E> {
    fn context(self, kind: ApiErrorKind) -> ApiResult<T> {
        self.map_err(|e| ApiError::new(kind, e))
    }
}

impl From<ApiErrorKind> for ApiError {
    fn from(kind: ApiErrorKind) -> Self {
        ApiError { cause: None, kind }
    }
}

impl From<String> for ApiError {
    fn from(msg: String) -> Self {
        ApiErrorKind::Other(msg).into()
    }
}

impl From<&str> for ApiError {
    fn from(msg: &str) -> Self {
        msg.to_string().into()
    }
}

impl From<curl::Error> for ApiError {
    fn from(error: curl::Error) -> Self {
        if error.is_ssl_connect_error() {
            ApiError::new(ApiErrorKind::Tls, error)
        } else {
            ApiError::new(ApiErrorKind::Curl, error)
        }
    }
}

impl From<curl::MultiError> for ApiError {
    fn from(error: curl::MultiError) -> Self {
        ApiError::new(ApiErrorKind::Curl, error)
    }
}

impl From<serde_cbor::error::Error> for ApiError {
    fn from(error: serde_cbor::error::Error) -> Self {
        ApiError::new(ApiErrorKind::Serialization, error)
    }
}

impl From<url::ParseError> for ApiError {
    fn from(error: url::ParseError) -> Self {
        ApiError::new(ApiErrorKind::Url, error)
    }
}
