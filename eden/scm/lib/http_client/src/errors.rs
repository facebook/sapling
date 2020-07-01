/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use futures::channel::oneshot;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum HttpClientError {
    #[error(transparent)]
    Curl(#[from] curl::Error),
    #[error(transparent)]
    CurlMulti(#[from] curl::MultiError),
    #[error(transparent)]
    CallbackAborted(#[from] Abort),
    #[error("Received invalid or malformed HTTP response")]
    BadResponse,
    #[error("The request was dropped before it could complete")]
    RequestDropped(#[from] oneshot::Canceled),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// This error is a separate struct rather than a variant
/// of `HttpClientError` because it is the only error that
/// can occur when setting the TLS credential paths.
/// As such, the downstream crate can more easily report
/// the problem to the user without pattern matching.
#[derive(Error, Debug)]
#[error("TLS certificate or key not found: {0:?}")]
pub struct CertOrKeyMissing(pub PathBuf);

/// Error type for user-provided callbacks. Indicates
/// that the client should abort the operation and
/// return early. The user may optionally provide a
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
