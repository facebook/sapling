/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Error;
use cxx::UniquePtr;
use edenapi::SaplingRemoteApiError;
use edenapi::types::SaplingRemoteApiServerError;
use edenapi::types::SaplingRemoteApiServerErrorKind;
use http_client::HttpClientError;
use revisionstore::error::LfsFetchError;
use revisionstore::error::LfsTransferError;

use crate::ffi::ffi::BackingStoreErrorKind;
use crate::ffi::ffi::SaplingBackingStoreError;
use crate::ffi::ffi::backingstore_error;
use crate::ffi::ffi::backingstore_error_with_code;

fn extract_http_client_error(err: &HttpClientError) -> (BackingStoreErrorKind, Option<i32>) {
    // The match statement is meant to be exhausitive without a default case to fall back into.
    // If a new error type is introduced, it's supposed to be explicitly handled here.
    // Consider updating SaplingBackingStoreError and EdenError if the existing definitions become insufficient.
    match err {
        HttpClientError::Curl(curl_err) => {
            (BackingStoreErrorKind::Network, Some(curl_err.code() as i32))
        }
        HttpClientError::CurlMulti(curl_err) => {
            (BackingStoreErrorKind::Network, Some(curl_err.code()))
        }
        HttpClientError::Tls(tls_err) => (
            BackingStoreErrorKind::Network,
            Some(tls_err.source.code() as i32),
        ),
        HttpClientError::CallbackAborted(_)
        | HttpClientError::BadResponse(_)
        | HttpClientError::RequestDropped(_)
        | HttpClientError::IoTaskFailed(_)
        | HttpClientError::CborError(_)
        | HttpClientError::CborStreamError(_)
        | HttpClientError::DecompressionFailed(_)
        | HttpClientError::Other(_) => (BackingStoreErrorKind::Network, None),
    }
}

fn extract_remote_api_server_error(
    err: &SaplingRemoteApiServerError,
) -> (BackingStoreErrorKind, Option<i32>) {
    // The match statement is meant to be exhausitive without a default case to
    // fall back into. If a new error type is introduced, it's supposed to be
    // explicitly handled here.
    match &err.err {
        SaplingRemoteApiServerErrorKind::OpaqueError(_) => (BackingStoreErrorKind::Network, None),
        SaplingRemoteApiServerErrorKind::PermissionDenied { .. } => {
            (BackingStoreErrorKind::PermissionDenied, None)
        }
    }
}

fn extract_remote_api_error(err: &SaplingRemoteApiError) -> (BackingStoreErrorKind, Option<i32>) {
    /*
     * The match statement is meant to be exhausitive without a default case to fall back into.
     * If a new error type is introduced, it's supposed to be explicitly handled here.
     * Consider updating SaplingBackingStoreError and EdenError if the existing definitions become insufficient.
     */
    match err {
        SaplingRemoteApiError::Http(client_err) => extract_http_client_error(client_err),
        SaplingRemoteApiError::HttpError { status, .. } => {
            (BackingStoreErrorKind::Network, Some(status.as_u16().into()))
        }
        SaplingRemoteApiError::ServerError(server_err) => {
            extract_remote_api_server_error(server_err)
        }
        SaplingRemoteApiError::NoResponse
        | SaplingRemoteApiError::IncompleteResponse(_)
        | SaplingRemoteApiError::ParseResponse(_) => (BackingStoreErrorKind::Network, None),
        SaplingRemoteApiError::RequestSerializationFailed(_) => (BackingStoreErrorKind::IO, None),
        SaplingRemoteApiError::BadConfig(_)
        | SaplingRemoteApiError::InvalidUrl(_)
        | SaplingRemoteApiError::MissingCerts(_)
        | SaplingRemoteApiError::NotSupported
        | SaplingRemoteApiError::WireToApiConversionFailed(_)
        | SaplingRemoteApiError::Other(_) => (BackingStoreErrorKind::Generic, None),
    }
}

