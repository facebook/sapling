/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use futures::channel::oneshot;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum HttpClientError {
    #[error(transparent)]
    Curl(curl::Error),
    #[error(transparent)]
    Tls(#[from] TlsError),
    #[error(transparent)]
    CurlMulti(#[from] curl::MultiError),
    #[error(transparent)]
    CallbackAborted(#[from] Abort),
    #[error("Received invalid or malformed HTTP response")]
    BadResponse(anyhow::Error),
    #[error("The request was dropped before it could complete")]
    RequestDropped(#[from] oneshot::Canceled),
    #[error("The I/O task terminated unexpectedly: {}", .0)]
    IoTaskFailed(#[from] tokio::task::JoinError),
    #[error(transparent)]
    CborError(#[from] serde_cbor::Error),
    #[error(transparent)]
    CborStreamError(#[from] crate::stream::CborStreamError),
    #[error("Could not decode response: {}", .0)]
    DecompressionFailed(futures::io::Error),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

impl From<curl::Error> for HttpClientError {
    fn from(e: curl::Error) -> Self {
        TlsError::try_from(e).map_or_else(HttpClientError::Curl, HttpClientError::Tls)
    }
}

/// Error type for user-provided callbacks. Indicates that the client should
/// abort the operation and return early. The user may optionally provide a
/// reason for aborting.
#[derive(Error, Debug)]
pub enum Abort {
    #[error("Operation aborted by user callback: {0}")]
    WithReason(#[source] anyhow::Error),
    #[error("Operation aborted by user callback")]
    Unspecified,
}

impl Abort {
    pub fn abort<E: Into<anyhow::Error>>(reason: E) -> Self {
        Abort::WithReason(reason.into())
    }
}

/// A strongly-typed representation of all possible TLS-related error codes
/// from libcurl. It is useful to separate these from other kinds of libcurl
/// errors because the problem is often related to the client's configuration
/// (e.g., expired client certificate or wrong CA bundle).
///
/// Full list of error codes: https://curl.se/libcurl/c/libcurl-errors.html
#[derive(Error, Debug)]
#[error("Try renewing your certificates. Run `eden doctor`. TlsError: {source}")]
pub struct TlsError {
    pub source: curl::Error,
    pub kind: TlsErrorKind,
}

impl TryFrom<curl::Error> for TlsError {
    type Error = curl::Error;

    fn try_from(source: curl::Error) -> Result<Self, Self::Error> {
        match TlsErrorKind::from_curl_error(&source) {
            Some(kind) => Ok(Self { source, kind }),
            None => Err(source),
        }
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum TlsErrorKind {
    RecvError = curl_sys::CURLE_RECV_ERROR as isize,
    CaCert = curl_sys::CURLE_SSL_CACERT as isize,
    CaCertBadFile = curl_sys::CURLE_SSL_CACERT_BADFILE as isize,
    CertProblem = curl_sys::CURLE_SSL_CERTPROBLEM as isize,
    Cipher = curl_sys::CURLE_SSL_CIPHER as isize,
    ConnectError = curl_sys::CURLE_SSL_CONNECT_ERROR as isize,
    CrlBadFile = curl_sys::CURLE_SSL_CRL_BADFILE as isize,
    EngineInitFailed = curl_sys::CURLE_SSL_ENGINE_INITFAILED as isize,
    EngineNotFound = curl_sys::CURLE_SSL_ENGINE_NOTFOUND as isize,
    EngineSetFailed = curl_sys::CURLE_SSL_ENGINE_SETFAILED as isize,
    InvalidCertStatus = curl_sys::CURLE_SSL_INVALIDCERTSTATUS as isize,
    IssuerError = curl_sys::CURLE_SSL_ISSUER_ERROR as isize,
    PinnedPubKeyNotMatch = curl_sys::CURLE_SSL_PINNEDPUBKEYNOTMATCH as isize,
    ShutdownFailed = curl_sys::CURLE_SSL_SHUTDOWN_FAILED as isize,
}

impl TlsErrorKind {
    fn from_curl_error(source: &curl::Error) -> Option<Self> {
        use TlsErrorKind::*;

        Some(match source.code() {
            curl_sys::CURLE_RECV_ERROR => ssl_categorize_recv_error(source),
            curl_sys::CURLE_SSL_CACERT => CaCert,
            curl_sys::CURLE_SSL_CACERT_BADFILE => CaCertBadFile,
            curl_sys::CURLE_SSL_CERTPROBLEM => CertProblem,
            curl_sys::CURLE_SSL_CIPHER => Cipher,
            curl_sys::CURLE_SSL_CONNECT_ERROR => ConnectError,
            curl_sys::CURLE_SSL_CRL_BADFILE => CrlBadFile,
            curl_sys::CURLE_SSL_ENGINE_INITFAILED => EngineInitFailed,
            curl_sys::CURLE_SSL_ENGINE_NOTFOUND => EngineNotFound,
            curl_sys::CURLE_SSL_ENGINE_SETFAILED => EngineSetFailed,
            curl_sys::CURLE_SSL_INVALIDCERTSTATUS => InvalidCertStatus,
            curl_sys::CURLE_SSL_ISSUER_ERROR => IssuerError,
            curl_sys::CURLE_SSL_PINNEDPUBKEYNOTMATCH => PinnedPubKeyNotMatch,
            curl_sys::CURLE_SSL_SHUTDOWN_FAILED => ShutdownFailed,
            _ => return None,
        })
    }
}

/// XXX: When libcurl's underlying TLS engine (e.g., OpenSSL) encounters an
/// error during the transfer, libcurl might report a generic error (such as
/// CURLE_RECV_ERROR) instead of a TLS-specific error. In these cases, the
/// "extra description" field of the error usually contains the actual error
/// message from the TLS engine. This function tries to detect this situation
/// using a crude string search heuristic.
fn ssl_categorize_recv_error(error: &curl::Error) -> TlsErrorKind {
    let extra = error.extra_description();
    if let Some(extra) = extra {
        let extra = extra.to_lowercase();
        // Covers: "alert certificate required" and "alert bad certificate"
        if extra.contains("certificate") {
            return TlsErrorKind::CertProblem;
        }
    }

    TlsErrorKind::RecvError
}
