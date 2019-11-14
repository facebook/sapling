/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::{
    convert::TryInto,
    fmt::{self, Display},
    path::PathBuf,
};

use failure::{Backtrace, Context, Fail};
use http::StatusCode;

pub type ApiResult<T> = Result<T, ApiError>;

#[derive(Debug)]
pub struct ApiError {
    context: Context<ApiErrorKind>,
}

impl ApiError {
    pub fn kind(&self) -> &ApiErrorKind {
        &*self.context.get_context()
    }

    pub(crate) fn from_http(code: u32, msg: impl ToString) -> Self {
        let code = match code.try_into() {
            Ok(code) => match StatusCode::from_u16(code) {
                Ok(code) => code,
                Err(e) => return e.context(ApiErrorKind::BadResponse).into(),
            },
            Err(e) => return e.context(ApiErrorKind::BadResponse).into(),
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

impl Fail for ApiError {
    fn cause(&self) -> Option<&dyn Fail> {
        self.context.cause()
    }

    fn backtrace(&self) -> Option<&Backtrace> {
        self.context.backtrace()
    }
}

impl Display for ApiError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if let Some(error) = self.context.cause() {
            write!(f, "{}: {}", &self.context, &error)
        } else {
            write!(f, "{}", &self.context)
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
#[derive(Clone, Debug, Fail)]
pub enum ApiErrorKind {
    #[fail(display = "Client TLS certificate is missing or invalid: {:?}", _0)]
    BadCertificate(PathBuf),
    #[fail(display = "Invalid client configuration: {}", _0)]
    BadConfig(String),
    #[fail(display = "The server returned an unexpected or invalid response")]
    BadResponse,
    #[fail(display = "libcurl returned an error")]
    Curl,
    #[fail(display = "Received HTTP status '{}' with response: {:?}", code, msg)]
    Http { code: StatusCode, msg: String },
    #[fail(display = "Proxy server returned an error (HTTP {})", _0)]
    Proxy(StatusCode),
    #[fail(display = "Error during serialization/deserialization")]
    Serialization,
    #[fail(display = "Failed to write data to the store")]
    Store,
    #[fail(display = "A TLS error occurred")]
    Tls,
    #[fail(display = "Malformed URL")]
    Url,
    #[fail(display = "{}", _0)]
    Other(String),
}

impl From<Context<ApiErrorKind>> for ApiError {
    fn from(context: Context<ApiErrorKind>) -> Self {
        Self { context }
    }
}

impl From<ApiErrorKind> for ApiError {
    fn from(kind: ApiErrorKind) -> Self {
        Context::new(kind).into()
    }
}

impl From<Context<String>> for ApiError {
    fn from(context: Context<String>) -> Self {
        context.map(ApiErrorKind::Other).into()
    }
}

impl From<Context<&str>> for ApiError {
    fn from(context: Context<&str>) -> Self {
        context.map(String::from).into()
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
            error.context(ApiErrorKind::Tls).into()
        } else {
            error.context(ApiErrorKind::Curl).into()
        }
    }
}

impl From<curl::MultiError> for ApiError {
    fn from(error: curl::MultiError) -> Self {
        error.context(ApiErrorKind::Curl).into()
    }
}

impl From<serde_cbor::error::Error> for ApiError {
    fn from(error: serde_cbor::error::Error) -> Self {
        error.context(ApiErrorKind::Serialization).into()
    }
}

impl From<url::ParseError> for ApiError {
    fn from(error: url::ParseError) -> Self {
        error.context(ApiErrorKind::Url).into()
    }
}