fn extract_lfs_error(err: &LfsFetchError) -> (BackingStoreErrorKind, Option<i32>) {
    /*
     * The match statement is meant to be exhausitive without a default case to fall back into.
     * If a new error type is introduced, it's supposed to be explicitly handled here.
     * Consider updating SaplingBackingStoreError and EdenError if the existing definitions become insufficient.
     */
    match &err.error {
        LfsTransferError::HttpStatus(status_code, _) => (
            BackingStoreErrorKind::Network,
            Some(status_code.as_u16().into()),
        ),
        LfsTransferError::HttpClientError(client_err) => extract_http_client_error(client_err),
        LfsTransferError::UnexpectedHttpStatus { received, .. } => (
            BackingStoreErrorKind::Network,
            Some(received.as_u16().into()),
        ),
        LfsTransferError::EndOfStream
        | LfsTransferError::Timeout(_)
        | LfsTransferError::ChunkTimeout { .. }
        | LfsTransferError::InvalidResponse(_) => (BackingStoreErrorKind::Network, None),
    }
}

fn extract_indexedlog_error(err: &indexedlog::Error) -> BackingStoreErrorKind {
    /*
     * err.io_error_kind() is available to get specific IO error kinds, which can enable
     * more granular categorizations. However, there's no fool-proof conversion from io::ErrorKind
     * to POSIX errno or Win32 error codes. As this function is written, we don't see
     * many pure IO errors from inexedlog. We can revisit this when it becomes necessary.
     */
    match err.is_corruption() {
        true => BackingStoreErrorKind::DataCorruption,
        false => BackingStoreErrorKind::IO,
    }
}

fn classify_backingstore_error(err: &Error) -> (BackingStoreErrorKind, Option<i32>) {
    let mut kind = BackingStoreErrorKind::Generic;
    let mut code: Option<i32> = None;
    // Per-key batch tree fetches surface SaplingRemoteApiServerError directly,
    // while request-level failures wrap it in SaplingRemoteApiError::ServerError.
    // Check the direct server error first so both shapes classify the same way.
    for e in err.chain() {
        if let Some(e) = e.downcast_ref::<SaplingRemoteApiServerError>() {
            (kind, code) = extract_remote_api_server_error(e);
            break;
        } else if let Some(e) = e.downcast_ref::<SaplingRemoteApiError>() {
            (kind, code) = extract_remote_api_error(e);
            break;
        } else if let Some(e) = e.downcast_ref::<LfsFetchError>() {
            (kind, code) = extract_lfs_error(e);
            break;
        } else if let Some(e) = e.downcast_ref::<indexedlog::Error>() {
            kind = extract_indexedlog_error(e);
            break;
        }
    }
    (kind, code)
}

