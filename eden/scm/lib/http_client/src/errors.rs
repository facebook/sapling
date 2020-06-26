/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::path::PathBuf;

use thiserror::Error;

#[derive(Error, Debug)]
pub enum HttpClientError {
    #[error("Received invalid status code: {0}")]
    InvalidStatusCode(u32),
    #[error(transparent)]
    Curl(#[from] curl::Error),
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