/// Translate anyhow errors from the backinstore
/// to SaplingBackingStoreError in C++ for EdenFS to consume
pub(crate) fn into_backingstore_err(err: Error) -> UniquePtr<SaplingBackingStoreError> {
    let msg = format!("{err:?}");
    let (kind, code) = classify_backingstore_error(&err);

    match code {
        Some(code) => backingstore_error_with_code(&msg, kind, code),
        None => backingstore_error(&msg, kind),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_remote_api_error() {
        use edenapi::types::SaplingRemoteApiServerError;
        use edenapi::types::SaplingRemoteApiServerErrorKind;
        use http_client::HttpClientError;
        use http_client::curl;
        use types::HgId;

        let test_cases = vec![
            (
                "NotSupported",
                SaplingRemoteApiError::NotSupported,
                BackingStoreErrorKind::Generic,
                None,
            ),
            (
                "NoResponse",
                SaplingRemoteApiError::NoResponse,
                BackingStoreErrorKind::Network,
                None,
            ),
            (
                "HttpError",
                SaplingRemoteApiError::HttpError {
                    status: http::StatusCode::NOT_FOUND,
                    message: "Not found".to_string(),
                    headers: Box::new(http::HeaderMap::new()),
                    url: "https://example.com".to_string(),
                },
                BackingStoreErrorKind::Network,
                Some(404),
            ),
            (
                "Http(Curl)",
                SaplingRemoteApiError::Http(HttpClientError::Curl(curl::Error::new(7))),
                BackingStoreErrorKind::Network,
                Some(7),
            ),
            (
                "ServerError(PermissionDenied)",
                SaplingRemoteApiError::ServerError(Box::new(SaplingRemoteApiServerError {
                    err: SaplingRemoteApiServerErrorKind::PermissionDenied {
                        tree_id: HgId::null_id().clone(),
                        request_acl: "test-acl".to_string(),
                    },
                    key: None,
                })),
                BackingStoreErrorKind::PermissionDenied,
                None,
            ),
            (
                "ServerError(OpaqueError)",
                SaplingRemoteApiError::ServerError(Box::new(SaplingRemoteApiServerError {
                    err: SaplingRemoteApiServerErrorKind::OpaqueError(
                        "internal server error".to_string(),
                    ),
                    key: None,
                })),
                BackingStoreErrorKind::Network,
                None,
            ),
        ];

        for (name, err, expected_kind, expected_code) in test_cases {
            let (kind, code) = extract_remote_api_error(&err);
            assert_eq!(
                kind, expected_kind,
                "{name} should map to the expected kind {expected_kind:?}"
            );
            assert_eq!(
                code, expected_code,
                "{name} should have code {expected_code:?}"
            );
        }
    }

    #[test]
    fn test_extract_lfs_error() {
        use http_client::Method;
        use revisionstore::error::LfsTransferError;
        use url::Url;

        let url = Url::parse("https://lfs.example.com").unwrap();

        let test_cases = vec![
            (
                "HttpStatus",
                LfsFetchError {
                    url: url.clone(),
                    method: Method::Get,
                    error: LfsTransferError::HttpStatus(
                        http::StatusCode::INTERNAL_SERVER_ERROR,
                        http::HeaderMap::new(),
                    ),
                },
                BackingStoreErrorKind::Network,
                Some(500),
            ),
            (
                "HttpClientError",
                LfsFetchError {
                    url: url.clone(),
                    method: Method::Post,
                    error: LfsTransferError::HttpClientError(HttpClientError::Curl(
                        http_client::curl::Error::new(7),
                    )),
                },
                BackingStoreErrorKind::Network,
                Some(7),
            ),
            (
                "UnexpectedHttpStatus",
                LfsFetchError {
                    url: url.clone(),
                    method: Method::Get,
                    error: LfsTransferError::UnexpectedHttpStatus {
                        expected: http::StatusCode::OK,
                        received: http::StatusCode::NOT_FOUND,
                    },
                },
                BackingStoreErrorKind::Network,
                Some(404),
            ),
            (
                "EndOfStream",
                LfsFetchError {
                    url: url.clone(),
                    method: Method::Get,
                    error: LfsTransferError::EndOfStream,
                },
                BackingStoreErrorKind::Network,
                None,
            ),
        ];

        for (name, err, expected_kind, expected_code) in test_cases {
            let (kind, code) = extract_lfs_error(&err);
            assert_eq!(
                kind, expected_kind,
                "{name} should map to the expected kind {expected_kind:?}"
            );
            assert_eq!(
                code, expected_code,
                "{name} should have code {expected_code:?}"
            );
        }
    }

    #[test]
    fn test_classify_backingstore_error_direct_server_error_permission_denied() {
        use edenapi::types::SaplingRemoteApiServerError;
        use edenapi::types::SaplingRemoteApiServerErrorKind;
        use types::HgId;

        let server_err = SaplingRemoteApiServerError {
            err: SaplingRemoteApiServerErrorKind::PermissionDenied {
                tree_id: HgId::null_id().clone(),
                request_acl: "test-acl".to_string(),
            },
            key: None,
        };
        let anyhow_err: anyhow::Error = server_err.into();
        let (kind, code) = classify_backingstore_error(&anyhow_err);
        assert_eq!(kind, BackingStoreErrorKind::PermissionDenied);
        assert_eq!(code, None);
    }

    #[test]
    fn test_classify_backingstore_error_direct_server_error_opaque_is_network() {
        use edenapi::types::SaplingRemoteApiServerError;
        use edenapi::types::SaplingRemoteApiServerErrorKind;

        let server_err = SaplingRemoteApiServerError {
            err: SaplingRemoteApiServerErrorKind::OpaqueError("internal server error".to_string()),
            key: None,
        };
        let anyhow_err: anyhow::Error = server_err.into();
        let (kind, code) = classify_backingstore_error(&anyhow_err);
        assert_eq!(kind, BackingStoreErrorKind::Network);
        assert_eq!(code, None);
    }
}
